use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tetonic_core::{Agent, AgentConfig, Conversation};
use tetonic_domain::{AgentInvocation, IdentityId, RunCommand};
use tetonic_inference::{ChatRequest, ChatResponse, InferenceError, InferenceProvider, TokenSink};

struct RejectCancellation(Arc<dyn tetonic_run::RunSupervisor>);
#[async_trait::async_trait]
impl tetonic_run::RunSupervisor for RejectCancellation {
    async fn handle(
        &self,
        command: RunCommand,
    ) -> Result<tetonic_domain::RunCommandResult, tetonic_domain::RunSupervisorError> {
        if matches!(command, RunCommand::CancelRun(_)) {
            return Err(tetonic_domain::RunSupervisorError::Persistence(
                "injected cancellation write failure".into(),
            ));
        }
        self.0.handle(command).await
    }
    async fn snapshot(
        &self,
        run: RunId,
    ) -> Result<RunSnapshot, tetonic_domain::RunSupervisorError> {
        self.0.snapshot(run).await
    }
    async fn resume_from_sequence(
        &self,
        run: RunId,
        after: u64,
        limit: u32,
    ) -> Result<Result<Vec<RunEventEnvelope>, ReplayGap>, tetonic_domain::RunSupervisorError> {
        self.0.resume_from_sequence(run, after, limit).await
    }
}

struct PendingProvider {
    entered: tokio::sync::mpsc::UnboundedSender<()>,
    calls: Arc<AtomicUsize>,
}
#[async_trait::async_trait]
impl InferenceProvider for PendingProvider {
    async fn chat(
        &self,
        _: ChatRequest,
        _: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let _ = self.entered.send(());
        std::future::pending().await
    }
}
#[derive(Default)]
struct Events(Mutex<Vec<ApplicationEvent>>);
impl ApplicationEventSink for Events {
    fn send(&self, ev: ApplicationEvent) {
        self.0.lock_recover().push(ev);
    }
}
fn command() -> StartIdentityJobCommand {
    let identity = AgentIdentity {
        id: IdentityId::new("actor"),
        owning_application: "test".into(),
        bound_definition_digest: "definition".into(),
        privilege_class: "default".into(),
        toolset_subscriptions: vec![],
        context_bindings: vec![],
        recovery_id: "actor".into(),
    };
    StartIdentityJobCommand {
        job_spec: AgentJobSpec {
            identity_id: identity.id.clone(),
            definition_digest: "definition".into(),
            input_digest: job_input_digest("input"),
            capability_bindings: vec![],
            artifact_bindings: vec![],
            recovery_id: "job".into(),
        },
        identity,
        invocation: AgentInvocation {
            instructions: "do work".into(),
            user_input: "input".into(),
            max_steps: 2,
            explain_turn: false,
            empty_tool_nudge: false,
            completion_tool: "finish".into(),
            discipline: Default::default(),
        },
    }
}
fn manager(tmp: &tempfile::TempDir, events: Arc<Events>, reject_cancel: bool) -> DefaultRunService {
    let store = tetonic_memory::SharedStore::open(tmp.path().join("run.db"), 1).unwrap();
    let supervisor = Arc::new(tetonic_run::DurableRunSupervisor::new(Some(store.clone())));
    let supervisor: Arc<dyn tetonic_run::RunSupervisor> = if reject_cancel {
        Arc::new(RejectCancellation(supervisor))
    } else {
        supervisor
    };
    let artifacts = Arc::new(
        tetonic_artifact::LocalArtifactStore::new(
            tmp.path().join("artifacts"),
            crate::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );
    DefaultRunService::new(
        Some(store),
        Arc::new(tetonic_policy::PolicyEngine::default()),
        events,
        supervisor,
        Arc::new(crate::SessionLiveStore::new()),
        artifacts,
    )
    .with_execution_policy(Arc::new(|_, _, _, _, _| Ok(())))
}
fn agent(
    tmp: &tempfile::TempDir,
    calls: Arc<AtomicUsize>,
    entered: tokio::sync::mpsc::UnboundedSender<()>,
) -> Agent {
    Agent::new(
        Arc::new(PendingProvider { entered, calls }),
        tetonic_tools::Tools::new(tetonic_tools::Workspace::new(tmp.path()).unwrap(), false),
        AgentConfig::default(),
    )
}

#[tokio::test]
async fn admitted_attempt_executes_its_revision_after_identity_update() {
    let tmp = tempfile::tempdir().unwrap();
    let runs = manager(&tmp, Arc::new(Events::default()), false);
    let cmd = command();
    let binding = runs
        .managed
        .admit(
            &runs.managed.reserve_dispatch().id,
            tetonic_run::AdmitJob {
                identity: cmd.identity.clone(),
                job_spec: cmd.job_spec.clone(),
                role: None,
                parent_attempt: None,
            },
        )
        .await
        .unwrap();
    let mut newer = cmd.identity.clone();
    newer.bound_definition_digest = "definition-v2".into();
    newer.context_bindings = vec!["different-context".into()];
    runs.managed
        .store()
        .unwrap()
        .write(move |db| tetonic_run::put_identity(db, &newer))
        .await
        .unwrap()
        .unwrap();
    let (tx, mut entered) = tokio::sync::mpsc::unbounded_channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut executor = agent(&tmp, calls.clone(), tx);
    let mut conversation = Conversation::default();
    let mut on_step = |_| {};
    let execution = runs.managed.execute_attempt(
        binding.attempt_id,
        &mut executor,
        &mut conversation,
        cmd.invocation,
        &mut on_step,
    );
    tokio::pin!(execution);
    tokio::select! {
        outcome = &mut execution => panic!("old revision was rejected before inference: {outcome:?}"),
        received = entered.recv() => assert!(received.is_some()),
        _ = tokio::time::sleep(std::time::Duration::from_secs(3)) => panic!("inference was never reached"),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    runs.cancel_run(CancelByRunCommand {
        run_id: binding.run_id.0,
    })
    .await
    .unwrap();
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(3), execution)
        .await
        .unwrap();
    assert!(matches!(outcome, CandidateOutcome::Canceled { .. }));
}
#[tokio::test]
async fn cancel_detached_and_localset_start_resolves_waiter_and_cleans_registry() {
    for (detached, reject_cancel) in [(false, false), (true, false), (true, true)] {
        let tmp = tempfile::tempdir().unwrap();
        let events = Arc::new(Events::default());
        let runs = manager(&tmp, events.clone(), reject_cancel);
        let (tx, mut entered) = tokio::sync::mpsc::unbounded_channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut executor = agent(&tmp, calls.clone(), tx);
        tokio::task::LocalSet::new()
            .run_until(async {
                let execute = async {
                    if detached {
                        let (_, rx) = runs.submit_identity_job(command(), executor).await.unwrap();
                        rx.await.unwrap()
                    } else {
                        runs.start_identity_job(command(), &mut executor)
                            .await
                            .unwrap()
                    }
                };
                let cancel = async {
                    entered.recv().await.unwrap();
                    let run_id = events
                        .0
                        .lock_recover()
                        .iter()
                        .find_map(|event| match event {
                            ApplicationEvent::RunStatus { run_id, .. } => Some(RunId::new(run_id)),
                            _ => None,
                        })
                        .expect("manager emitted start");
                    let result = runs
                        .cancel_run(CancelByRunCommand {
                            run_id: run_id.0.clone(),
                        })
                        .await;
                    if reject_cancel {
                        assert!(result.is_err());
                    } else {
                        result.unwrap();
                    }
                    let state = runs.managed.inspect_run(&run_id).await.unwrap().state;
                    assert_eq!(state == tetonic_domain::RunState::Canceled, !reject_cancel);
                };
                let (result, _) = tokio::time::timeout(std::time::Duration::from_secs(3), async {
                    tokio::join!(execute, cancel)
                })
                .await
                .unwrap();
                if reject_cancel {
                    assert!(matches!(result.outcome, CandidateOutcome::Failed { .. }));
                } else {
                    assert!(matches!(result.outcome, CandidateOutcome::Canceled { .. }));
                    assert!(runs.managed.binding(&result.attempt_id).is_none());
                }
            })
            .await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let events = events.0.lock_recover();
        let terminal: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                ApplicationEvent::TurnCompleted { status, .. } => Some(status),
                _ => None,
            })
            .collect();
        assert_eq!(
            terminal,
            vec![if reject_cancel { "error" } else { "canceled" }]
        );
    }
}
#[tokio::test]
async fn missing_durable_identity_denies_dispatch_even_with_empty_capabilities() {
    for missing_store in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let mut runs = manager(&tmp, Arc::new(Events::default()), false);
        let policy_calls = Arc::new(AtomicUsize::new(0));
        let counter = policy_calls.clone();
        runs = runs.with_execution_policy(Arc::new(move |_, _, _, _, _| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
        let cmd = command();
        let active = runs
            .begin_job_run(None, &cmd.identity, cmd.job_spec, None)
            .await
            .unwrap();
        let attempt = active.attempt_id.clone();
        let run = active.run_id.clone();
        // Remove or corrupt the persisted identity after admission, preserving
        // the actual manager and its registry to exercise execution-time reads.
        let conn = rusqlite::Connection::open(tmp.path().join("run.db")).unwrap();
        if missing_store {
            conn.execute("DROP TABLE agent_identity_revisions", [])
                .unwrap();
        } else {
            conn.execute("DELETE FROM agent_identity_revisions", [])
                .unwrap();
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let (tx, _) = tokio::sync::mpsc::unbounded_channel();
        let mut executor = agent(&tmp, calls.clone(), tx);
        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            runs.execute_bound_attempt(
                attempt.clone(),
                &mut executor,
                &mut Conversation::new(),
                cmd.invocation,
                &mut |_| {},
            ),
        )
        .await
        .unwrap();
        assert!(matches!(outcome, CandidateOutcome::Failed { .. }));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(policy_calls.load(Ordering::SeqCst), 0);
        assert!(
            !runs.managed.inspect_run(&run).await.unwrap().attempts[&attempt].execution_claimed
        );
    }
}

#[tokio::test]
async fn full_invocation_policy_denies_changes_before_claim_or_inference() {
    for field in [
        "instructions",
        "completion",
        "discipline",
        "explain",
        "nudge",
        "steps",
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let cmd = command();
        let expected = cmd.invocation.clone();
        let policy_calls = Arc::new(AtomicUsize::new(0));
        let counter = policy_calls.clone();
        let runs = manager(&tmp, Arc::new(Events::default()), false).with_execution_policy(
            Arc::new(move |_, _, _, _, invocation| {
                counter.fetch_add(1, Ordering::SeqCst);
                if invocation != &expected {
                    return Err("invocation differs from prepared revision".into());
                }
                Ok(())
            }),
        );
        let active = runs
            .begin_job_run(None, &cmd.identity, cmd.job_spec, None)
            .await
            .unwrap();
        let mut invocation = cmd.invocation;
        match field {
            "instructions" => invocation.instructions = "replacement instructions".into(),
            "completion" => invocation.completion_tool = "other".into(),
            "discipline" => invocation.discipline.finish_min_chars = Some(100),
            "explain" => invocation.explain_turn = true,
            "nudge" => invocation.empty_tool_nudge = true,
            "steps" => invocation.max_steps = 1,
            _ => unreachable!(),
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let (tx, _) = tokio::sync::mpsc::unbounded_channel();
        let mut executor = agent(&tmp, calls.clone(), tx);
        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            runs.execute_bound_attempt(
                active.attempt_id.clone(),
                &mut executor,
                &mut Conversation::new(),
                invocation,
                &mut |_| {},
            ),
        )
        .await
        .unwrap();
        assert!(
            matches!(outcome, CandidateOutcome::Failed { ref message }
            if message == "invocation differs from prepared revision"),
            "{field}: {outcome:?}"
        );
        assert_eq!(policy_calls.load(Ordering::SeqCst), 1, "{field}");
        assert_eq!(calls.load(Ordering::SeqCst), 0, "{field}");
        assert!(
            !runs
                .managed
                .inspect_run(&active.run_id)
                .await
                .unwrap()
                .attempts[&active.attempt_id]
                .execution_claimed,
            "{field}"
        );
    }
}

#[tokio::test]
async fn registered_general_revision_completes_through_existing_managed_runtime() {
    let tmp = tempfile::tempdir().unwrap();
    let local = crate::resources::LocalControl::open(tmp.path().join("run.db"), "test".into())
        .await
        .unwrap();
    local
        .bootstrap("admin".into(), "org".into(), "Org".into())
        .await
        .unwrap();
    let credential = local
        .credentials()
        .issue("admin".into(), 3600)
        .await
        .unwrap();
    let resource = local.resources();
    let registered = resource
        .register_agent(
            credential.expose_secret(),
            "org".into(),
            "agent".into(),
            "general".into(),
            serde_json::json!({"instructions":"Analyze the question"}),
        )
        .await
        .unwrap();
    let prepared = resource
        .prepare_general_revision(
            credential.expose_secret(),
            "org".into(),
            "agent".into(),
            registered.identity.bound_definition_digest.clone(),
            "A question".into(),
            crate::resources::HarnessPreparationLimits {
                max_steps: 2,
                max_input_bytes: 1024,
            },
        )
        .await
        .unwrap();
    // Trusted test composition only: definition conformance is not an employee
    // execution grant. Reuse the real store, admission, claim and executor.
    let legacy_events = Arc::new(Events::default());
    let runs = manager(&tmp, legacy_events.clone(), false)
        .with_execution_policy(prepared.execution_policy().unwrap());
    let id = IdentityId::new(registered.identity.identity_id);
    let digest = registered.identity.bound_definition_digest;
    let identity = runs
        .managed
        .store()
        .unwrap()
        .read(move |db| tetonic_run::get_identity_revision(db, &id, &digest))
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let spec = AgentJobSpec {
        identity_id: identity.id.clone(),
        definition_digest: identity.bound_definition_digest.clone(),
        input_digest: job_input_digest(&prepared.invocation().user_input),
        capability_bindings: vec![],
        artifact_bindings: vec![],
        recovery_id: "general-job".into(),
    };
    let contexts = local.contexts();
    contexts
        .create(
            credential.expose_secret(),
            "private".into(),
            crate::resources::ContextOwner::Private {
                org_id: "org".into(),
            },
        )
        .await
        .unwrap();
    let authorization = contexts
        .bind_stored_execution_grant(
            credential.expose_secret(),
            "org".into(),
            "private".into(),
            "agent".into(),
            identity.bound_definition_digest.clone(),
            "job-grant".into(),
        )
        .await
        .unwrap();
    // Reading both the definition and private context does not grant execution.
    assert!(authorization
        .authority
        .authorize(&authorization.scope, &identity, &spec)
        .await
        .is_err());
    let grant = tetonic_memory::ExecutionGrant {
        grant_id: "job-grant".into(),
        scope: authorization.scope.clone(),
        job: spec.clone(),
        expires_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 3600,
    };
    resource
        .issue_execution_grant(credential.expose_secret(), grant)
        .await
        .unwrap();
    let mut substituted = authorization.scope.clone();
    substituted.organization_id = "other-org".into();
    assert!(authorization
        .authority
        .authorize(&substituted, &identity, &spec)
        .await
        .is_err());
    let active = runs
        .managed
        .admit_with_context(
            &runs.managed.reserve_dispatch().id,
            tetonic_run::AdmitJob {
                identity: identity.clone(),
                job_spec: spec.clone(),
                role: None,
                parent_attempt: None,
            },
            tetonic_run::managed::AdmissionContext {
                authorization: Some(authorization.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let reopened = tetonic_memory::SharedStore::open(tmp.path().join("run.db"), 1).unwrap();
    let supervisor = tetonic_run::DurableRunSupervisor::new(Some(reopened));
    let snapshot = tetonic_run::RunSupervisor::snapshot(&supervisor, active.run_id.clone())
        .await
        .unwrap();
    assert_eq!(
        snapshot.tasks[&active.task_id]
            .binding
            .execution_grant_id
            .as_deref(),
        Some("job-grant")
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let tools =
        tetonic_tools::Tools::new(tetonic_tools::Workspace::new(tmp.path()).unwrap(), false)
            .with_allowed_tools(["finish".to_string()].into_iter().collect());
    let mut executor = Agent::new(
        Arc::new(GeneralFinisher {
            calls: calls.clone(),
        }),
        tools,
        AgentConfig::default(),
    );
    let mut conversation = Conversation::new();
    let mut on_step = |_| {};
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        runs.execute_bound_attempt(
            active.attempt_id.clone(),
            &mut executor,
            &mut conversation,
            prepared.invocation().clone(),
            &mut on_step,
        ),
    )
    .await
    .unwrap();
    assert!(
        matches!(outcome, CandidateOutcome::Completed { ref summary, .. } if summary == "GOVERNEDRESULTCANARY")
    );
    let finalized = runs
        .managed
        .finalize(tetonic_run::FinalizeJob {
            attempt: active.attempt_id.clone(),
            outcome,
            policy: None,
            finish_run: true,
        })
        .await
        .unwrap();
    assert!(matches!(finalized, CandidateOutcome::Completed { .. }));
    let snapshot = tetonic_run::RunSupervisor::snapshot(&supervisor, active.run_id.clone())
        .await
        .unwrap();
    assert_eq!(snapshot.state, tetonic_domain::RunState::Succeeded);
    let receipt = snapshot.tasks[&active.task_id]
        .accepted_artifact
        .as_ref()
        .unwrap();
    let output_id = tetonic_domain::ArtifactId::new(receipt.artifact_id.clone());
    let output = contexts
        .bind_artifacts(
            credential.expose_secret(),
            "private".into(),
            runs.managed.artifacts().clone(),
        )
        .await
        .unwrap();
    let mut reader = output.open(&output_id).await.unwrap();
    let mut bytes = [0u8; 1024];
    let count = reader.read_chunk(&mut bytes).await.unwrap();
    assert!(String::from_utf8_lossy(&bytes[..count]).contains("GOVERNEDRESULTCANARY"));
    contexts
        .create(
            credential.expose_secret(),
            "other".into(),
            crate::resources::ContextOwner::Private {
                org_id: "org".into(),
            },
        )
        .await
        .unwrap();
    let other = contexts
        .bind_artifacts(
            credential.expose_secret(),
            "other".into(),
            runs.managed.artifacts().clone(),
        )
        .await
        .unwrap();
    assert!(other.open(&output_id).await.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let revoked_attempt = runs
        .managed
        .admit_with_context(
            &runs.managed.reserve_dispatch().id,
            tetonic_run::AdmitJob {
                identity: identity.clone(),
                job_spec: spec.clone(),
                role: None,
                parent_attempt: None,
            },
            tetonic_run::managed::AdmissionContext {
                authorization: Some(authorization.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    resource
        .revoke_execution_grant(credential.expose_secret(), "org".into(), "job-grant".into())
        .await
        .unwrap();
    assert!(authorization
        .authority
        .authorize(&authorization.scope, &identity, &spec)
        .await
        .is_err());

    let (revoked_tx, _) = tokio::sync::mpsc::unbounded_channel();
    let mut revoked_executor = agent(&tmp, calls.clone(), revoked_tx);
    let denied = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        runs.execute_bound_attempt(
            revoked_attempt.attempt_id.clone(),
            &mut revoked_executor,
            &mut Conversation::new(),
            prepared.invocation().clone(),
            &mut |_| {},
        ),
    )
    .await
    .unwrap();
    assert!(
        matches!(denied, CandidateOutcome::Failed { ref message } if message == "execution authorization denied")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        !runs
            .managed
            .inspect_run(&revoked_attempt.run_id)
            .await
            .unwrap()
            .attempts[&revoked_attempt.attempt_id]
            .execution_claimed
    );
    local
        .credentials()
        .revoke(credential.credential_id.clone())
        .await
        .unwrap();
    assert!(authorization
        .authority
        .authorize(&authorization.scope, &identity, &spec)
        .await
        .is_err());
    assert!(output.open(&output_id).await.is_err());

    assert!(
        runs.managed
            .inspect_run(&active.run_id)
            .await
            .unwrap()
            .attempts[&active.attempt_id]
            .execution_claimed
    );
    assert!(
        legacy_events.0.lock_recover().is_empty(),
        "scoped events reached the unscoped product sink"
    );
}

#[tokio::test]
async fn admitted_policy_cannot_be_replaced_by_dispatching_through_another_handle() {
    let tmp = tempfile::tempdir().unwrap();
    let runs = manager(&tmp, Arc::new(Events::default()), false);
    let policy_calls = Arc::new(AtomicUsize::new(0));
    let mut admitted = Vec::new();
    for label in ["first", "second"] {
        let counter = policy_calls.clone();
        let admission = runs
            .managed
            .as_ref()
            .clone()
            .with_execution_policy(Arc::new(move |_, _, _, _, _| {
                counter.fetch_add(1, Ordering::SeqCst);
                Err(format!("pinned {label}"))
            }));
        let cmd = command();
        let binding = admission
            .admit(
                &admission.reserve_dispatch().id,
                tetonic_run::AdmitJob {
                    identity: cmd.identity,
                    job_spec: cmd.job_spec,
                    role: None,
                    parent_attempt: None,
                },
            )
            .await
            .unwrap();
        admitted.push((label, binding, cmd.invocation));
    }
    // This handle still has the permissive test default. Neither admitted
    // validator may be replaced by that default or the other agent's policy.
    let calls = Arc::new(AtomicUsize::new(0));
    for (label, binding, invocation) in admitted {
        let (tx, _) = tokio::sync::mpsc::unbounded_channel();
        let mut executor = agent(&tmp, calls.clone(), tx);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            runs.execute_bound_attempt(
                binding.attempt_id.clone(),
                &mut executor,
                &mut Conversation::new(),
                invocation,
                &mut |_| {},
            ),
        )
        .await
        .unwrap();
        assert!(matches!(result, CandidateOutcome::Failed { ref message }
            if message == &format!("pinned {label}")));
        assert!(
            !runs
                .managed
                .inspect_run(&binding.run_id)
                .await
                .unwrap()
                .attempts[&binding.attempt_id]
                .execution_claimed
        );
    }
    assert_eq!(policy_calls.load(Ordering::SeqCst), 2);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

struct TestExecutionAuthority(Arc<std::sync::atomic::AtomicBool>);
#[async_trait::async_trait]
impl tetonic_run::managed::ExecutionAuthority for TestExecutionAuthority {
    async fn authorize(
        &self,
        _: &tetonic_domain::ExecutionScope,
        _: &AgentIdentity,
        _: &AgentJobSpec,
    ) -> Result<(), ()> {
        if self.0.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(())
        }
    }
}

#[tokio::test]
async fn execution_scope_persists_and_revocation_blocks_claim_and_delegation() {
    let tmp = tempfile::tempdir().unwrap();
    let runs = manager(&tmp, Arc::new(Events::default()), false);
    let allowed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let scope = tetonic_domain::ExecutionScope {
        principal_id: "alice".into(),
        organization_id: "org".into(),
        information_context_id: "private".into(),
    };
    let context = tetonic_run::managed::AdmissionContext {
        authorization: Some(tetonic_run::managed::AuthorizedExecution {
            grant_id: Some("test-grant".into()),
            scope: scope.clone(),
            authority: Arc::new(TestExecutionAuthority(allowed.clone())),
        }),
        ..Default::default()
    };
    let cmd = command();
    let job = || tetonic_run::AdmitJob {
        identity: cmd.identity.clone(),
        job_spec: cmd.job_spec.clone(),
        role: None,
        parent_attempt: None,
    };
    let denied_ticket = runs.managed.reserve_dispatch();
    assert!(runs
        .managed
        .admit_with_context(&denied_ticket.id, job(), context.clone())
        .await
        .is_err());
    let identity_id = cmd.identity.id.clone();
    assert!(runs
        .managed
        .store()
        .unwrap()
        .read(move |db| tetonic_run::get_identity(db, &identity_id))
        .await
        .unwrap()
        .unwrap()
        .is_none());
    allowed.store(true, Ordering::SeqCst);
    let mut session_context = context.clone();
    session_context.session_id = Some(tetonic_domain::SessionId::new("legacy-or-scoped"));
    assert!(runs
        .managed
        .admit_with_context(&runs.managed.reserve_dispatch().id, job(), session_context)
        .await
        .is_err());
    let binding = runs
        .managed
        .admit_with_context(&runs.managed.reserve_dispatch().id, job(), context)
        .await
        .unwrap();
    // Reopen storage and reconstruct the supervisor to prove this is durable
    // task state rather than merely a process-local authorization attachment.
    let reopened = tetonic_memory::SharedStore::open(tmp.path().join("run.db"), 1).unwrap();
    let supervisor = tetonic_run::DurableRunSupervisor::new(Some(reopened));
    let snapshot = tetonic_run::RunSupervisor::snapshot(&supervisor, binding.run_id.clone())
        .await
        .unwrap();
    assert_eq!(
        snapshot.tasks[&binding.task_id]
            .binding
            .execution_scope
            .as_ref(),
        Some(&scope)
    );
    let mut child = job();
    child.parent_attempt = Some(binding.attempt_id.clone());
    assert!(runs
        .managed
        .admit(&runs.managed.reserve_dispatch().id, child)
        .await
        .is_err());
    allowed.store(false, Ordering::SeqCst);
    let calls = Arc::new(AtomicUsize::new(0));
    let (tx, _) = tokio::sync::mpsc::unbounded_channel();
    let mut executor = agent(&tmp, calls.clone(), tx);
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        runs.execute_bound_attempt(
            binding.attempt_id.clone(),
            &mut executor,
            &mut Conversation::new(),
            cmd.invocation,
            &mut |_| {},
        ),
    )
    .await
    .unwrap();
    assert!(matches!(outcome, CandidateOutcome::Failed { ref message }
        if message == "execution authorization denied"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(
        !runs
            .managed
            .inspect_run(&binding.run_id)
            .await
            .unwrap()
            .attempts[&binding.attempt_id]
            .execution_claimed
    );
}

struct RevokingProvider(Arc<std::sync::atomic::AtomicBool>);
#[async_trait::async_trait]
impl InferenceProvider for RevokingProvider {
    async fn chat(
        &self,
        _: ChatRequest,
        _: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        self.0.store(false, Ordering::SeqCst);
        Ok(ChatResponse {
            message: tetonic_inference::Message::assistant("").with_tool_calls(vec![tetonic_inference::ToolCall {
                function: tetonic_inference::FunctionCall { name: "write_file".into(),
                    arguments: serde_json::json!({"path":"unauthorized.txt","content":"must not execute"}) },
            }]),
            usage: Default::default(), provenance: Default::default(),
        })
    }
}

#[tokio::test]
async fn revocation_during_inference_blocks_the_returned_tool_action() {
    let tmp = tempfile::tempdir().unwrap();
    let runs = manager(&tmp, Arc::new(Events::default()), false);
    let allowed = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let cmd = command();
    let binding = runs
        .managed
        .admit_with_context(
            &runs.managed.reserve_dispatch().id,
            tetonic_run::AdmitJob {
                identity: cmd.identity,
                job_spec: cmd.job_spec,
                role: None,
                parent_attempt: None,
            },
            tetonic_run::managed::AdmissionContext {
                authorization: Some(tetonic_run::managed::AuthorizedExecution {
                    grant_id: None,
                    scope: tetonic_domain::ExecutionScope {
                        principal_id: "alice".into(),
                        organization_id: "org".into(),
                        information_context_id: "private".into(),
                    },
                    authority: Arc::new(TestExecutionAuthority(allowed.clone())),
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let mut executor = Agent::new(
        Arc::new(RevokingProvider(allowed)),
        tetonic_tools::Tools::new(tetonic_tools::Workspace::new(tmp.path()).unwrap(), false),
        AgentConfig::default(),
    );
    let mut steps = Vec::new();
    let result = runs
        .execute_bound_attempt(
            binding.attempt_id,
            &mut executor,
            &mut Conversation::new(),
            cmd.invocation,
            &mut |step| steps.push(step),
        )
        .await;
    assert!(
        matches!(result, CandidateOutcome::Failed { ref message } if message=="execution authorization denied")
    );
    assert!(!tmp.path().join("unauthorized.txt").exists());
    assert!(!steps
        .iter()
        .any(|step| matches!(step, tetonic_core::Step::ToolCall { .. })));
}

struct RevokingFinalizationDriver {
    authority: Arc<std::sync::atomic::AtomicBool>,
    verifies: Arc<AtomicUsize>,
    commits: Arc<AtomicUsize>,
}
impl tetonic_run::FinalizationEffectDriver for RevokingFinalizationDriver {
    fn bind_effect_identity(
        &self,
        _: &tetonic_domain::TaskId,
        _: &tetonic_domain::AttemptId,
    ) -> Result<(), String> {
        Ok(())
    }
    fn run_verify(
        &self,
        _: &str,
        _: &tetonic_domain::work_scope::CancellationSignal,
    ) -> Result<(), (String, Option<String>)> {
        self.verifies.fetch_add(1, Ordering::SeqCst);
        self.authority.store(false, Ordering::SeqCst);
        Ok(())
    }
    fn commit_workspace(&self) -> Result<Option<tetonic_domain::CommitResult>, String> {
        self.commits.fetch_add(1, Ordering::SeqCst);
        Ok(None)
    }
}
#[tokio::test]
async fn finalization_revocation_prevents_verify_or_commit_and_records_failure() {
    for revoke_before in [true, false] {
        let tmp = tempfile::tempdir().unwrap();
        let runs = manager(&tmp, Arc::new(Events::default()), false);
        let allowed = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let cmd = command();
        let binding = runs
            .managed
            .admit_with_context(
                &runs.managed.reserve_dispatch().id,
                tetonic_run::AdmitJob {
                    identity: cmd.identity,
                    job_spec: cmd.job_spec,
                    role: None,
                    parent_attempt: None,
                },
                tetonic_run::managed::AdmissionContext {
                    authorization: Some(tetonic_run::managed::AuthorizedExecution {
                        grant_id: None,
                        scope: tetonic_domain::ExecutionScope {
                            principal_id: "alice".into(),
                            organization_id: "org".into(),
                            information_context_id: "private".into(),
                        },
                        authority: Arc::new(TestExecutionAuthority(allowed.clone())),
                    }),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        if revoke_before {
            allowed.store(false, Ordering::SeqCst);
        }
        let verifies = Arc::new(AtomicUsize::new(0));
        let commits = Arc::new(AtomicUsize::new(0));
        let outcome = runs
            .managed
            .finalize(tetonic_run::FinalizeJob {
                attempt: binding.attempt_id.clone(),
                outcome: CandidateOutcome::Completed {
                    summary: "done".into(),
                    kind: tetonic_domain::CompletionKind::Answer,
                },
                policy: Some(tetonic_run::FinalizationPolicy {
                    effect_driver: Some(Arc::new(RevokingFinalizationDriver {
                        authority: allowed,
                        verifies: verifies.clone(),
                        commits: commits.clone(),
                    })),
                    verify_cmd: Some("verify".into()),
                }),
                finish_run: true,
            })
            .await
            .unwrap();
        assert!(
            matches!(outcome,CandidateOutcome::Failed { ref message } if message=="execution authorization denied during finalization")
        );
        assert_eq!(
            verifies.load(Ordering::SeqCst),
            if revoke_before { 0 } else { 1 }
        );
        assert_eq!(commits.load(Ordering::SeqCst), 0);
        let snapshot = runs.managed.inspect_run(&binding.run_id).await.unwrap();
        assert_eq!(snapshot.state, tetonic_domain::RunState::Failed);
    }
}

#[tokio::test]
async fn governed_final_output_uses_existing_private_artifact_access() {
    for fail_binding in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let local = crate::resources::LocalControl::open(tmp.path().join("run.db"), "test".into())
            .await
            .unwrap();
        local
            .bootstrap("alice".into(), "org".into(), "Org".into())
            .await
            .unwrap();
        let credential = local
            .credentials()
            .issue("alice".into(), 3600)
            .await
            .unwrap();
        let contexts = local.contexts();
        for context in ["private", "other"] {
            contexts
                .create(
                    credential.expose_secret(),
                    context.into(),
                    crate::resources::ContextOwner::Private {
                        org_id: "org".into(),
                    },
                )
                .await
                .unwrap();
        }
        let runs = manager(&tmp, Arc::new(Events::default()), false);
        let cmd = command();
        let binding = runs
            .managed
            .admit_with_context(
                &runs.managed.reserve_dispatch().id,
                tetonic_run::AdmitJob {
                    identity: cmd.identity,
                    job_spec: cmd.job_spec,
                    role: None,
                    parent_attempt: None,
                },
                tetonic_run::managed::AdmissionContext {
                    authorization: Some(tetonic_run::managed::AuthorizedExecution {
                        grant_id: None,
                        scope: tetonic_domain::ExecutionScope {
                            principal_id: "alice".into(),
                            organization_id: "org".into(),
                            information_context_id: "private".into(),
                        },
                        authority: Arc::new(TestExecutionAuthority(Arc::new(
                            std::sync::atomic::AtomicBool::new(true),
                        ))),
                    }),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        if fail_binding {
            let conn = rusqlite::Connection::open(tmp.path().join("run.db")).unwrap();
            conn.execute_batch("CREATE TRIGGER fail_output_binding BEFORE INSERT ON context_artifacts BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
        }
        let outcome = runs
            .managed
            .finalize(tetonic_run::FinalizeJob {
                attempt: binding.attempt_id.clone(),
                outcome: CandidateOutcome::Completed {
                    summary: "PRIVATEOUTPUTCANARY".into(),
                    kind: tetonic_domain::CompletionKind::Answer,
                },
                policy: None,
                finish_run: true,
            })
            .await
            .unwrap();
        let snapshot = runs.managed.inspect_run(&binding.run_id).await.unwrap();
        let receipt = &snapshot.tasks[&binding.task_id].accepted_artifact;
        if fail_binding {
            assert!(matches!(outcome, CandidateOutcome::Failed { .. }));
            assert_eq!(snapshot.state, tetonic_domain::RunState::Failed);
            assert!(receipt.is_none());
        } else {
            assert!(matches!(outcome, CandidateOutcome::Completed { .. }));
            let id = tetonic_domain::ArtifactId::new(receipt.as_ref().unwrap().artifact_id.clone());
            let scoped = contexts
                .bind_artifacts(
                    credential.expose_secret(),
                    "private".into(),
                    runs.managed.artifacts().clone(),
                )
                .await
                .unwrap();
            let foreign = contexts
                .bind_artifacts(
                    credential.expose_secret(),
                    "other".into(),
                    runs.managed.artifacts().clone(),
                )
                .await
                .unwrap();
            let mut reader = scoped.open(&id).await.unwrap();
            let mut bytes = [0u8; 1024];
            let count = reader.read_chunk(&mut bytes).await.unwrap();
            assert!(String::from_utf8_lossy(&bytes[..count]).contains("PRIVATEOUTPUTCANARY"));
            assert!(foreign.open(&id).await.is_err());
            local
                .credentials()
                .revoke(credential.credential_id.clone())
                .await
                .unwrap();
            assert!(scoped.open(&id).await.is_err());
        }
    }
}

struct GeneralFinisher {
    calls: Arc<AtomicUsize>,
}
#[async_trait::async_trait]
impl InferenceProvider for GeneralFinisher {
    async fn chat(
        &self,
        request: ChatRequest,
        _: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(request
            .messages
            .iter()
            .any(|m| m.role == "system" && m.content.contains("Analyze the question")));
        assert!(request
            .messages
            .iter()
            .any(|m| m.role == "user" && m.content.contains("A question")));
        assert_eq!(request.tools.len(), 1);
        assert_eq!(request.tools[0].function.name, "finish");
        Ok(ChatResponse {
            message: tetonic_inference::Message::assistant("").with_tool_calls(vec![
                tetonic_inference::ToolCall {
                    function: tetonic_inference::FunctionCall {
                        name: "finish".into(),
                        arguments: serde_json::json!({"summary":"GOVERNEDRESULTCANARY"}),
                    },
                },
            ]),
            usage: Default::default(),
            provenance: Default::default(),
        })
    }
}

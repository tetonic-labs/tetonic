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
        let policy_calls = Arc::new(AtomicUsize::new(0));
        let counter = policy_calls.clone();
        runs = runs.with_execution_policy(Arc::new(move |_, _, _, _, _| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
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

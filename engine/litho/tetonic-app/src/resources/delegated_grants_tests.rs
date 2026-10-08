use super::*;
use crate::{events::RecordingEventSink, Application, ComputePlaneRequest};
use tetonic_run::managed::{AdmissionContext, AdmitJob};

#[tokio::test]
async fn live_parent_credential_revocation_reaches_child_authority_and_runtime() {
    delegation_scenario(Scenario::AuthorityOnly).await;
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scenario {
    AuthorityOnly,
    Complete,
    Revoke,
    ParentFinishes,
    QueuedRevoke,
}

#[tokio::test]
async fn governed_child_executes_once_in_parent_run_and_shares_usage() {
    delegation_scenario(Scenario::Complete).await;
}

#[tokio::test]
async fn parent_revocation_quiesces_parent_and_active_child_without_refunding_unknown_spend() {
    delegation_scenario(Scenario::Revoke).await;
}

#[tokio::test]
async fn parent_cannot_finish_successfully_with_a_live_child() {
    delegation_scenario(Scenario::ParentFinishes).await;
}

#[tokio::test]
async fn child_queued_at_single_local_runtime_stops_on_parent_revocation() {
    delegation_scenario(Scenario::QueuedRevoke).await;
}

async fn delegation_scenario(scenario: Scenario) {
    for lifetime in [
        tetonic_memory::DelegationLifetime::ParentLease,
        tetonic_memory::DelegationLifetime::ParentWork,
    ] {
        delegation_scenario_with_lifetime(scenario, lifetime).await;
    }
}

async fn delegation_scenario_with_lifetime(
    scenario: Scenario,
    lifetime: tetonic_memory::DelegationLifetime,
) {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("control.db");
    let local = LocalControl::open(database.clone(), "test".into())
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
    let secret = credential.expose_secret();
    let child_credential = local
        .credentials()
        .issue("admin".into(), 3600)
        .await
        .unwrap();
    let child_secret = child_credential.expose_secret();
    let resources = local.resources();
    resources
        .create_team(secret, "org".into(), "team".into(), "Team".into())
        .await
        .unwrap();
    local
        .contexts()
        .create(
            secret,
            "shared".into(),
            ContextOwner::Team {
                org_id: "org".into(),
                team_id: "team".into(),
            },
        )
        .await
        .unwrap();
    let limits = || HarnessPreparationLimits {
        human_handoff: false,
        work_director: false,
        max_steps: 2,
        max_input_bytes: 1024,
    };
    let mut prepared = Vec::new();
    for key in ["lead", "worker"] {
        let agent = resources.register_agent(secret, "org".into(), key.into(), "general".into(),
            serde_json::json!({"instructions":"Answer the assigned question", "requested_tools":["finish"], "max_steps":2})).await.unwrap();
        let revision = resources
            .prepare_general_revision(
                secret,
                "org".into(),
                key.into(),
                agent.identity.bound_definition_digest,
                "Explain the work".into(),
                limits(),
            )
            .await
            .unwrap();
        prepared.push(revision.start_command(format!("job-{key}")).unwrap());
    }
    let parent = prepared.remove(0);
    let child = prepared.remove(0);
    let expiry = chrono::Utc::now().timestamp() + 3600;
    resources
        .issue_execution_grant(
            secret,
            tetonic_memory::ExecutionGrant {
                grant_id: "parent-grant".into(),
                scope: ExecutionScope {
                    principal_id: "admin".into(),
                    organization_id: "org".into(),
                    information_context_id: "shared".into(),
                },
                job: parent.job_spec.clone(),
                expires_at: expiry,
            },
        )
        .await
        .unwrap();
    resources
        .create_team_work_item(
            secret,
            crate::resources::CreateTeamWorkItem {
                org: "org".into(),
                team: "team".into(),
                work_id: "root".into(),
                title: "Explain the work".into(),
                request_id: "root-request".into(),
                goal_id: None,
            },
        )
        .await
        .unwrap();
    resources
        .authorize_work_budget(
            secret,
            "org".into(),
            "team".into(),
            "root".into(),
            "fund".into(),
            100,
        )
        .await
        .unwrap();
    resources
        .create_work_delegation(
            secret,
            crate::resources::CreateWorkDelegation {
                org: "org".into(),
                team: "team".into(),
                delegation_id: "delegation".into(),
                parent_work_id: "root".into(),
                child_work_id: "child".into(),
                child_title: "Explain the work".into(),
                request_id: "delegation-request".into(),
                parent_budget_tokens: 100,
                child_budget_tokens: 40,
                stop_scope: "inherit".into(),
                peer_org: None,
                peer_team: None,
            },
        )
        .await
        .unwrap();
    let store = SharedStore::open(database, 1).unwrap();
    let (sink, _) = RecordingEventSink::new();
    let app = Application::bootstrap_mock_with_store(dir.path(), Some(store.clone()), sink, vec![]);
    let (url, child_url, requests, parent_response, child_response, server) =
        delegation_inference_server().await;
    let guard = Arc::new(tetonic_egress::EgressGuard::new());
    guard.configure_loopback_inference(url.rsplit(':').next().unwrap().parse().unwrap());
    let plane = crate::build_compute_plane(ComputePlaneRequest {
        guard,
        ollama_base: url,
        policy: app.host.runtime.policy().clone(),
        workspace_root: dir.path().to_path_buf(),
        artifact_store: app.host.runtime.artifact_store().clone(),
        store: Some(store.clone()),
        coordinator: None,
        placement_sink: None,
        previous_pooled: None,
    })
    .await;
    app.install_compute_services(&plane);
    let work = async {
        let (_, submission) = app
            .activate_team_work(
                secret,
                local.credentials().clone(),
                TeamWorkLaunch {
                    organization_id: "org".into(),
                    team_id: "team".into(),
                    work_id: "root".into(),
                    information_context_id: "shared".into(),
                    agent_key: "lead".into(),
                    definition_digest: parent.job_spec.definition_digest.clone(),
                    execution_grant_id: "parent-grant".into(),
                    input: None,
                    recovery_id: "job-lead".into(),
                },
                RegisteredExecutionSettings {
                    skills: None,
                    mcp: None,
                    plan_dispatch: None,
                    response_schema: None,
                    hosted: None,
                    max_elapsed_seconds: 30,
                    reported_token_ceiling: if scenario == Scenario::AuthorityOnly {
                        None
                    } else {
                        Some(40)
                    },
                    workspace_root: None,
                    model: "qwen3.5:latest".into(),
                    num_ctx: 8192,
                    data_class: tetonic_domain::DataClass::Secret,
                    allowed_tools: Default::default(),
                    limits: limits(),
                },
            )
            .await
            .unwrap();
        let execution = submission.execution.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if !requests.lock().unwrap().is_empty() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the real managed parent must reach inference");
        let parent_handle = app
            .run_manager
            .managed()
            .delegation_parent(&execution.attempt_id)
            .unwrap();
        let request = tetonic_memory::DelegatedGrantRequest {
            lifetime,
            approved_environment: if scenario == Scenario::Complete {
                Some(
                    RegisteredExecutionSettings {
                        skills: None,
                        mcp: None,
                        plan_dispatch: None,
                        response_schema: None,
                        hosted: None,
                        max_elapsed_seconds: 120,
                        reported_token_ceiling: Some(40),
                        workspace_root: None,
                        model: "qwen3.5:latest".into(),
                        num_ctx: 8192,
                        data_class: tetonic_domain::DataClass::Secret,
                        allowed_tools: Default::default(),
                        limits: limits(),
                    }
                    .environment_binding(&child.job_spec.capability_bindings)
                    .unwrap(),
                )
            } else {
                None
            },
            request_id: "derive".into(),
            grant_id: "child-grant".into(),
            parent_grant_id: "parent-grant".into(),
            delegation_id: "delegation".into(),
            job: child.job_spec.clone(),
            expires_at: expiry - 1,
        };
        let derived = resources
            .derive_execution_grant(secret, "org".into(), "team".into(), request.clone())
            .await
            .unwrap();
        assert_eq!(derived.lineage.parent_attempt_id, execution.attempt_id);
        assert_eq!(derived.lineage.parent_run_id, submission.run_id.0);
        assert_eq!(
            derived,
            resources
                .derive_execution_grant(secret, "org".into(), "team".into(), request)
                .await
                .unwrap()
        );
        let contexts = local.contexts();
        let root_authority = contexts
            .bind_stored_execution_grant(
                secret,
                "org".into(),
                "shared".into(),
                "worker".into(),
                child.job_spec.definition_digest.clone(),
                "child-grant".into(),
            )
            .await
            .unwrap();
        assert!(root_authority
            .authority
            .authorize(&root_authority.scope, &child.identity, &child.job_spec)
            .await
            .is_err());
        let inherited = contexts
            .bind_delegated_execution_grant(
                child_secret,
                parent_handle.clone(),
                crate::resources::BindDelegatedExecutionGrant {
                    org: "org".into(),
                    context: "shared".into(),
                    agent_key: "worker".into(),
                    definition_digest: child.job_spec.definition_digest.clone(),
                    grant_id: "child-grant".into(),
                },
            )
            .await
            .unwrap();
        assert!(inherited
            .authority
            .authorize(&inherited.scope, &child.identity, &child.job_spec)
            .await
            .is_ok());
        // Even a valid inherited authority cannot bypass root admission by being
        // passed through the generic host API. No child run/provider call exists.
        let ticket = app.run_manager.managed().reserve_dispatch();
        let bypass = app
            .run_manager
            .managed()
            .admit_with_context(
                &ticket.id,
                AdmitJob {
                    identity: child.identity.clone(),
                    job_spec: child.job_spec.clone(),
                    role: None,
                    parent_attempt: None,
                },
                AdmissionContext {
                    authorization: Some(inherited.clone()),
                    ..Default::default()
                },
            )
            .await;
        assert!(
            matches!(bypass,Err(tetonic_run::ManagedRunError::InvalidRequest(ref text)) if text.contains("delegated grant"))
        );
        app.run_manager
            .managed()
            .release_dispatch(&ticket.id)
            .await
            .unwrap();
        if scenario != Scenario::AuthorityOnly {
            if scenario != Scenario::QueuedRevoke {
                // Two separate local inference runtimes, each retaining its own
                // residency admission. Existing executions retain their provider;
                // subsequent child assembly uses the host's second endpoint.
                let guard = Arc::new(tetonic_egress::EgressGuard::new());
                guard.configure_loopback_inference(
                    child_url.rsplit(':').next().unwrap().parse().unwrap(),
                );
                let child_plane = crate::build_compute_plane(ComputePlaneRequest {
                    guard,
                    ollama_base: child_url,
                    policy: app.host.runtime.policy().clone(),
                    workspace_root: dir.path().into(),
                    artifact_store: app.host.runtime.artifact_store().clone(),
                    store: Some(store.clone()),
                    coordinator: None,
                    placement_sink: None,
                    previous_pooled: None,
                })
                .await;
                app.install_compute_services(&child_plane);
            }
            let launch = || TeamWorkLaunch {
                organization_id: "org".into(),
                team_id: "team".into(),
                work_id: "child".into(),
                information_context_id: "shared".into(),
                agent_key: "worker".into(),
                definition_digest: child.job_spec.definition_digest.clone(),
                execution_grant_id: "child-grant".into(),
                input: None,
                recovery_id: "job-worker".into(),
            };
            let settings = || RegisteredExecutionSettings {
                skills: None,
                mcp: None,
                plan_dispatch: None,
                response_schema: None,
                hosted: None,
                max_elapsed_seconds: 120,
                reported_token_ceiling: Some(40),
                workspace_root: None,
                model: "qwen3.5:latest".into(),
                num_ctx: if scenario == Scenario::QueuedRevoke {
                    16384
                } else {
                    8192
                },
                data_class: tetonic_domain::DataClass::Secret,
                allowed_tools: Default::default(),
                limits: limits(),
            };
            let mut broader = settings();
            broader.workspace_root = Some(dir.path().into());
            assert!(app
                .activate_delegated_team_work(
                    child_secret,
                    local.credentials().clone(),
                    launch(),
                    broader,
                    parent_handle.clone()
                )
                .await
                .is_err());
            if scenario == Scenario::Complete {
                for change in 0..3 {
                    let mut changed = settings();
                    match change {
                        0 => changed.model = "substituted-model".into(),
                        1 => changed.reported_token_ceiling = Some(41),
                        _ => changed.num_ctx = 4096,
                    }
                    assert!(app
                        .activate_delegated_team_work(
                            child_secret,
                            local.credentials().clone(),
                            launch(),
                            changed,
                            parent_handle.clone()
                        )
                        .await
                        .is_err());
                }
            }
            // Matching retry keys in two team namespaces cannot redirect the
            // inherited grant to the other team's work or budget.
            resources
                .create_team(secret, "org".into(), "other-team".into(), "Other".into())
                .await
                .unwrap();
            resources
                .create_team_work_item(
                    secret,
                    crate::resources::CreateTeamWorkItem {
                        org: "org".into(),
                        team: "other-team".into(),
                        work_id: "other-root".into(),
                        title: "Other".into(),
                        request_id: "other-root-request".into(),
                        goal_id: None,
                    },
                )
                .await
                .unwrap();
            resources
                .authorize_work_budget(
                    secret,
                    "org".into(),
                    "other-team".into(),
                    "other-root".into(),
                    "other-fund".into(),
                    100,
                )
                .await
                .unwrap();
            resources
                .create_work_delegation(
                    secret,
                    crate::resources::CreateWorkDelegation {
                        org: "org".into(),
                        team: "other-team".into(),
                        delegation_id: "other-delegation".into(),
                        parent_work_id: "other-root".into(),
                        child_work_id: "other-child".into(),
                        child_title: "Explain the work".into(),
                        request_id: "delegation-request".into(),
                        parent_budget_tokens: 100,
                        child_budget_tokens: 40,
                        stop_scope: "inherit".into(),
                        peer_org: None,
                        peer_team: None,
                    },
                )
                .await
                .unwrap();
            let mut wrong_work = launch();
            wrong_work.team_id = "other-team".into();
            wrong_work.work_id = "other-child".into();
            assert!(app
                .activate_delegated_team_work(
                    child_secret,
                    local.credentials().clone(),
                    wrong_work,
                    settings(),
                    parent_handle.clone()
                )
                .await
                .is_err());
            // The parent occupies the first slot. A child cannot avoid that
            // count by being another task within the same run.
            store
                .write(|db| db.set_team_execution_limits("admin", "org", "team", 1, 1))
                .await
                .unwrap()
                .unwrap();
            assert!(app
                .activate_delegated_team_work(
                    child_secret,
                    local.credentials().clone(),
                    launch(),
                    settings(),
                    parent_handle.clone()
                )
                .await
                .is_err());
            store
                .write(|db| db.set_team_execution_limits("admin", "org", "team", 2, 3))
                .await
                .unwrap()
                .unwrap();
            let (first, second) = tokio::join!(
                app.activate_delegated_team_work(
                    child_secret,
                    local.credentials().clone(),
                    launch(),
                    settings(),
                    parent_handle.clone()
                ),
                app.activate_delegated_team_work(
                    child_secret,
                    local.credentials().clone(),
                    launch(),
                    settings(),
                    parent_handle.clone()
                )
            );
            let (_, first) = first.unwrap();
            let (_, second) = second.unwrap();
            assert_eq!(first.run_id, submission.run_id);
            assert_eq!(second.run_id, submission.run_id);
            assert_eq!(first.task_id, second.task_id);
            assert_ne!(first.task_id, submission.task_id);
            assert_eq!(first.audit_session_id, second.audit_session_id);
            assert_ne!(
                first.execution.is_some(),
                second.execution.is_some(),
                "duplicate delivery must have only one worker"
            );
            let mut child_execution = first.execution.or(second.execution).unwrap();
            let reached = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    if scenario == Scenario::QueuedRevoke {
                        if store
                            .read(|db| db.team_work_usage("admin", "org", "team"))
                            .await
                            .unwrap()
                            .unwrap()
                            .iter()
                            .any(|row| row.work_id == "child" && row.pending_calls == 1)
                        {
                            break;
                        }
                    } else if requests.lock().unwrap().len() >= 2 {
                        break;
                    }
                    if let Ok(done) = child_execution.completion.try_recv() {
                        panic!("child ended before inference: {:?}", done.outcome);
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            })
            .await;
            if reached.is_err() {
                panic!("child must reach brokered inference");
            }
            let snapshot = app
                .run_manager
                .managed()
                .inspect_run(&submission.run_id)
                .await
                .unwrap();
            assert_eq!(snapshot.tasks.len(), 2);
            assert_eq!(snapshot.attempts.len(), 2);
            assert_eq!(
                snapshot
                    .tasks
                    .values()
                    .filter(|task| task.binding.activation.is_some())
                    .count(),
                1
            );
            assert_eq!(
                snapshot.tasks[&first.task_id].binding.deadline,
                snapshot.tasks[&submission.task_id].binding.deadline,
                "a child cannot extend its parent's deadline"
            );
            assert_eq!(
                snapshot.tasks
                    [&child_execution_binding_task(&snapshot, &child_execution.attempt_id)]
                    .binding
                    .delegation
                    .as_ref()
                    .unwrap()
                    .parent_attempt,
                execution.attempt_id
            );
            assert_eq!(
                resources
                    .work_budget(child_secret, "org".into(), "team".into(), "root".into())
                    .await
                    .unwrap()
                    .available_tokens,
                20
            );
            if scenario == Scenario::Complete {
                child_response.notify_one();
                let done = tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    child_execution.completion,
                )
                .await
                .unwrap()
                .unwrap();
                assert!(done.outcome.is_completed(), "{:?}", done.outcome);
                let (_, replay) = app
                    .activate_delegated_team_work(
                        child_secret,
                        local.credentials().clone(),
                        launch(),
                        settings(),
                        parent_handle.clone(),
                    )
                    .await
                    .unwrap();
                assert!(replay.execution.is_none());
                let mut changed = launch();
                changed.input = Some("Different assignment".into());
                assert!(app
                    .activate_delegated_team_work(
                        child_secret,
                        local.credentials().clone(),
                        changed,
                        settings(),
                        parent_handle.clone()
                    )
                    .await
                    .is_err());
                parent_response.notify_one();
                let done =
                    tokio::time::timeout(std::time::Duration::from_secs(5), execution.completion)
                        .await
                        .unwrap()
                        .unwrap();
                assert!(done.outcome.is_completed(), "{:?}", done.outcome);
                let snapshot = app
                    .run_manager
                    .managed()
                    .inspect_run(&submission.run_id)
                    .await
                    .unwrap();
                assert_eq!(snapshot.state, tetonic_domain::RunState::Succeeded);
                assert!(snapshot
                    .attempts
                    .values()
                    .all(|attempt| attempt.execution_quiesced));
                let usage = store
                    .read(|db| db.team_work_usage("admin", "org", "team"))
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    usage
                        .iter()
                        .map(|row| row.input_tokens + row.output_tokens)
                        .sum::<i64>(),
                    60
                );
                assert_eq!(usage.iter().map(|row| row.held_tokens).sum::<i64>(), 0);
                assert_eq!(usage.iter().map(|row| row.released_tokens).sum::<i64>(), 20);
                assert_eq!(requests.lock().unwrap().len(), 2);
                return;
            }
            if matches!(scenario, Scenario::Revoke | Scenario::QueuedRevoke) {
                local
                    .credentials()
                    .revoke(credential.credential_id.clone())
                    .await
                    .unwrap();
            } else {
                parent_response.notify_one();
            }
            let (parent_done, child_done) =
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    tokio::join!(execution.completion, child_execution.completion)
                })
                .await
                .unwrap();
            let parent_outcome = parent_done.unwrap().outcome;
            let child_outcome = child_done.unwrap().outcome;
            assert!(
                matches!(
                    parent_outcome,
                    tetonic_domain::CandidateOutcome::Canceled { .. }
                ),
                "parent outcome: {parent_outcome:?}"
            );
            assert!(
                matches!(
                    child_outcome,
                    tetonic_domain::CandidateOutcome::Canceled { .. }
                ),
                "child outcome: {child_outcome:?}"
            );
            let snapshot = app
                .run_manager
                .managed()
                .inspect_run(&submission.run_id)
                .await
                .unwrap();
            assert_eq!(snapshot.state, tetonic_domain::RunState::Canceled);
            assert!(snapshot
                .attempts
                .values()
                .all(|attempt| attempt.execution_quiesced));
            let usage = store
                .read(|db| db.team_work_usage("admin", "org", "team"))
                .await
                .unwrap()
                .unwrap();
            let child_usage = usage.iter().find(|row| row.work_id == "child").unwrap();
            assert_eq!(child_usage.held_tokens, 40);
            assert_eq!(child_usage.released_tokens, 0);
            assert!(requests.lock().unwrap().len() <= 2);
            return;
        }
        // Child has a separate, still-valid credential. Revoking the parent's
        // original credential must nevertheless remove inherited authority.
        local
            .credentials()
            .revoke(credential.credential_id.clone())
            .await
            .unwrap();
        assert!(
            inherited
                .authority
                .revoked_during_execution(&inherited.scope, &child.identity, &child.job_spec)
                .await
        );
        assert!(resources
            .get_execution_grant(child_secret, "org".into(), "parent-grant".into())
            .await
            .unwrap()
            .is_some());
        let completed =
            tokio::time::timeout(std::time::Duration::from_secs(5), execution.completion)
                .await
                .unwrap()
                .unwrap();
        assert!(
            matches!(
                completed.outcome,
                tetonic_domain::CandidateOutcome::Canceled { .. }
            ),
            "{:?}",
            completed.outcome
        );
        let run = app
            .run_manager
            .managed()
            .inspect_run(&submission.run_id)
            .await
            .unwrap();
        assert_eq!(run.state, tetonic_domain::RunState::Canceled);
        assert!(app
            .run_manager
            .managed()
            .delegation_parent(&parent_handle.binding().attempt_id)
            .is_err());
        assert!(parent_handle
            .authorize_child_scope(&inherited.scope)
            .await
            .is_err());
        assert!(run.tasks.values().all(|t| t
            .binding
            .job_spec
            .as_ref()
            .is_none_or(|job| job.identity_id != child.identity.id)));
        assert_eq!(requests.lock().unwrap().len(), 1);
        // The parent's unresolved in-flight inference retains its own 60-token
        // reservation; the child's 40-token allocation is still separate.
        assert_eq!(
            resources
                .work_budget(child_secret, "org".into(), "team".into(), "root".into())
                .await
                .unwrap()
                .available_tokens,
            0
        );
    };
    tokio::task::LocalSet::new().run_until(work).await;
    server.abort();
}

fn child_execution_binding_task(
    snapshot: &tetonic_domain::RunSnapshot,
    attempt: &tetonic_domain::AttemptId,
) -> tetonic_domain::TaskId {
    snapshot.attempts[attempt].task_id.clone()
}

/// Controllable HTTP responses exercise real provider/broker/manager plumbing.
/// Every socket is owned by the server's JoinSet and is aborted on test cleanup.
async fn delegation_inference_server() -> (
    String,
    String,
    Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
    Arc<tokio::sync::Notify>,
    Arc<tokio::sync::Notify>,
    tokio::task::JoinHandle<()>,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let child_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let child_url = format!("http://{}", child_listener.local_addr().unwrap());
    let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
    let parent = Arc::new(tokio::sync::Notify::new());
    let child = Arc::new(tokio::sync::Notify::new());
    let (captured, parent_gate, child_gate) = (requests.clone(), parent.clone(), child.clone());
    let server = tokio::spawn(async move {
        let mut connections = tokio::task::JoinSet::new();
        loop {
            let (mut stream,_)=tokio::select! {socket=listener.accept()=>socket, socket=child_listener.accept()=>socket}.unwrap();
            let (captured, parent_gate, child_gate) =
                (captured.clone(), parent_gate.clone(), child_gate.clone());
            connections.spawn(async move {
                let mut bytes=Vec::new();
                let (end,length)=loop {
                    let mut chunk=[0;4096];let n=stream.read(&mut chunk).await.unwrap();if n==0 {return;}bytes.extend_from_slice(&chunk[..n]);
                    if let Some(end)=bytes.windows(4).position(|v|v==b"\r\n\r\n") {
                        let headers=String::from_utf8_lossy(&bytes[..end]);
                        let length=headers.lines().find_map(|line|{let (key,value)=line.split_once(':')?;key.eq_ignore_ascii_case("content-length").then(||value.trim().parse::<usize>().unwrap())}).unwrap_or(0);
                        break(end+4,length);
                    }
                };
                while bytes.len()<end+length {let mut chunk=[0;4096];let n=stream.read(&mut chunk).await.unwrap();if n==0{return;}bytes.extend_from_slice(&chunk[..n]);}
                let header=String::from_utf8_lossy(&bytes[..end]);
                let response=if header.starts_with("POST /api/chat ") {
                    let request=serde_json::from_slice(&bytes[end..end+length]).unwrap();
                    let index={let mut requests=captured.lock().unwrap();requests.push(request);requests.len()};
                    if index==1 {parent_gate.notified().await;} else {child_gate.notified().await;}
                    serde_json::json!({"model":"qwen3.5:latest","done":true,"prompt_eval_count":10,"eval_count":20,"message":{"role":"assistant","content":"","tool_calls":[{"function":{"name":"finish","arguments":{"summary":"Completed the assigned analysis."}}}]}})
                }else if header.starts_with("POST /api/generate ") {serde_json::json!({"model":"qwen3.5:latest","done":true,"response":""})}
                else {serde_json::json!({"models":[{"name":"qwen3.5:latest","model":"qwen3.5:latest","size":1,"size_vram":1}],"capabilities":["completion","tools"]})};
                let body=format!("{response}\n");
                let reply=format!("HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                let _=stream.write_all(reply.as_bytes()).await;
            });
            while connections.try_join_next().is_some() {}
        }
    });
    (url, child_url, requests, parent, child, server)
}

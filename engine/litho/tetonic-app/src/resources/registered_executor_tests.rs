use super::*;
use crate::{events::RecordingEventSink, Application, ComputePlaneRequest};
use tetonic_domain::{CandidateOutcome, ExecutionScope, RunState};

#[tokio::test]
async fn registered_workspace_job_uses_production_runtime_broker_tools_and_scoped_audit() {
    for scenario in [
        "success",
        "write",
        "audit-failure",
        "egress-denied",
        "deadline",
    ] {
        let dir = tempfile::tempdir().unwrap();
        // Keep host control data outside the granted workspace.
        let workspace = dir.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::write(workspace.join("fixture.txt"), "cobalt orchard").unwrap();
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
        let resources = local.resources();
        let tool = if scenario == "write" {
            "write_file"
        } else {
            "read_file"
        };
        let registered = resources.register_agent(credential.expose_secret(), "org".into(), "agent".into(), "general".into(),
            serde_json::json!({"instructions":"Perform the requested workspace task", "requested_tools":[tool]})).await.unwrap();
        let limits = || HarnessPreparationLimits {
            max_steps: 3,
            max_input_bytes: 1024,
        };
        let input = if scenario == "write" {
            "Create result.txt with the requested content"
        } else {
            "Read fixture.txt"
        };
        let prepared = resources
            .prepare_general_revision(
                credential.expose_secret(),
                "org".into(),
                "agent".into(),
                registered.identity.bound_definition_digest.clone(),
                input.into(),
                limits(),
            )
            .await
            .unwrap();
        for context in ["private", "other"] {
            local
                .contexts()
                .create(
                    credential.expose_secret(),
                    context.into(),
                    ContextOwner::Private {
                        org_id: "org".into(),
                    },
                )
                .await
                .unwrap();
        }
        let command = prepared.start_command("job".into()).unwrap();
        resources
            .issue_execution_grant(
                credential.expose_secret(),
                tetonic_memory::ExecutionGrant {
                    grant_id: "grant".into(),
                    scope: ExecutionScope {
                        principal_id: "admin".into(),
                        organization_id: "org".into(),
                        information_context_id: "private".into(),
                    },
                    job: command.job_spec,
                    expires_at: chrono::Utc::now().timestamp() + 3600,
                },
            )
            .await
            .unwrap();
        let store = tetonic_memory::SharedStore::open(database.clone(), 1).unwrap();
        let (sink, events) = RecordingEventSink::new();
        let app =
            Application::bootstrap_mock_with_store(dir.path(), Some(store.clone()), sink, vec![]);
        let request = || RegisteredAgentJob {
            request_id: "request-1".into(),
            organization_id: "org".into(),
            information_context_id: "private".into(),
            agent_key: "agent".into(),
            definition_digest: registered.identity.bound_definition_digest.clone(),
            execution_grant_id: "grant".into(),
            input: input.into(),
            recovery_id: "job".into(),
        };
        let settings = || RegisteredExecutionSettings {
            max_elapsed_seconds: if scenario == "deadline" { 5 } else { 30 },
            reported_token_ceiling: None,
            workspace_root: Some(workspace.clone()),
            model: "qwen3.5:latest".into(),
            num_ctx: 8192,
            data_class: tetonic_domain::DataClass::RepositorySource,
            allowed_tools: [tool.into()].into_iter().collect(),
            limits: limits(),
        };
        // A raw provider binding cannot stand in for the installed compute plane.
        assert!(matches!(
            app.submit_registered_job(
                credential.expose_secret(),
                local.credentials().clone(),
                request(),
                settings()
            )
            .await,
            Err(AppError::InferenceUnavailable)
        ));
        let arguments = if scenario == "write" {
            serde_json::json!({"path":"result.txt", "content":"registered workspace write"})
        } else {
            serde_json::json!({"path":"fixture.txt"})
        };
        let (url, requests, server) = crate::tui_mvp_tests::inference_server_with_behavior(
            true,
            tool,
            arguments,
            scenario == "deadline",
        )
        .await;
        let guard = Arc::new(tetonic_egress::EgressGuard::new());
        if scenario != "egress-denied" {
            guard.configure_loopback_inference(url.rsplit(':').next().unwrap().parse().unwrap());
        }
        let plane = crate::build_compute_plane(ComputePlaneRequest {
            guard,
            ollama_base: url,
            policy: app.turn.runtime.policy().clone(),
            workspace_root: workspace.clone(),
            artifact_store: app.turn.runtime.artifact_store().clone(),
            store: Some(store.clone()),
            coordinator: None,
            placement_sink: None,
            previous_pooled: None,
        })
        .await;
        app.install_compute_services(&plane);
        let mut invalid_settings = settings();
        invalid_settings.max_elapsed_seconds = 0;
        assert!(matches!(
            app.submit_registered_job(
                credential.expose_secret(),
                local.credentials().clone(),
                request(),
                invalid_settings,
            )
            .await,
            Err(AppError::InvalidRequest(_))
        ));
        let mut denied_settings = settings();
        denied_settings.allowed_tools.clear();
        assert!(matches!(
            app.submit_registered_job(
                credential.expose_secret(),
                local.credentials().clone(),
                request(),
                denied_settings
            )
            .await,
            Err(AppError::PolicyDenied(_))
        ));
        assert!(store
            .read(|db| db.list_all_run_ids())
            .await
            .unwrap()
            .unwrap()
            .is_empty());
        if scenario == "audit-failure" {
            rusqlite::Connection::open(&database).unwrap().execute_batch("CREATE TRIGGER fail_registered_audit BEFORE INSERT ON tool_calls BEGIN SELECT RAISE(ABORT,'injected audit write failure'); END;").unwrap();
        }
        let (submission, result) = tokio::task::LocalSet::new()
            .run_until(async {
                let (a, b) = tokio::join!(
                    app.submit_registered_job(
                        credential.expose_secret(),
                        local.credentials().clone(),
                        request(),
                        settings()
                    ),
                    app.submit_registered_job(
                        credential.expose_secret(),
                        local.credentials().clone(),
                        request(),
                        settings()
                    ),
                );
                let (a, b) = (a.unwrap(), b.unwrap());
                assert_eq!(a.run_id, b.run_id);
                assert_eq!(a.audit_session_id, b.audit_session_id);
                assert_ne!(
                    a.execution.is_some(),
                    b.execution.is_some(),
                    "exactly one request may own execution"
                );
                let submission = if a.execution.is_some() { a } else { b };
                let super::RegisteredAgentSubmission {
                    audit_session_id,
                    execution,
                    ..
                } = submission;
                let RegisteredAgentExecution {
                    attempt_id,
                    completion,
                } = execution.unwrap();
                if scenario == "deadline" {
                    let mut competing = request();
                    competing.request_id = "distinct-work-while-busy".into();
                    assert!(matches!(app.submit_registered_job(
                        credential.expose_secret(), local.credentials().clone(), competing, settings()
                    ).await, Err(AppError::ExecutionCapacityExceeded)),
                        "the product entry must reject a distinct concurrent job for the same identity");
                }
                let result = tokio::time::timeout(std::time::Duration::from_secs(15), completion)
                    .await
                    .unwrap()
                    .unwrap();
                ((attempt_id, audit_session_id), result)
            })
            .await;
        server.abort();
        assert_eq!(result.attempt_id, submission.0);
        // A terminal retry returns the original receipt without installing a new
        // inference owner, extending time, or replacing its private audit.
        let replay = app
            .submit_registered_job(
                credential.expose_secret(),
                local.credentials().clone(),
                request(),
                settings(),
            )
            .await
            .unwrap();
        assert!(replay.execution.is_none());
        assert_eq!(replay.run_id, result.run_id);
        assert_eq!(replay.audit_session_id, submission.1);
        let mut changed_settings = settings();
        changed_settings.max_elapsed_seconds += 1;
        assert!(matches!(
            app.submit_registered_job(
                credential.expose_secret(),
                local.credentials().clone(),
                request(),
                changed_settings
            )
            .await,
            Err(AppError::InvalidRequest(_))
        ));
        assert_eq!(
            store
                .read(|db| db.list_all_run_ids())
                .await
                .unwrap()
                .unwrap()
                .iter()
                .filter(|id| id.starts_with("run_activation_"))
                .count(),
            1
        );
        let snapshot = local
            .contexts()
            .inspect_run(
                credential.expose_secret(),
                "org".into(),
                "private".into(),
                result.run_id.0.clone(),
            )
            .await
            .unwrap();
        assert!(snapshot.tasks[&result.task_id].binding.deadline.is_some());
        assert!(
            snapshot.attempts[&result.attempt_id].execution_quiesced,
            "completion must acknowledge actual worker drain for every terminal scenario"
        );
        assert_eq!(
            snapshot.deadlines.run_deadline,
            snapshot.tasks[&result.task_id].binding.deadline
        );
        let transcript = local
            .contexts()
            .transcript(
                credential.expose_secret(),
                "private".into(),
                submission.1.clone(),
                200,
            )
            .await
            .unwrap();
        assert!(local
            .contexts()
            .transcript(
                credential.expose_secret(),
                "other".into(),
                submission.1.clone(),
                200
            )
            .await
            .is_err());
        let history = submission.1.clone();
        assert!(store
            .read(move |db| db.require_legacy_session(&history))
            .await
            .unwrap()
            .is_err());
        assert!(
            events.lock().unwrap().is_empty(),
            "scoped execution leaked into legacy product events"
        );
        let requests = requests.lock().unwrap();
        match scenario {
            "success" => {
                assert_eq!(snapshot.state, RunState::Succeeded);
                assert!(
                    matches!(result.outcome, CandidateOutcome::Completed { .. }),
                    "{:?}",
                    result.outcome
                );
                assert_eq!(requests.len(), 2);
                assert!(requests[1]["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|m| m["role"] == "tool"
                        && m["content"]
                            .as_str()
                            .unwrap_or_default()
                            .contains("cobalt orchard")));
                assert!(transcript
                    .iter()
                    .any(|(_, role, text)| role == "tool" && text.contains("cobalt orchard")));
                let raw = rusqlite::Connection::open(&database).unwrap();
                let reservations: i64 = raw
                    .query_row("SELECT COUNT(*) FROM compute_reservations", [], |r| {
                        r.get(0)
                    })
                    .unwrap();
                assert_eq!(
                    reservations, 2,
                    "each inference must use the installed broker"
                );
                let linkage: i64 = raw.query_row("SELECT COUNT(*) FROM messages m JOIN tool_calls t ON m.tool_call_id=t.id AND m.session_id=t.session_id WHERE m.session_id=?1 AND m.role='tool'", [&submission.1], |r| r.get(0)).unwrap();
                assert_eq!(linkage, 1);
            }
            "write" => {
                assert_eq!(snapshot.state, RunState::Succeeded, "{:?}", result.outcome);
                assert_eq!(requests.len(), 2);
                assert_eq!(
                    std::fs::read_to_string(workspace.join("result.txt")).unwrap(),
                    "registered workspace write"
                );
                assert!(snapshot.tasks[&result.task_id].accepted_artifact.is_some());
            }
            "audit-failure" => {
                assert_eq!(snapshot.state, RunState::Failed);
                assert_eq!(
                    requests.len(),
                    1,
                    "audit failure must block the next inference"
                );
                assert!(snapshot.tasks[&result.task_id].accepted_artifact.is_none());
            }
            "egress-denied" => {
                assert_eq!(snapshot.state, RunState::Failed);
                assert!(requests.is_empty(), "denied endpoint received agent input");
            }
            "deadline" => {
                assert_eq!(
                    requests.len(),
                    1,
                    "hung inference must not be retried after expiry"
                );
                assert_eq!(snapshot.state, RunState::Failed);
                assert_eq!(
                    snapshot.attempts[&result.attempt_id].state,
                    tetonic_domain::AttemptState::TimedOut
                );
                assert_eq!(
                    snapshot.attempts[&result.attempt_id].failure_class,
                    Some(tetonic_domain::FailureClass::TimedOut)
                );
                assert!(
                    matches!(result.outcome, CandidateOutcome::Failed { ref message } if message == "execution deadline exceeded")
                );
                assert!(snapshot.tasks[&result.task_id].accepted_artifact.is_none());
                assert!(app
                    .run_manager
                    .managed()
                    .binding(&result.attempt_id)
                    .is_none());
            }
            _ => unreachable!(),
        }
        drop(requests);
        resources
            .revoke_execution_grant(credential.expose_secret(), "org".into(), "grant".into())
            .await
            .unwrap();
        assert!(
            matches!(
                app.submit_registered_job(
                    credential.expose_secret(),
                    local.credentials().clone(),
                    request(),
                    settings()
                )
                .await,
                Err(AppError::PolicyDenied(_))
            ),
            "revoked grants must not disclose a retry receipt"
        );
    }
}

#[tokio::test]
async fn team_execution_cannot_retrieve_unpublished_private_history() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
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
    let resources = local.resources();
    resources
        .create_team(secret, "org".into(), "team".into(), "Team".into())
        .await
        .unwrap();
    let contexts = local.contexts();
    contexts
        .create(
            secret,
            "private".into(),
            ContextOwner::Private {
                org_id: "org".into(),
            },
        )
        .await
        .unwrap();
    contexts
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
    contexts
        .provision_discussion(secret, "private".into(), "private-notes".into())
        .await
        .unwrap();
    contexts
        .provision_discussion(secret, "shared".into(), "shared-notes".into())
        .await
        .unwrap();
    let seq = contexts
        .append_message(
            secret,
            "private".into(),
            "private-notes".into(),
            "secret".into(),
            "searchword PRIVATECANARY".into(),
        )
        .await
        .unwrap();
    let registered = resources.register_agent(secret,"org".into(),"agent".into(),"general".into(),
        serde_json::json!({"instructions":"Look up prior notes","requested_tools":["recall"],"max_steps":4})).await.unwrap();
    let limits = || HarnessPreparationLimits {
        max_steps: 4,
        max_input_bytes: 1024,
    };
    let prepared = resources
        .prepare_general_revision(
            secret,
            "org".into(),
            "agent".into(),
            registered.identity.bound_definition_digest.clone(),
            "Look up prior notes".into(),
            limits(),
        )
        .await
        .unwrap();
    for (grant_id, recovery_id) in [("grant-1", "job-1"), ("grant-2", "job-2")] {
        let command = prepared.start_command(recovery_id.into()).unwrap();
        resources
            .issue_execution_grant(
                secret,
                tetonic_memory::ExecutionGrant {
                    grant_id: grant_id.into(),
                    scope: ExecutionScope {
                        principal_id: "admin".into(),
                        organization_id: "org".into(),
                        information_context_id: "shared".into(),
                    },
                    job: command.job_spec,
                    expires_at: chrono::Utc::now().timestamp() + 3600,
                },
            )
            .await
            .unwrap();
    }
    let store = tetonic_memory::SharedStore::open(database, 1).unwrap();
    let (sink, events) = RecordingEventSink::new();
    let app = Application::bootstrap_mock_with_store(dir.path(), Some(store), sink, vec![]);
    let (url, requests, server) = crate::tui_mvp_tests::inference_server_with_behavior(
        true,
        "recall",
        serde_json::json!({"query":"searchword"}),
        false,
    )
    .await;
    let guard = Arc::new(tetonic_egress::EgressGuard::new());
    guard.configure_loopback_inference(url.rsplit(':').next().unwrap().parse().unwrap());
    let plane = crate::build_compute_plane(ComputePlaneRequest {
        guard,
        ollama_base: url,
        policy: app.turn.runtime.policy().clone(),
        workspace_root: workspace.clone(),
        artifact_store: app.turn.runtime.artifact_store().clone(),
        store: app.run_manager.managed().store().cloned(),
        coordinator: None,
        placement_sink: None,
        previous_pooled: None,
    })
    .await;
    app.install_compute_services(&plane);
    let submit = |request_id: &str, grant_id: &str, recovery_id: &str| {
        let request = RegisteredAgentJob {
            request_id: request_id.into(),
            organization_id: "org".into(),
            information_context_id: "shared".into(),
            agent_key: "agent".into(),
            definition_digest: registered.identity.bound_definition_digest.clone(),
            execution_grant_id: grant_id.into(),
            input: "Look up prior notes".into(),
            recovery_id: recovery_id.into(),
        };
        app.submit_registered_job(
            credential.expose_secret(),
            local.credentials().clone(),
            request,
            RegisteredExecutionSettings {
                max_elapsed_seconds: 30,
                reported_token_ceiling: None,
                workspace_root: Some(workspace.clone()),
                model: "qwen3.5:latest".into(),
                num_ctx: 8192,
                data_class: tetonic_domain::DataClass::RepositorySource,
                allowed_tools: ["recall".into()].into_iter().collect(),
                limits: limits(),
            },
        )
    };
    let first = tokio::task::LocalSet::new()
        .run_until(async {
            let submission = submit("request-1", "grant-1", "job-1").await.unwrap();
            let execution = submission.execution.expect("first launch owns execution");
            let result =
                tokio::time::timeout(std::time::Duration::from_secs(20), execution.completion)
                    .await
                    .unwrap()
                    .unwrap();
            (submission.run_id, submission.audit_session_id, result)
        })
        .await;
    let captured = requests.lock().unwrap();
    let before = serde_json::to_string(&captured[..]).unwrap();
    assert!(
        !before.contains("PRIVATECANARY"),
        "unpublished private history entered team inference: {before}"
    );
    drop(captured);
    assert!(!events
        .lock()
        .unwrap()
        .iter()
        .any(|event| format!("{event:?}").contains("PRIVATECANARY")));
    let transcript = contexts
        .transcript(secret, "shared".into(), first.1.clone(), 200)
        .await
        .unwrap();
    assert!(transcript
        .iter()
        .all(|(_, _, text)| !text.contains("PRIVATECANARY")));
    assert!(contexts
        .inspect_run(secret, "org".into(), "private".into(), first.0 .0.clone())
        .await
        .is_err());
    contexts
        .publish_message(
            secret,
            "private".into(),
            "private-notes".into(),
            seq,
            "shared".into(),
            "shared-notes".into(),
            "share-1".into(),
        )
        .await
        .unwrap();
    server.abort();
    let (url, requests, server) = crate::tui_mvp_tests::inference_server_with_behavior(
        true,
        "recall",
        serde_json::json!({"query":"searchword"}),
        false,
    )
    .await;
    let guard = Arc::new(tetonic_egress::EgressGuard::new());
    guard.configure_loopback_inference(url.rsplit(':').next().unwrap().parse().unwrap());
    let plane = crate::build_compute_plane(ComputePlaneRequest {
        guard,
        ollama_base: url,
        policy: app.turn.runtime.policy().clone(),
        workspace_root: workspace.clone(),
        artifact_store: app.turn.runtime.artifact_store().clone(),
        store: app.run_manager.managed().store().cloned(),
        coordinator: None,
        placement_sink: None,
        previous_pooled: None,
    })
    .await;
    app.install_compute_services(&plane);
    let second = tokio::task::LocalSet::new()
        .run_until(async {
            let submission = submit("request-2", "grant-2", "job-2").await.unwrap();
            let execution = submission
                .execution
                .expect("published launch owns execution");
            tokio::time::timeout(std::time::Duration::from_secs(20), execution.completion)
                .await
                .unwrap()
                .unwrap();
            submission.audit_session_id
        })
        .await;
    server.abort();
    let captured = requests.lock().unwrap();
    let after = serde_json::to_string(&captured[..]).unwrap();
    assert!(
        after.contains("PRIVATECANARY"),
        "authorized publication did not become visible to team recall: {after}"
    );
    let published = contexts
        .transcript(secret, "shared".into(), second, 200)
        .await
        .unwrap();
    assert!(published
        .iter()
        .any(|(_, _, text)| text.contains("PRIVATECANARY")));
    assert_eq!(
        contexts
            .transcript(secret, "private".into(), "private-notes".into(), 20)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn noncoding_recall_job_runs_without_a_repository() {
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
    let resources = local.resources();
    let registered = resources.register_agent(secret,"org".into(),"agent".into(),"general".into(),
        serde_json::json!({"instructions":"Use recall","requested_tools":["recall"],"max_steps":3})).await.unwrap();
    let limits = || HarnessPreparationLimits {
        max_steps: 3,
        max_input_bytes: 1024,
    };
    let prepared = resources
        .prepare_general_revision(
            secret,
            "org".into(),
            "agent".into(),
            registered.identity.bound_definition_digest.clone(),
            "Look up the note".into(),
            limits(),
        )
        .await
        .unwrap();
    local
        .contexts()
        .create(
            secret,
            "private".into(),
            ContextOwner::Private {
                org_id: "org".into(),
            },
        )
        .await
        .unwrap();
    local
        .contexts()
        .provision_discussion(secret, "private".into(), "notes".into())
        .await
        .unwrap();
    local
        .contexts()
        .append_message(
            secret,
            "private".into(),
            "notes".into(),
            "note".into(),
            "EXTERNALCANARY from outside any repository".into(),
        )
        .await
        .unwrap();
    let command = prepared.start_command("job".into()).unwrap();
    resources
        .issue_execution_grant(
            secret,
            tetonic_memory::ExecutionGrant {
                grant_id: "grant".into(),
                scope: ExecutionScope {
                    principal_id: "admin".into(),
                    organization_id: "org".into(),
                    information_context_id: "private".into(),
                },
                job: command.job_spec,
                expires_at: chrono::Utc::now().timestamp() + 3600,
            },
        )
        .await
        .unwrap();
    let store = tetonic_memory::SharedStore::open(database, 1).unwrap();
    let (sink, _) = RecordingEventSink::new();
    let app = Application::bootstrap_mock_with_store(dir.path(), Some(store.clone()), sink, vec![]);
    let request = || RegisteredAgentJob {
        request_id: "request-1".into(),
        organization_id: "org".into(),
        information_context_id: "private".into(),
        agent_key: "agent".into(),
        definition_digest: registered.identity.bound_definition_digest.clone(),
        execution_grant_id: "grant".into(),
        input: "Look up the note".into(),
        recovery_id: "job".into(),
    };
    let settings = |tools: &[&str]| RegisteredExecutionSettings {
        max_elapsed_seconds: 30,
        reported_token_ceiling: None,
        workspace_root: None,
        model: "qwen3.5:latest".into(),
        num_ctx: 8192,
        data_class: tetonic_domain::DataClass::RepositorySource,
        allowed_tools: tools.iter().map(|tool| (*tool).to_string()).collect(),
        limits: limits(),
    };
    assert!(matches!(
        app.submit_registered_job(
            secret,
            local.credentials().clone(),
            request(),
            settings(&["recall", "read_file"])
        )
        .await,
        Err(AppError::WorkspaceUnavailable)
    ));
    let (url, requests, server) = crate::tui_mvp_tests::inference_server_with_behavior(
        true,
        "recall",
        serde_json::json!({"query":"EXTERNALCANARY"}),
        false,
    )
    .await;
    let guard = Arc::new(tetonic_egress::EgressGuard::new());
    guard.configure_loopback_inference(url.rsplit(':').next().unwrap().parse().unwrap());
    let plane = crate::build_compute_plane(ComputePlaneRequest {
        guard,
        ollama_base: url,
        policy: app.turn.runtime.policy().clone(),
        workspace_root: dir.path().to_path_buf(),
        artifact_store: app.turn.runtime.artifact_store().clone(),
        store: Some(store),
        coordinator: None,
        placement_sink: None,
        previous_pooled: None,
    })
    .await;
    app.install_compute_services(&plane);
    let receipt = tokio::task::LocalSet::new()
        .run_until(async {
            let submission = app
                .submit_registered_job(
                    secret,
                    local.credentials().clone(),
                    request(),
                    settings(&["recall"]),
                )
                .await
                .unwrap();
            let execution = submission.execution.expect("launch owns execution");
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(20),
                execution.completion,
            )
            .await
            .unwrap()
            .unwrap();
            (submission.audit_session_id, result.outcome)
        })
        .await;
    server.abort();
    assert!(matches!(receipt.1, CandidateOutcome::Completed { .. }));
    let body = serde_json::to_string(&requests.lock().unwrap().clone()).unwrap();
    assert!(body.contains("EXTERNALCANARY"));
    let transcript = local
        .contexts()
        .transcript(secret, "private".into(), receipt.0, 50)
        .await
        .unwrap();
    assert!(transcript
        .iter()
        .any(|(_, _, text)| text.contains("EXTERNALCANARY")));
}

#[tokio::test]
async fn registered_shell_is_rejected_before_inference() {
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
    let resources = local.resources();
    let registered = resources
        .register_agent(
            secret,
            "org".into(),
            "agent".into(),
            "general".into(),
            serde_json::json!({"instructions":"Run a command","requested_tools":["run_shell"],"max_steps":2}),
        )
        .await
        .unwrap();
    let limits = HarnessPreparationLimits {
        max_steps: 2,
        max_input_bytes: 1024,
    };
    let prepared = resources
        .prepare_general_revision(
            secret,
            "org".into(),
            "agent".into(),
            registered.identity.bound_definition_digest.clone(),
            "Say hello".into(),
            limits,
        )
        .await
        .unwrap();
    local
        .contexts()
        .create(
            secret,
            "private".into(),
            ContextOwner::Private {
                org_id: "org".into(),
            },
        )
        .await
        .unwrap();
    let command = prepared.start_command("job".into()).unwrap();
    resources
        .issue_execution_grant(
            secret,
            tetonic_memory::ExecutionGrant {
                grant_id: "grant".into(),
                scope: ExecutionScope {
                    principal_id: "admin".into(),
                    organization_id: "org".into(),
                    information_context_id: "private".into(),
                },
                job: command.job_spec,
                expires_at: chrono::Utc::now().timestamp() + 3600,
            },
        )
        .await
        .unwrap();
    let store = tetonic_memory::SharedStore::open(database.clone(), 1).unwrap();
    let (sink, _) = RecordingEventSink::new();
    let app = Application::bootstrap_mock_with_store(dir.path(), Some(store.clone()), sink, vec![]);
    let submitted = app
        .submit_registered_job(
            secret,
            local.credentials().clone(),
            RegisteredAgentJob {
                request_id: "request-1".into(),
                organization_id: "org".into(),
                information_context_id: "private".into(),
                agent_key: "agent".into(),
                definition_digest: registered.identity.bound_definition_digest,
                execution_grant_id: "grant".into(),
                input: "Say hello".into(),
                recovery_id: "job".into(),
            },
            RegisteredExecutionSettings {
                max_elapsed_seconds: 30,
                reported_token_ceiling: None,
                workspace_root: None,
                model: "qwen3.5:latest".into(),
                num_ctx: 8192,
                data_class: tetonic_domain::DataClass::RepositorySource,
                allowed_tools: ["run_shell".into()].into_iter().collect(),
                limits: HarnessPreparationLimits {
                    max_steps: 2,
                    max_input_bytes: 1024,
                },
            },
        )
        .await;
    let Err(error) = submitted else {
        panic!("shell profile was admitted");
    };
    match error {
        AppError::PolicyDenied(message) => assert!(message.contains("run_shell"), "{message}"),
        other => panic!("expected isolation denial, got {other}"),
    }
    let count: i64 = rusqlite::Connection::open(&database)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM sessions WHERE mode='execution-audit'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "unsupported shell must not create an execution audit");
}

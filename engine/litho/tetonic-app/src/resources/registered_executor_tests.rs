use super::*;
use crate::{events::RecordingEventSink, Application, ComputePlaneRequest};
use tetonic_domain::{CandidateOutcome, ExecutionScope, RunState};

#[tokio::test]
async fn registered_workspace_job_uses_production_runtime_broker_tools_and_scoped_audit() {
    for scenario in ["success", "write", "audit-failure", "egress-denied"] {
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
            organization_id: "org".into(),
            information_context_id: "private".into(),
            agent_key: "agent".into(),
            definition_digest: registered.identity.bound_definition_digest.clone(),
            execution_grant_id: "grant".into(),
            input: input.into(),
            recovery_id: "job".into(),
        };
        let settings = || RegisteredExecutionSettings {
            workspace_root: workspace.clone(),
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
        let (url, requests, server) =
            crate::tui_mvp_tests::inference_server_with_tool(true, tool, arguments).await;
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
                let submission = app
                    .submit_registered_job(
                        credential.expose_secret(),
                        local.credentials().clone(),
                        request(),
                        settings(),
                    )
                    .await
                    .unwrap();
                let super::RegisteredAgentSubmission {
                    attempt_id,
                    audit_session_id,
                    completion,
                } = submission;
                let result = tokio::time::timeout(std::time::Duration::from_secs(15), completion)
                    .await
                    .unwrap()
                    .unwrap();
                ((attempt_id, audit_session_id), result)
            })
            .await;
        server.abort();
        assert_eq!(result.attempt_id, submission.0);
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
            _ => unreachable!(),
        }
    }
}

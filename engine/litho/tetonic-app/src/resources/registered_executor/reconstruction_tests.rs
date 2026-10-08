//! Composition contract tests. The answer hook is controlled by the fixture;
//! production question persistence/controller cutover is intentionally separate.
use super::*;
use std::{sync::atomic::AtomicBool, time::Duration};
use tetonic_domain::{AttemptState, CandidateOutcome};
use tetonic_run::managed::{ActivationReceipt, ManagedSubmission};

#[path = "reconstruction_fixture_tests.rs"]
mod fixture;
#[path = "reconstruction_server_tests.rs"]
mod server;
use fixture::Fixture;

fn answer_hook(answer: Arc<AtomicBool>) -> tetonic_core::SpawnHook {
    Box::new(move |request, _| {
        let answer = answer.clone();
        Box::pin(async move {
            assert_eq!(request.tool_name, "ask_human");
            while !answer.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            tetonic_domain::ToolOutcome::ok("answered", "Beginners")
        })
    })
}

async fn park(
    f: &Fixture,
    app: &crate::Application,
) -> (ActivationReceipt, tetonic_domain::AttemptId) {
    let assembly::RegisteredHarness {
        prepared,
        agent,
        history,
        ..
    } = f.harness(app, None).await;
    let agent = agent
        .with_spawn(answer_hook(Arc::new(AtomicBool::new(false))))
        .with_durable_waits();
    let ManagedSubmission::Started { binding, .. } = app
        .run_manager
        .submit_prepared_registered_job(prepared, agent)
        .await
        .unwrap()
    else {
        panic!("fresh execution")
    };
    f.local
        .resources()
        .activate_team_work_item(
            f.credential.expose_secret(),
            "org".into(),
            "team".into(),
            "work".into(),
            binding.attempt_id.0.clone(),
            binding.run_id.0.clone(),
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let run = app
                .run_manager
                .managed()
                .inspect_run(&binding.run_id)
                .await
                .unwrap();
            if run.attempts[&binding.attempt_id].state == AttemptState::Suspended {
                break;
            }
            assert_eq!(run.state, tetonic_domain::RunState::Active, "{run:?}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    (
        ActivationReceipt {
            run_id: binding.run_id,
            task_id: binding.task_id,
            audit_session_id: history,
        },
        binding.attempt_id,
    )
}

#[tokio::test]
async fn registered_harness_rebuild_retains_audit_tools_usage_and_exact_continuation() {
    let f = Fixture::new().await;
    let (url, requests, server) = server::inference().await;
    let app = f.app(&url).await;
    let (receipt, attempt) = tokio::task::LocalSet::new().run_until(park(&f, &app)).await;
    drop(app); // Lose the executor, rebuild from the same on-disk control/artifact stores.
    assert_eq!(requests.lock().unwrap().len(), 2);
    let reopened = f.app(&url).await;
    let before = f
        .store()
        .read(|db| db.team_work_usage("admin", "org", "team"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(before[0].input_tokens + before[0].output_tokens, 60);
    assert_eq!(before[0].held_tokens, 35);
    let rebuilt = f.harness(&reopened, Some(receipt.clone())).await;
    assert_eq!(rebuilt.history, receipt.audit_session_id);
    assert!(
        rebuilt.prepared.deadline.is_none(),
        "restoration cannot reset the execution allowance"
    );
    assert_eq!(
        rebuilt
            .agent
            .advertised_tool_names()
            .into_iter()
            .collect::<std::collections::HashSet<_>>(),
        ["read_file", "finish", "ask_human"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
    let result = tokio::task::LocalSet::new()
        .run_until(async {
            let restored = reopened
                .run_manager
                .execute_prepared_registered_job(
                    rebuilt.prepared,
                    rebuilt
                        .agent
                        .with_spawn(answer_hook(Arc::new(AtomicBool::new(true)))),
                    rebuilt.restore,
                )
                .await
                .unwrap();
            let ManagedSubmission::Started {
                binding,
                completion,
            } = restored
            else {
                panic!("restored executor")
            };
            assert_eq!(binding.attempt_id, attempt);
            tokio::time::timeout(Duration::from_secs(15), completion)
                .await
                .unwrap()
                .unwrap()
        })
        .await;
    assert!(
        matches!(result.outcome, CandidateOutcome::Completed { .. }),
        "{:?}",
        result.outcome
    );
    {
        let calls = requests.lock().unwrap();
        assert_eq!(calls.len(), 3, "do not replay model turns or read_file");
        let messages = calls[2]["messages"].as_array().unwrap();
        assert_eq!(messages.iter().filter(|m| m["role"] == "user").count(), 1);
        assert!(messages.iter().any(|m| m["role"] == "tool"
            && m["content"]
                .as_str()
                .is_some_and(|s| s.contains("cobalt orchard"))));
        assert!(messages.iter().any(|m| m["role"] == "tool"
            && m["content"]
                .as_str()
                .is_some_and(|s| s.contains("Beginners"))));
    }
    let after = f
        .store()
        .read(|db| db.team_work_usage("admin", "org", "team"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        after[0].input_tokens + after[0].output_tokens,
        90,
        "reuse the original usage ledger after lease transfer"
    );
    assert_eq!(after[0].held_tokens, 5);
    f.store()
        .write(move |db| db.settle_work_inference(&attempt.0))
        .await
        .unwrap()
        .unwrap();
    let settled = f
        .store()
        .read(|db| db.team_work_usage("admin", "org", "team"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        settled[0].released_tokens, 5,
        "only the unused original allowance is released"
    );
    assert_eq!(settled[0].held_tokens, 0);
    let history = f
        .local
        .contexts()
        .transcript(
            f.credential.expose_secret(),
            "shared".into(),
            receipt.audit_session_id,
            200,
        )
        .await
        .unwrap();
    assert!(serde_json::to_string(&history)
        .unwrap()
        .contains("cobalt orchard"));
    assert!(serde_json::to_string(&history)
        .unwrap()
        .contains("Beginners"));
    server.abort();
}

#[tokio::test]
async fn reconstruction_rejects_changed_approval_and_missing_audit_without_new_execution() {
    let f = Fixture::new().await;
    let (url, requests, server) = server::inference().await;
    let app = f.app(&url).await;
    let (receipt, _) = tokio::task::LocalSet::new().run_until(park(&f, &app)).await;
    drop(app);
    let app = f.app(&url).await;
    for change in [
        "model",
        "tools",
        "workspace",
        "tokens",
        "time",
        "receipt",
        "request",
        "input",
        "revision",
    ] {
        let mut settings = f.settings();
        let mut request = f.request();
        let mut expected = receipt.clone();
        match change {
            "model" => settings.model = "different-model".into(),
            "tools" => {
                settings.allowed_tools.insert("write_file".into());
            }
            "workspace" => settings.workspace_root = Some(f.dir.path().to_path_buf()),
            "tokens" => settings.reported_token_ceiling = Some(100),
            "time" => settings.max_elapsed_seconds += 1,
            "receipt" => expected.audit_session_id = "another-audit".into(),
            "request" => request.request_id = "another-request".into(),
            "input" => request.input = "a different job".into(),
            "revision" => request.definition_digest = "sha256:missing".into(),
            _ => unreachable!(),
        }
        assert!(
            app.prepare_registered_harness(
                f.credential.expose_secret(),
                f.local.credentials().clone(),
                request,
                settings,
                Fixture::context(Some(expected))
            )
            .await
            .is_err(),
            "{change}"
        );
    }
    // A normal retry is only a receipt, even while a durable wait exists.
    assert!(matches!(
        app.prepare_registered_harness(
            f.credential.expose_secret(),
            f.local.credentials().clone(),
            f.request(),
            f.settings(),
            Fixture::context(None)
        )
        .await
        .unwrap(),
        preparation::HarnessPreparation::Existing(_)
    ));
    let plan = app
        .prepare_registered_harness(
            f.credential.expose_secret(),
            f.local.credentials().clone(),
            f.request(),
            f.settings(),
            Fixture::context(Some(receipt.clone())),
        )
        .await
        .unwrap();
    let preparation::HarnessPreparation::Ready(plan) = plan else {
        panic!("rebuild")
    };
    // Fault injection: audit loss must not silently create a replacement history.
    rusqlite::Connection::open(f.dir.path().join("control.db"))
        .unwrap()
        .execute(
            "UPDATE sessions SET mode='invalid' WHERE id=?1",
            [&receipt.audit_session_id],
        )
        .unwrap();
    assert!(app
        .assemble_registered_harness(f.credential.expose_secret(), *plan)
        .await
        .is_err());
    assert_eq!(requests.lock().unwrap().len(), 2);
    f.local
        .credentials()
        .revoke(f.credential.credential_id.clone())
        .await
        .unwrap();
    assert!(app
        .prepare_registered_harness(
            f.credential.expose_secret(),
            f.local.credentials().clone(),
            f.request(),
            f.settings(),
            Fixture::context(Some(receipt))
        )
        .await
        .is_err());
    server.abort();
}

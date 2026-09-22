//! H3-2: sealed turn artifacts on the production complete_turn path.

use crate::*;
use lokai_artifact::{verify_stored_content, LocalArtifactStore};
use lokai_domain::artifact::{ArtifactLocation, ArtifactStore};
use lokai_domain::ids::ArtifactId;
use lokai_domain::{AttemptState, CandidateOutcome, CompletionKind, RunState};
use std::sync::Arc;

struct FakeEventSink;
impl events::ApplicationEventSink for FakeEventSink {
    fn send(&self, _event: events::ApplicationEvent) {}
}

fn make_app_with_store(
    dir: &std::path::Path,
) -> (
    Application,
    Arc<LocalArtifactStore>,
    lokai_memory::SharedStore,
) {
    std::fs::create_dir_all(dir).unwrap();
    let db_path = dir.join(format!(
        "lokai_test_{}.db",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = lokai_memory::SharedStore::open(&db_path, 1).unwrap();
    let policy = Arc::new(lokai_policy::PolicyEngine::default());
    let artifacts = Arc::new(
        LocalArtifactStore::new(
            dir.join("artifacts"),
            crate::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );
    let runtime = lokai_runtime::EngineRuntime::new(policy.clone(), None, artifacts.clone());
    let app = Application::new(ApplicationDependencies {
        runtime: Arc::new(runtime),
        store: Some(store.clone()),
        policy,
        event_sink: Arc::new(FakeEventSink),
        index_db: None,
        fabric_hint: None,
    });
    (app, artifacts, store)
}

async fn start_and_plan(
    app: &Application,
) -> (String, lokai_domain::RunId, lokai_domain::AttemptId) {
    let tmp = std::env::temp_dir();
    let started = app
        .sessions
        .start_session(commands::StartSessionCommand {
            workspace_root: tmp.display().to_string(),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .expect("session");
    let plan = app
        .runs
        .plan_turn(&commands::RunTurnCommand {
            session_id: started.session_id.clone(),
            user_input: "do the work".into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .expect("plan");
    (started.session_id, plan.run_id, plan.attempt_id)
}

async fn accepted_digest(app: &Application, run_id: lokai_domain::RunId) -> (String, String) {
    let snap = app
        .runs
        .inspect_run(commands::InspectRunCommand {
            run_id: run_id.to_string(),
        })
        .await
        .expect("snapshot");
    let task_id = lokai_domain::TaskId::new(format!("task_root_{run_id}"));
    let art = snap
        .tasks
        .get(&task_id)
        .and_then(|t| t.accepted_artifact.clone())
        .expect("accepted artifact");
    (art.artifact_id, art.digest)
}

#[tokio::test]
async fn finish_turn_seals_matching_artifact() {
    let dir = std::env::temp_dir().join(lokai_memory::new_id("h32"));
    let (app, artifacts, mem) = make_app_with_store(&dir);
    let (session_id, run_id, attempt_id) = start_and_plan(&app).await;
    {
        let _ = mem.write_sync({
            let session_id = session_id.clone();
            move |db| {
                db.record_file_change(
                    "tc1",
                    &session_id,
                    "README.md",
                    "write",
                    None,
                    Some("# ok\n"),
                )
                .unwrap();
                db.append_message(&session_id, "assistant", "", "added readme", None)
                    .unwrap();
            }
        });
    }
    app.runs
        .complete_turn(
            &commands::CompleteTurnCommand {
                session_id: session_id.clone(),
                attempt_id: attempt_id.clone(),
                workspace_root: std::env::temp_dir().display().to_string(),
                canceled: false,
                error: None,
            },
            None,
            None,
        )
        .await
        .expect("complete");
    let (artifact_id, digest) = accepted_digest(&app, run_id).await;
    assert!(artifact_id.starts_with("art_"), "{artifact_id}");
    assert!(digest.starts_with("sha256:"), "{digest}");
    assert!(!digest.contains("turn_ok"), "{digest}");
    let id = ArtifactId::new(artifact_id);
    let verified = verify_stored_content(artifacts.as_ref(), &id)
        .await
        .expect("verify sealed bytes");
    assert_eq!(digest, format!("sha256:{}", verified.0));
}

#[tokio::test]
async fn accept_path_marks_accepted_and_provenance() {
    use lokai_domain::artifact::{ArtifactState, ProvenanceTrustLabel};
    let dir = std::env::temp_dir().join(lokai_memory::new_id("r24"));
    let (app, artifacts, mem) = make_app_with_store(&dir);
    let (session_id, run_id, attempt_id) = start_and_plan(&app).await;
    {
        let _ = mem.write_sync({
            let session_id = session_id.clone();
            move |db| {
                db.append_message(&session_id, "assistant", "", "ship it", None)
                    .unwrap();
            }
        });
    }
    app.runs
        .complete_turn(
            &commands::CompleteTurnCommand {
                session_id: session_id.clone(),
                attempt_id: attempt_id.clone(),
                workspace_root: std::env::temp_dir().display().to_string(),
                canceled: false,
                error: None,
            },
            None,
            None,
        )
        .await
        .expect("complete");
    let (artifact_id, _) = accepted_digest(&app, run_id).await;
    let id = ArtifactId::new(artifact_id.clone());
    let meta = artifacts.metadata(&id).await.expect("meta after accept");
    assert_eq!(meta.lifecycle_state, ArtifactState::Accepted);
    let bundle = artifacts
        .reconstruct_provenance(&id)
        .await
        .expect("provenance");
    assert_eq!(bundle.trust_label, ProvenanceTrustLabel::Accepted);
    assert_eq!(bundle.artifact_id.0, artifact_id);

    let missing = artifacts
        .reconstruct_provenance(&ArtifactId::new("art_does_not_exist"))
        .await
        .unwrap_err();
    assert!(matches!(
        missing,
        lokai_domain::artifact::ArtifactError::NotFound(_)
    ));
}

#[tokio::test]
async fn tampered_artifact_fails_verify() {
    let dir = std::env::temp_dir().join(lokai_memory::new_id("h32"));
    let (app, artifacts, mem) = make_app_with_store(&dir);
    let (session_id, run_id, attempt_id) = start_and_plan(&app).await;
    {
        let _ = mem.write_sync({
            let session_id = session_id.clone();
            move |db| {
                db.append_message(&session_id, "assistant", "", "hi", None)
                    .unwrap();
            }
        });
    }
    app.runs
        .complete_turn(
            &commands::CompleteTurnCommand {
                session_id: session_id.clone(),
                attempt_id: attempt_id.clone(),
                workspace_root: std::env::temp_dir().display().to_string(),
                canceled: false,
                error: None,
            },
            None,
            None,
        )
        .await
        .expect("complete");
    let (artifact_id, _) = accepted_digest(&app, run_id).await;
    let id = ArtifactId::new(artifact_id);
    let meta = artifacts.metadata(&id).await.expect("meta");
    match meta.storage_location {
        ArtifactLocation::LocalFile(path) => std::fs::write(path, b"tampered").unwrap(),
        other => panic!("expected local file {other:?}"),
    }
    let err = verify_stored_content(artifacts.as_ref(), &id)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            lokai_domain::artifact::ArtifactError::DigestMismatch { .. }
        ),
        "{err:?}"
    );
}

#[tokio::test]
async fn identical_output_same_digest_changed_file_differs() {
    let dir = std::env::temp_dir().join(lokai_memory::new_id("h32"));
    async fn run(dir: &std::path::Path, after: &str) -> String {
        let (app, _, mem) = make_app_with_store(dir);
        let (session_id, run_id, attempt_id) = start_and_plan(&app).await;
        {
            let _ = mem.write_sync({
                let session_id = session_id.clone();
                let after = after.to_string();
                move |db| {
                    db.record_file_change("tc1", &session_id, "a.txt", "write", None, Some(&after))
                        .unwrap();
                    db.append_message(&session_id, "assistant", "", "done", None)
                        .unwrap();
                }
            });
        }
        let outcome = CandidateOutcome::Completed {
            summary: after.to_string(),
            kind: CompletionKind::Finish,
        };
        app.runs
            .complete_turn(
                &commands::CompleteTurnCommand {
                    session_id: session_id.clone(),
                    attempt_id: attempt_id.clone(),
                    workspace_root: std::env::temp_dir().display().to_string(),
                    canceled: false,
                    error: None,
                },
                Some(&outcome),
                None,
            )
            .await
            .expect("complete");
        accepted_digest(&app, run_id).await.1
    }
    let d1 = run(&dir.join("a"), "same").await;
    let d2 = run(&dir.join("b"), "same").await;
    let d3 = run(&dir.join("c"), "diff").await;
    assert_eq!(d1, d2);
    assert_ne!(d1, d3);
}

#[tokio::test]
async fn failed_turn_records_error_digest_not_success() {
    let dir = std::env::temp_dir().join(lokai_memory::new_id("h32"));
    let (ok_app, _, ok_mem) = make_app_with_store(&dir.join("ok"));
    let (ok_session, ok_run, ok_attempt) = start_and_plan(&ok_app).await;
    {
        let _ = ok_mem.write_sync({
            let ok_session = ok_session.clone();
            move |db| {
                db.append_message(&ok_session, "assistant", "", "", None)
                    .unwrap();
            }
        });
    }
    ok_app
        .runs
        .complete_turn(
            &commands::CompleteTurnCommand {
                session_id: ok_session.clone(),
                attempt_id: ok_attempt,
                workspace_root: std::env::temp_dir().display().to_string(),
                canceled: false,
                error: None,
            },
            None,
            None,
        )
        .await
        .expect("ok complete");
    let ok_digest = accepted_digest(&ok_app, ok_run).await.1;

    let (err_app, _, _) = make_app_with_store(&dir.join("err"));
    let (err_session, err_run, err_attempt) = start_and_plan(&err_app).await;
    err_app
        .runs
        .complete_turn(
            &commands::CompleteTurnCommand {
                session_id: err_session.clone(),
                attempt_id: err_attempt,
                workspace_root: std::env::temp_dir().display().to_string(),
                canceled: false,
                error: Some("tool failed".into()),
            },
            None,
            None,
        )
        .await
        .expect("error complete");
    let err_snap = err_app
        .runs
        .inspect_run(commands::InspectRunCommand {
            run_id: err_run.to_string(),
        })
        .await
        .expect("error snapshot");
    assert_eq!(err_snap.state, RunState::Failed);
    let err_task = lokai_domain::TaskId::new(format!("task_root_{err_run}"));
    assert!(
        err_snap
            .tasks
            .get(&err_task)
            .and_then(|t| t.accepted_artifact.clone())
            .is_none(),
        "Failed must not AcceptArtifact"
    );
    assert!(err_snap
        .attempts
        .values()
        .any(|a| a.state == AttemptState::Failed));
    assert!(!err_snap
        .attempts
        .values()
        .any(|a| a.state == AttemptState::Succeeded));
    assert!(ok_digest.starts_with("sha256:"));
}

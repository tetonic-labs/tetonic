//! Eval recovery honesty (R06 / M0-4 Partial).
//!
//! One real crash→restart proof against a temp `lokai.db` + workspace.
//! Replaces the prior always-Ok greenwash probe.

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Named fault-inject point: after turn plan has durable run journal + live bind.
/// Production calls [`lokai_telemetry::fault::inject_fault`] with this string.
pub const INJECT_AFTER_TURN_PLAN: &str = "after_turn_plan";

/// Documented recovery contract for this inject point (R06).
///
/// If the process dies after `plan_turn` has committed the run journal and a
/// turn_operation remains in a recovery-required state (`executing`), restart
/// must:
/// - restore the run snapshot from `lokai.db` (no silent empty journal),
/// - report `resume_state = recovery_required` (never treat as clean success),
/// - not claim a finished turn.
pub const AFTER_TURN_PLAN_CONTRACT: &str = "\
crash after_turn_plan → reopen same lokai.db → run journal present; \
session resume_state=recovery_required; no silent half-commit success";

struct FakeEventSink;
impl lokai_app::events::ApplicationEventSink for FakeEventSink {
    fn send(&self, _event: lokai_app::events::ApplicationEvent) {}
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lokai-eval-r06-{}-{}-{}",
        label,
        std::process::id(),
        lokai_memory::new_id("r06")
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn make_app(dir: &Path) -> (lokai_app::Application, lokai_memory::SharedStore) {
    let db_path = dir.join("lokai.db");
    let store = lokai_memory::SharedStore::open(&db_path, 1).unwrap();
    let policy = Arc::new(lokai_policy::PolicyEngine::default());
    // Eval scans with the same engine production does (M6, ADR-V3-016): eval is
    // how CI proves the system works, so it must not prove it on a path that
    // skips the scanner.
    let artifacts = Arc::new(
        lokai_artifact::LocalArtifactStore::new(
            dir.join("artifacts"),
            lokai_app::secret_scanner_factory::artifact_scan_policy(&Some(store.clone())),
        )
        .unwrap(),
    );
    let runtime = lokai_runtime::EngineRuntime::new(policy.clone(), None, artifacts);
    let app = lokai_app::Application::new(lokai_app::ApplicationDependencies {
        runtime: Arc::new(runtime),
        store: Some(store.clone()),
        policy,
        event_sink: Arc::new(FakeEventSink),
        index_db: None,
        fabric_hint: None,
    });
    (app, store)
}

/// Simulate a crash at [`INJECT_AFTER_TURN_PLAN`]: durable plan + incomplete turn op,
/// then drop the in-memory Application (process death).
///
/// Returns `(data_dir, workspace, session_id, run_id)` for the restart half.
pub async fn crash_after_turn_plan(
) -> Result<(PathBuf, PathBuf, String, lokai_domain::RunId), String> {
    let dir = temp_dir("crash");
    let ws = dir.join("ws");
    std::fs::create_dir_all(&ws).map_err(|e| e.to_string())?;
    let (app, store) = make_app(&dir);
    let started = app
        .sessions
        .start_session(lokai_app::commands::StartSessionCommand {
            workspace_root: ws.display().to_string(),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .map_err(|e| e.to_string())?;
    let sid = started.session_id.clone();
    let plan = app
        .runs
        .plan_turn(&lokai_app::commands::RunTurnCommand {
            session_id: sid.clone(),
            user_input: "r06 recovery probe".into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .map_err(|e| e.to_string())?;
    let run_id = plan.run_id.clone();
    store
        .write({
            let sid = sid.clone();
            move |db| {
                db.upsert_turn_operation(&sid, "turn_r06", "executing", "{}")
                    .map_err(|e| e.to_string())
            }
        })
        .await
        .map_err(|e| e.to_string())??;

    // Named inject point: process would exit(1) when LOKAI_FAULT_INJECT matches.
    lokai_telemetry::fault::inject_fault(INJECT_AFTER_TURN_PLAN);

    // Crash: drop live process state without finishing the turn.
    drop(app);
    drop(store);
    Ok((dir, ws, sid, run_id))
}

/// Restart half of the [`INJECT_AFTER_TURN_PLAN`] contract.
pub async fn assert_recovery_after_turn_plan_crash(
    dir: &Path,
    ws: &Path,
    session_id: &str,
    run_id: &lokai_domain::RunId,
) -> Result<(), String> {
    let (app2, _) = make_app(dir);
    let snap = app2
        .runs
        .inspect_run(lokai_app::commands::InspectRunCommand {
            run_id: run_id.to_string(),
        })
        .await
        .map_err(|e| format!("run journal missing after crash: {e}"))?;
    if snap.attempts.is_empty() {
        return Err("run journal restored but attempts empty: silent half-commit".into());
    }
    if matches!(
        snap.state,
        lokai_domain::RunState::Succeeded | lokai_domain::RunState::Canceled
    ) {
        return Err(format!(
            "run must not look finished after mid-turn crash, got {:?}",
            snap.state
        ));
    }

    let resumed = app2
        .sessions
        .start_session(lokai_app::commands::StartSessionCommand {
            workspace_root: ws.display().to_string(),
            resume: Some(true),
            session_id: Some(session_id.to_string()),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .map_err(|e| e.to_string())?;
    if resumed.resume_state != "recovery_required" {
        return Err(format!(
            "expected resume_state=recovery_required, got `{}` (contract: {AFTER_TURN_PLAN_CONTRACT})",
            resumed.resume_state
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn crash_after_turn_plan_then_restart_is_recovery_required() {
        let (dir, ws, sid, run_id) = crash_after_turn_plan().await.expect("crash half");
        assert_recovery_after_turn_plan_crash(&dir, &ws, &sid, &run_id)
            .await
            .expect("recovery contract");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn greenwash_version_path_is_gone() {
        let src = include_str!("recovery.rs");
        let production = src.split("#[cfg(test)]").next().unwrap_or(src);
        assert!(
            !production.contains("current_exe") && !production.contains("Command::new"),
            "production recovery must not shell out to a sibling binary"
        );
        assert!(
            production.contains(INJECT_AFTER_TURN_PLAN),
            "named inject point required"
        );
        assert!(
            production.contains("recovery_required"),
            "must assert durable recovery_required outcome"
        );
    }
}

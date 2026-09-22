//! WORK-02 Attempt-keyed execution / Infer admission / Agent::run pins.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use lokai_app::commands;
use lokai_app::events;
use lokai_app::{Application, ApplicationDependencies};

struct FakeEventSink;
impl events::ApplicationEventSink for FakeEventSink {
    fn send(&self, _event: events::ApplicationEvent) {}
}

static WORK02_DB: AtomicU64 = AtomicU64::new(0);

fn make_app() -> Application {
    let db_path = std::env::temp_dir().join(format!(
        "lokai_work02_{}_{}.db",
        std::process::id(),
        WORK02_DB.fetch_add(1, Ordering::Relaxed)
    ));
    let store = lokai_memory::SharedStore::open(&db_path, 1).unwrap();
    let policy = Arc::new(lokai_policy::PolicyEngine::default());
    let artifact_store = Arc::new(
        lokai_artifact::LocalArtifactStore::new(
            std::env::temp_dir().join("artifacts"),
            lokai_app::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );
    let runtime = lokai_runtime::EngineRuntime::new(policy.clone(), None, artifact_store);
    Application::new(ApplicationDependencies {
        runtime: Arc::new(runtime),
        store: Some(store),
        policy,
        event_sink: Arc::new(FakeEventSink),
        index_db: None,
        fabric_hint: None,
    })
}

fn run_service_src() -> String {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut src = fs::read_to_string(dir.join("run_service.rs")).expect("run_service.rs");
    src.push_str(
        &fs::read_to_string(dir.join("turn_finalization.rs")).expect("turn_finalization.rs"),
    );
    src
}

fn production_prefix(src: &str) -> &str {
    src.split("#[cfg(test)]").next().unwrap_or(src)
}

#[test]
fn work02_active_is_attempt_keyed() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Registry);
}

#[tokio::test]
async fn work02_complete_turn_has_no_session_run_fallback() {
    let src = run_service_src();
    assert!(!src.contains("unwrap_or_else(|| cmd.session_id.clone())"));
    let app = make_app();
    let started = app
        .sessions
        .start_session(commands::StartSessionCommand {
            workspace_root: std::env::temp_dir().display().to_string(),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .expect("session");
    let err = app
        .runs
        .complete_turn(
            &commands::CompleteTurnCommand {
                session_id: started.session_id.clone(),
                attempt_id: lokai_domain::AttemptId::new("att_missing"),
                workspace_root: std::env::temp_dir().display().to_string(),
                canceled: false,
                error: None,
            },
            None,
            None,
        )
        .await
        .expect_err("miss must not session-as-run");
    assert!(err.to_string().contains("no active turn run"));
}

#[tokio::test]
async fn work02_spawn_empty_active_inserts_by_attempt() {
    let src = run_service_src();
    assert!(
        src.contains("insert(active.attempt_id.clone(), active")
            || src.contains("insert(attempt_id.clone(), active)")
    );
    let app = make_app();
    let started = app
        .sessions
        .start_session(commands::StartSessionCommand {
            workspace_root: std::env::temp_dir().display().to_string(),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .expect("session");
    let attempt = app
        .runs
        .register_spawn_task(&started.session_id, "a0_s0", "coder", "")
        .await
        .expect("empty-active spawn");
    assert!(!attempt.0.is_empty());
    assert_ne!(attempt.0, started.session_id);
}

#[test]
fn work02_attest_uses_attempt() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Cleanup);
}

#[test]
fn work02_finish_clears_live_by_session() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Cleanup);
}

#[test]
fn work02_heartbeat_uses_attempt() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Heartbeat);
}

#[tokio::test]
async fn work02_cancel_session_drops_attempt_entries() {
    let app = make_app();
    let started = app
        .sessions
        .start_session(commands::StartSessionCommand {
            workspace_root: std::env::temp_dir().display().to_string(),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .expect("session");
    let plan = app
        .runs
        .plan_turn(&commands::RunTurnCommand {
            session_id: started.session_id.clone(),
            user_input: "cancel drop".into(),
            verify_cmd: None,
            llm_router: Some(false),
        })
        .await
        .expect("plan");
    app.sessions
        .cancel_session(commands::CancelRunCommand {
            session_id: started.session_id.clone(),
            pooled_cancel: false,
        })
        .await
        .expect("cancel");
    let err = app
        .runs
        .complete_turn(
            &commands::CompleteTurnCommand {
                session_id: started.session_id,
                attempt_id: plan.attempt_id,
                workspace_root: std::env::temp_dir().display().to_string(),
                canceled: false,
                error: None,
            },
            None,
            None,
        )
        .await
        .expect_err("dropped attempt");
    assert!(err.to_string().contains("no active turn run"));
}

#[test]
fn work02_root_start_uses_local_executor() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Execution);
}

#[test]
fn work02_orchestrator_leftover_turn_remains() {
    let src = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../mantle/lokai-orchestrator/src/turn.rs"),
    )
    .expect("turn.rs");
    let prod = src.split("#[cfg(test)]").next().unwrap_or(&src);
    assert!(prod.contains("root_execute"));
    assert!(prod.contains("trait ChildJob"));
    assert!(!prod.contains(".turn("));
}

#[test]
fn work02_one_local_executor_impl() {
    let engine_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut impls = Vec::new();
    let layers = [
        "core",
        "strata",
        "mantle",
        "atmos",
        "litho",
        "portals",
        "product",
        "manager",
        "substrate",
        "compute",
        "capabilities",
        "infrastructure",
        "tooling",
        "bins",
        "crates",
    ];
    for layer in layers {
        walk_production_rs(&engine_root.join(layer), &mut |path, src| {
            if src.contains("AgentAttemptExecutor for") {
                impls.push(path.display().to_string());
            }
        });
    }
    assert_eq!(impls.len(), 1, "found {impls:?}");
    assert!(impls[0]
        .replace('\\', "/")
        .ends_with("lokai-runtime/src/executor.rs"));
}

#[test]
fn work02_agent_run_deleted() {
    let src = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../core/lokai-core/src/agent.rs"),
    )
    .expect("agent.rs");
    assert!(!src.contains("pub async fn run<F>"));
}

#[test]
fn work02_broker_has_no_runcommand_lifecycle() {
    let lease = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../mantle/lokai-broker/src/scheduler/attempt_lease.rs"),
    )
    .expect("attempt_lease.rs");
    let prod = production_prefix(&lease);
    for needle in [
        "RunCommand::CreateRun",
        "RunCommand::StartRun",
        "RunCommand::AddTask",
        "RunCommand::FailAttempt",
        "RunCommand::CreateAttempt",
        "RunCommand::LeaseAttempt",
        "RunCommand::CancelTask",
    ] {
        assert!(
            !prod.contains(needle),
            "production broker still has {needle}"
        );
    }
    let broker = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../mantle/lokai-broker/src/broker.rs"),
    )
    .expect("broker.rs");
    assert!(!production_prefix(&broker).contains("RunCommand::CancelTask"));
}

#[test]
fn work02_infer_hop_still_no_job_spec() {
    let src = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../mantle/lokai-run/src/infer_admission.rs"),
    )
    .expect("infer_admission.rs");
    assert!(src.contains("job_spec: None"));
    assert!(!src.contains("job_spec: Some"));
    assert!(lokai_run::hop_job_spec_must_be_none(None));
}

#[test]
fn work02_id001_not_established() {
    let live =
        fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/session_live.rs"))
            .expect("session_live.rs");
    assert!(live.contains("pub struct LiveSession"));
    assert!(!live.contains("AgentIdentity"));
}

#[test]
fn work02_work002_not_established_portal_futures_remain() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::ProductDispatch);
}

#[test]
fn work02_cmp001_not_established_fabric_complete_remains() {
    let src = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/fabric_run_bridge.rs"),
    )
    .expect("fabric_run_bridge.rs");
    assert!(!src.contains("apply_complete_attempt"));
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tooling/lokai-arch-gate/fixtures/v4/ARCH-V4-CMP-001.v4fix");
    assert!(
        fixture.exists(),
        "ARCH-V4-CMP-001 stays planted inventory; not ESTABLISHED"
    );
}

fn walk_production_rs(root: &Path, visit: &mut impl FnMut(&Path, &str)) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if matches!(name, "target" | "tests") {
                    continue;
                }
                stack.push(path);
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                if let Ok(src) = fs::read_to_string(&path) {
                    visit(&path, &src);
                }
            }
        }
    }
}

#[path = "support/lifecycle_contract.rs"]
mod lifecycle_contract;

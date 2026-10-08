//! PORTAL-01 IMPLEMENT source pins. Close `011`/`012`/`013`/`019`/`020` at CONVERGE only.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use tetonic_app::commands::CancelByRunCommand;
use tetonic_app::events::ApplicationEventSink;
use tetonic_app::{Application, ApplicationDependencies};

fn crate_src(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    if path.exists() {
        return fs::read_to_string(&path).expect(rel);
    }
    let alt_rel = rel.replace("lokai-", "tetonic-");
    let alt_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(&alt_rel);
    if alt_path.exists() {
        return fs::read_to_string(&alt_path).expect(&alt_rel);
    }
    fs::read_to_string(&path).expect(rel)
}

fn fn_body<'a>(src: &'a str, sig: &str) -> &'a str {
    let start = src.rfind(sig).expect(sig);
    let after = &src[start..];
    let brace = after.find('{').expect("fn body");
    let bytes = &after.as_bytes()[brace..];
    let mut depth = 0i32;
    for (i, ch) in bytes.iter().enumerate() {
        match *ch {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &after[brace..=brace + i];
                }
            }
            _ => {}
        }
    }
    panic!("unclosed {sig}");
}

struct FakeEventSink;
impl ApplicationEventSink for FakeEventSink {
    fn send(&self, _event: tetonic_app::events::ApplicationEvent) {}
}

static PORTAL01_DB: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn make_app() -> Application {
    let db_path = std::env::temp_dir().join(format!(
        "lokai_portal01_{}_{}_{}.db",
        std::process::id(),
        PORTAL01_DB.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = tetonic_memory::SharedStore::open(&db_path, 1).unwrap();
    let policy = Arc::new(tetonic_policy::PolicyEngine::default());
    let artifact_store = Arc::new(
        tetonic_artifact::LocalArtifactStore::new(
            std::env::temp_dir().join("artifacts"),
            tetonic_app::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );
    let runtime = tetonic_runtime::EngineRuntime::new(policy.clone(), None, artifact_store);
    Application::new(ApplicationDependencies {
        runtime: Arc::new(runtime),
        store: Some(store),
        policy,
        event_sink: Arc::new(FakeEventSink),
        index_db: None,
    })
    .with_execution_policy(std::sync::Arc::new(
        tetonic_app::definition::validate_coding_execution,
    ))
}

#[test]
fn portal01_start_identity_job_still_none() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Sessionless);
}

#[test]
fn portal01_create_run_still_persist_only() {
    let src = crate_src("src/run_service.rs");
    let body = fn_body(&src, "async fn create_run(");
    assert!(body.contains("RunCommand::CreateRun"));
    assert!(!body.contains("StartRun"));
    assert!(!body.contains("execute_turn"));
}

#[test]
fn portal01_local_ui_no_take_restore() {
    let src = crate_src("../../litho/tetonic-cli/src/local_ui.rs");
    assert!(!src.contains("take_conversation"));
    assert!(!src.contains("restore_conversation"));
}

#[test]
fn portal01_fabric_spawn_local_untouched() {
    let src = crate_src("src/node_worker.rs");
    assert!(src.contains("tokio::task::spawn_local"));
    assert!(src.contains("server.run("));
}

#[test]
fn portal01_local_ui_no_coding_pack_parse() {
    let src = crate_src("../../litho/tetonic-cli/src/local_ui.rs");
    assert!(!src.contains("CodingPack"));
}

#[test]
fn portal01_local_ui_no_private_approval_waiter() {
    let ui = crate_src("../../litho/tetonic-cli/src/local_ui.rs");
    assert!(!ui.contains("RpcApprovalWaiter"));
    assert!(!ui.contains("take_pending"));
    assert!(!ui.contains("oneshot::Sender<bool>"));
    assert!(ui.contains(".resolve_approval(id, payload)"));
}

#[test]
fn portal01_register_request_no_live_lookup() {
    let src = crate_src("src/approval.rs");
    assert!(!src.contains("SessionLiveStore"));
    let body = fn_body(&src, "fn register_request(");
    assert!(body.contains("cmd.auto_grant_approvals"));
    assert!(!body.contains("sessions.live"));
}

#[test]
fn portal01_cancel_run_does_not_require_session() {
    let src = crate_src("src/run_service.rs");
    let body = fn_body(&src, "async fn cancel_run(");
    assert!(!body.contains("fail_session_waits"));
    assert!(!body.contains("LiveSession"));
}

#[test]
fn portal01_step_to_events_remains() {
    let src = crate_src("src/turn_execution.rs");
    assert!(src.contains("fn step_to_events("));
}

#[test]
fn portal01_supervisor_field_remains() {
    let src = crate_src("src/lib.rs");
    assert!(src.contains("supervisor: Arc<dyn tetonic_run::RunSupervisor>"));
    assert!(!src.contains("pub supervisor: Arc<dyn tetonic_run::RunSupervisor>"));
    let cli = crate_src("../../litho/tetonic-cli/src/main.rs");
    let ui = crate_src("../../litho/tetonic-cli/src/local_ui.rs");
    assert!(!cli.contains("RunSupervisor"));
    assert!(!ui.contains("RunSupervisor"));
}

#[test]
fn portal01_iface001_not_established() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tooling/tetonic-arch-gate/fixtures/v4/ARCH-V4-IFACE-001.v4fix");
    assert!(fixture.is_file());
}

#[test]
fn portal01_arch_iface_001_still_planted() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tooling/tetonic-arch-gate/fixtures/v4/ARCH-V4-IFACE-001.v4fix");
    let body = fs::read_to_string(&fixture).expect("iface fixture");
    assert!(fixture.is_file());
    assert!(body.contains("production-failing detector live"));
}

#[test]
fn portal01_readme_not_terminal_only_claim() {
    let src = crate_src("../../litho/tetonic-cli/README.md");
    assert!(!src.contains("The CLI owns terminal rendering only."));
}

#[tokio::test]
async fn portal01_cancel_run_still_sessionless() {
    let app = make_app();
    let err = app
        .runs
        .cancel_run(CancelByRunCommand {
            run_id: "run_missing".into(),
        })
        .await
        .expect_err("missing run");
    assert!(
        err.to_string().contains("run not found"),
        "cancel_run is sessionless inspect, got {err}"
    );
}

#[path = "support/lifecycle_contract.rs"]
mod lifecycle_contract;

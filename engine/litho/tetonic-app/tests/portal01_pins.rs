//! PORTAL-01 IMPLEMENT source pins. Close `011`/`012`/`013`/`019`/`020` at CONVERGE only.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use tetonic_app::commands::{
    CancelByRunCommand, CancelRunCommand, RegisterApprovalCommand, StartSessionCommand,
};
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

fn production_prefix(src: &str) -> &str {
    src.split("#[cfg(test)]").next().unwrap_or(src)
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

fn source_order(haystack: &str, earlier: &str, later: &str) {
    let a = haystack
        .find(earlier)
        .unwrap_or_else(|| panic!("missing `{earlier}`"));
    let b = haystack
        .find(later)
        .unwrap_or_else(|| panic!("missing `{later}`"));
    assert!(a < b, "`{earlier}` must precede `{later}`");
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
        fabric_hint: None,
    })
}

#[test]
fn portal01_submit_chat_turn_returns_without_conversation_arg() {
    let src = crate_src("src/product_submit.rs");
    assert!(src.contains(
        "pub fn submit_chat_turn(&self, mut cmd: RunTurnCommand) -> Result<(), AppError>"
    ));
    assert!(!src.contains("&mut Conversation"));
}

#[test]
fn portal01_submit_does_not_return_attempt_before_plan() {
    let src = crate_src("src/product_submit.rs");
    let body = fn_body(&src, "pub fn submit_chat_turn(");
    assert!(body.contains("Ok(())"));
    assert!(!body.contains("AttemptId"));
}

#[test]
fn portal01_arm_turn_join_before_submit() {
    let src = crate_src("src/product_submit.rs");
    assert!(src.contains("pub fn arm_turn_join("));
    assert!(src.contains("TurnFinish"));
    let cli = crate_src("../../litho/tetonic-cli/src/chat.rs");
    let one_shot = fn_body(&cli, "pub async fn run_one_shot(");
    source_order(one_shot, "arm_turn_join", "submit_chat_turn");
    let eval = crate_src("../../tooling/tetonic-eval/src/kernel.rs");
    let eval_body = fn_body(&eval, "async fn run_kernel_turn(");
    source_order(eval_body, "arm_turn_join", "submit_chat_turn");
}

#[test]
fn portal01_run_turn_symbol_remains() {
    let src = crate_src("src/run_service.rs");
    assert!(src.contains("async fn run_turn("));
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
fn portal01_app_root_execute_still_local() {
    let src = crate_src("src/turn_execution.rs");
    assert!(src.contains("struct AppRootExecute"));
    assert!(src.contains(".execute_attempt("));
}

#[test]
fn portal01_host_not_built_from_cli_turn_context() {
    let src = crate_src("src/product_submit.rs");
    assert!(!src.contains("CliTurnContext"));
    let cli = crate_src("../../litho/tetonic-cli/src/session.rs");
    assert!(!cli.contains("fn turn_host"));
}

#[test]
fn portal01_submit_admits_then_leases() {
    let src = crate_src("src/product_submit.rs");
    let body = fn_body(&src, "pub fn submit_chat_turn(");
    source_order(body, "admit_chat_turn", "take_conversation");
    source_order(body, "take_conversation", "dispatch_chat_turn");
}

#[test]
fn portal01_admit_reject_does_not_spawn() {
    let src = crate_src("src/product_submit.rs");
    let body = fn_body(&src, "pub fn submit_chat_turn(");
    assert!(body.contains("TurnAdmitDecision::Reject"));
    source_order(body, "TurnAdmitDecision::Reject", "dispatch_chat_turn");
}

#[test]
fn portal01_end_turn_on_drop() {
    let src = crate_src("src/product_submit.rs");
    assert!(src.contains("impl Drop for OwnedTurn"));
    let drop_body = fn_body(&src, "impl Drop for OwnedTurn");
    assert!(drop_body.contains("end_turn()"));
    assert!(drop_body.contains("restore_conversation"));
}

#[test]
fn portal01_lease_fail_after_admit_ends_turn() {
    let src = crate_src("src/product_submit.rs");
    let body = fn_body(&src, "pub fn submit_chat_turn(");
    assert!(body.contains("live.end_turn()"));
    source_order(body, "take_conversation", "end_turn");
}

#[test]
fn portal01_audit_factory_is_store_projection_not_wrap() {
    let src = crate_src("src/product_submit.rs");
    assert!(src.contains("product_audit_factory"));
    assert!(!src.contains("ExecutionProjectionAudit"));
    let store = crate_src("src/store_audit.rs");
    assert!(store.contains("struct StoreAudit"));
}

#[test]
fn portal01_cli_chat_no_take_restore() {
    let owned = crate_src("../../litho/tetonic-cli/src/chat.rs");
    let src = production_prefix(&owned);
    assert!(!src.contains("take_conversation"));
    assert!(!src.contains("restore_conversation"));
}

#[test]
fn portal01_cli_chat_no_turn_spawn_local() {
    let src = crate_src("../../litho/tetonic-cli/src/chat.rs");
    assert!(!src.contains("runs.run_turn"));
    assert!(!src.contains("execute_turn"));
}

#[test]
fn portal01_cli_one_shot_does_not_await_run_turn() {
    let src = crate_src("../../litho/tetonic-cli/src/chat.rs");
    let body = fn_body(&src, "pub async fn run_one_shot(");
    assert!(body.contains("arm_turn_join"));
    assert!(body.contains("submit_chat_turn"));
    assert!(!body.contains("run_turn("));
}

#[test]
fn portal01_cli_turn_host_gone() {
    let src = crate_src("../../litho/tetonic-cli/src/session.rs");
    assert!(!src.contains("TurnExecutionHost"));
    assert!(!src.contains("fn turn_host"));
}

#[test]
fn portal01_cli_no_admit_chat_turn() {
    let chat = crate_src("../../litho/tetonic-cli/src/chat.rs");
    assert!(!chat.contains("admit_chat_turn"));
    assert!(!chat.contains("admit_cli_turn"));
}

#[test]
fn portal01_cli_no_end_turn() {
    let chat = crate_src("../../litho/tetonic-cli/src/chat.rs");
    assert!(!chat.contains("end_turn"));
}

#[test]
fn portal01_daemon_chat_no_spawn_local() {
    let src = crate_src("../../litho/tetonicd/src/daemon/handlers/chat.rs");
    assert!(!src.contains("spawn_local"));
    assert!(src.contains("submit_chat_turn"));
}

#[test]
fn portal01_daemon_chat_no_take_restore() {
    let src = crate_src("../../litho/tetonicd/src/daemon/handlers/chat.rs");
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
fn portal01_daemon_spawn_no_coding_pack_parse() {
    let src = crate_src("../../litho/tetonicd/src/daemon/handlers/agent.rs");
    assert!(!src.contains("CodingPack"));
}

#[test]
fn portal01_daemon_spawn_no_execute_spawn_call() {
    let src = crate_src("../../litho/tetonicd/src/daemon/handlers/agent.rs");
    assert!(!src.contains("execute_spawn"));
    assert!(src.contains("submit_spawn"));
}

#[test]
fn portal01_execute_spawn_symbol_remains() {
    let src = crate_src("src/turn_execution.rs");
    assert!(src.contains("async fn execute_spawn("));
}

#[test]
fn portal01_parse_spawn_role_remains() {
    let src = crate_src("src/turn_execution.rs");
    assert!(src.contains("fn parse_spawn_role("));
}

#[test]
fn portal01_no_live_session_pending_oneshot() {
    let src = crate_src("src/session_live.rs");
    assert!(!src.contains("PendingApproval"));
    assert!(!src.contains("insert_pending"));
    assert!(!src.contains("take_pending"));
}

#[test]
fn portal01_cli_no_approval_oneshot() {
    let kernel = crate_src("../../litho/tetonic-cli/src/app_kernel.rs");
    assert!(!kernel.contains("oneshot::Sender<bool>"));
    assert!(!kernel.contains("impl ApprovalWaiter"));
    let tui = crate_src("../../litho/tetonic-cli/src/tui/mod.rs");
    assert!(!tui.contains("approval_tx"));
}

#[test]
fn portal01_daemon_no_rpc_approval_waiter() {
    let chat = crate_src("../../litho/tetonicd/src/daemon/handlers/chat.rs");
    let agent = crate_src("../../litho/tetonicd/src/daemon/handlers/agent.rs");
    let misc = crate_src("../../litho/tetonicd/src/daemon/handlers/misc.rs");
    assert!(!chat.contains("RpcApprovalWaiter"));
    assert!(!agent.contains("RpcApprovalWaiter"));
    assert!(!misc.contains("take_pending"));
}

#[test]
fn portal01_approval_ux_respond_remains() {
    let tui = crate_src("../../litho/tetonic-cli/src/tui/mod.rs");
    assert!(tui.contains("approvals.respond") || tui.contains("svc.respond"));
    let misc = crate_src("../../litho/tetonicd/src/daemon/handlers/misc.rs");
    assert!(misc.contains("approvals"));
    assert!(misc.contains("respond("));
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
fn portal01_daemon_toolcall_rpc_mapped() {
    let src = crate_src("../../litho/tetonicd/src/daemon/events.rs");
    assert!(src.contains("ApplicationEvent::ToolCall"));
    assert!(src.contains("events::TOOL_CALL"));
    assert!(src.contains("ApplicationEvent::ToolResult"));
    assert!(src.contains("events::TOOL_RESULT"));
    assert!(!src.contains("audit path owns RPC notify"));
}

#[test]
fn portal01_step_to_events_remains() {
    let src = crate_src("src/turn_execution.rs");
    assert!(src.contains("fn step_to_events("));
}

#[test]
fn portal01_eval_kernel_no_take_restore() {
    let src = crate_src("../../tooling/tetonic-eval/src/kernel.rs");
    assert!(!src.contains("take_conversation"));
    assert!(!src.contains("restore_conversation"));
}

#[test]
fn portal01_eval_kernel_no_run_turn_call() {
    let src = crate_src("../../tooling/tetonic-eval/src/kernel.rs");
    assert!(!src.contains("runs.run_turn"));
    assert!(src.contains("submit_chat_turn"));
}

#[test]
fn portal01_eval_verify_cmd_none_untouched() {
    let src = crate_src("../../tooling/tetonic-eval/src/kernel.rs");
    assert!(src.contains("verify_cmd: None"));
}

#[test]
fn portal01_eval_kernel_no_admit() {
    let src = crate_src("../../tooling/tetonic-eval/src/kernel.rs");
    assert!(!src.contains("admit_chat_turn"));
    assert!(!src.contains("end_turn"));
}

#[test]
fn portal01_eval_kernel_no_autogrant() {
    let src = crate_src("../../tooling/tetonic-eval/src/kernel.rs");
    assert!(!src.contains("AutoGrant"));
    assert!(!src.contains("TurnExecutionHost {"));
    assert!(src.contains("auto_grant_approvals: Some(true)"));
}

#[test]
fn portal01_eval_timeout_uses_cancel_session() {
    let src = crate_src("../../tooling/tetonic-eval/src/kernel.rs");
    assert!(src.contains("cancel_session"));
    assert!(src.contains("wall_clock_limit"));
}

#[test]
fn portal01_eval_timeout_does_not_call_cancel_run() {
    let src = crate_src("../../tooling/tetonic-eval/src/kernel.rs");
    assert!(!src.contains("cancel_run"));
}

#[test]
fn portal01_eval_timeout_does_not_drop_future() {
    let src = crate_src("../../tooling/tetonic-eval/src/kernel.rs");
    assert!(src.contains("submit_chat_turn"));
    assert!(!src.contains("timeout(\n        wall,\n        app.runs.run_turn"));
}

#[test]
fn portal01_turn_execution_host_type_remains() {
    let src = crate_src("src/turn_execution.rs");
    assert!(src.contains("pub(crate) struct TurnExecutionHost"));
}

#[test]
fn portal01_supervisor_field_remains() {
    let src = crate_src("src/lib.rs");
    assert!(src.contains("supervisor: Arc<dyn tetonic_run::RunSupervisor>"));
    assert!(!src.contains("pub supervisor: Arc<dyn tetonic_run::RunSupervisor>"));
    let cli = crate_src("../../litho/tetonic-cli/src/main.rs");
    let daemon = crate_src("../../litho/tetonicd/src/daemon/handlers/initialize.rs");
    assert!(!cli.contains("RunSupervisor"));
    assert!(!daemon.contains("RunSupervisor"));
}

#[test]
fn portal01_live_session_remains() {
    let src = crate_src("src/session_live.rs");
    assert!(src.contains("pub struct LiveSession"));
}

#[test]
fn portal01_bh_id_session_still_defect() {
    let src = crate_src("src/session_live.rs");
    assert!(src.contains("pub struct LiveSession"));
    assert!(!src.contains("AgentIdentity"));
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
async fn portal01_cancel_session_fails_parked_wait() {
    let app = make_app();
    let tmp = std::env::temp_dir();
    let started = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: tmp.display().to_string(),
            briefing: Some(false),
            ..Default::default()
        })
        .await
        .expect("session");
    let rx = app
        .approvals
        .register_request(RegisterApprovalCommand {
            session_id: started.session_id.clone(),
            approval_id: "ap_park".into(),
            call_id: "c1".into(),
            kind: "run_shell".into(),
            detail: "echo".into(),
            tool: "run_shell".into(),
            args: serde_json::json!({}),
            missing_controls: vec![],
            user_approval_required: true,
            auto_grant_approvals: false,
            attempt_id: None,
        })
        .expect("park");
    app.sessions
        .cancel_session(CancelRunCommand {
            session_id: started.session_id.clone(),
            pooled_cancel: false,
        })
        .await
        .expect("cancel");
    assert!(!rx.await.unwrap());
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

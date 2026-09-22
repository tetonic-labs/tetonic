//! AUD-01 same-crate characterization of private run_service sequences and
//! pub(crate) stream mapping. Does not expand production visibility.

use super::*;
use crate::approval::{ApprovalService, DefaultApprovalService};
use crate::coding_pack::CodingPack;
use crate::commands::{ApprovalResponseCommand, StartSessionCommand};
use crate::events::{ApplicationEvent, ApplicationEventSink, RecordingEventSink};
use crate::session_live::LiveSession;
use crate::turn_execution::step_to_events;
use crate::{Application, ApplicationDependencies};
use tetonic_core::{Conversation, Step};
use tetonic_domain::{DataClass, DisclosureTier};
use tetonic_memory::RecoverMutex;
use tetonic_orchestrator::{RoleId, SessionStartPlan, SpecialistPack};
use tetonic_secrets::ScannerEngine;
use std::sync::Arc;

fn empty_plan() -> SessionStartPlan {
    SessionStartPlan {
        data_class: DataClass::default(),
        disclosure_tier: DisclosureTier::default(),
        briefing: None,
        project_context: None,
        verify_cmd: None,
    }
}

struct FakeEventSink;
impl ApplicationEventSink for FakeEventSink {
    fn send(&self, _event: ApplicationEvent) {}
}

fn make_app() -> Application {
    let db_path = std::env::temp_dir().join(format!(
        "lokai_aud01_{}.db",
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
            crate::secret_scanner_factory::artifact_scan_policy(&None),
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

fn run_service_src() -> String {
    format!(
        "{}{}",
        include_str!("run_service.rs"),
        include_str!("turn_finalization.rs")
    )
}

fn source_order_from(haystack: &str, start_at: &str, earlier: &str, later: &str) {
    let start = haystack
        .find(start_at)
        .unwrap_or_else(|| panic!("missing start pin `{start_at}`"));
    let rest = &haystack[start..];
    let a = rest
        .find(earlier)
        .unwrap_or_else(|| panic!("missing earlier pin `{earlier}` after `{start_at}`"));
    let b = rest
        .find(later)
        .unwrap_or_else(|| panic!("missing later pin `{later}` after `{start_at}`"));
    assert!(
        a < b,
        "expected `{earlier}` before `{later}` after `{start_at}`"
    );
}

#[test]
fn aud01_bh_stream_tokens_map_without_attempt_id() {
    // PRESERVE: tokens stream to the portal. Missing Attempt id is INV-V4-OBS-001, fixed in OBS-02.
    let scanner = ScannerEngine::default_engine();
    let (rec, events) = RecordingEventSink::new();
    let sink: Arc<dyn ApplicationEventSink> = rec;
    step_to_events(
        &sink,
        "sess_stream",
        "a0",
        Step::Token("hi".into()),
        Some(&scanner),
        None,
        None,
    );
    let captured = events.lock().unwrap();
    match &captured[..] {
        [ApplicationEvent::ModelToken {
            session_id,
            agent_id,
            token,
            ..
        }] => {
            assert_eq!(session_id, "sess_stream");
            assert_eq!(agent_id, "a0");
            assert_eq!(token, "hi");
        }
        other => panic!("expected ModelToken, got {other:?}"),
    }
    let src = include_str!("events.rs");
    assert!(src.contains("struct ModelToken") || src.contains("ModelToken {"));
    let token_def = src
        .split("ModelToken {")
        .nth(1)
        .and_then(|s| s.split('}').next())
        .unwrap_or(src);
    assert!(
        token_def.contains("attempt_id"),
        "ModelToken now carries attempt_id (OBS-02)"
    );
}

#[test]
fn aud01_defect_bh_fin_strand_current() {
    // WORK-FIN-02 inverted the skip. Passing this does not establish INV-V4-FIN-*.
    let src = run_service_src();
    assert!(src.contains("if let Err(e) = self.heartbeat_active(attempt_id).await"));
    source_order_from(
        &src,
        "async fn finalize_attempt",
        "if let Err(e) = self.heartbeat_active(attempt_id).await",
        "finish_turn_run",
    );
    let body_start = src.find("async fn finalize_attempt").expect("finalize");
    let rest = &src[body_start..];
    assert!(
        rest.contains("pre_fail") && rest.contains("finish_turn_run"),
        "heartbeat failure still reaches finish_turn_run"
    );
}

#[test]
fn aud01_defect_bh_fin_drop_current() {
    // WORK-FIN-02 inverted drop-before-terminal. Passing this does not ESTABLISH FIN-*.
    let src = run_service_src();
    source_order_from(
        &src,
        "async fn finish_turn_run",
        ".get(attempt_id)",
        "RunCommand::CompleteAttempt",
    );
    source_order_from(
        &src,
        "async fn finish_turn_run",
        "submit_finish_run",
        ".remove(attempt_id)",
    );
    assert!(src.contains("TurnTerminal::Fail"));
}

#[tokio::test]
async fn aud01_defect_bh_id_session_current() {
    // baseline_defect: Session remains the production standing actor (WORK-02).
    // WORK-01 added identity records; that does not ESTABLISH INV-V4-ID-001.
    let src = run_service_src();
    assert!(src.contains("fn bind_live_run(&self, session_id: &str"));
    assert!(!src.contains("HashMap<IdentityId"));
    let live_src = include_str!("session_live.rs");
    assert!(live_src.contains("pub struct LiveSession"));
    assert!(!live_src.contains("AgentIdentity"));

    let runs = DefaultRunService::new(
        None,
        Arc::new(tetonic_policy::PolicyEngine::default()),
        Arc::new(FakeEventSink),
        crate::build_supervisor(None),
        Arc::new(crate::session_live::SessionLiveStore::new()),
        Arc::new(
            tetonic_artifact::LocalArtifactStore::new(
                std::env::temp_dir().join("aud01_id_art"),
                crate::secret_scanner_factory::artifact_scan_policy(&None),
            )
            .unwrap(),
        ),
    );
    assert!(
        runs.active.lock_recover().is_empty(),
        "Session/LiveSession remain the standing actor leftover"
    );
}

#[test]
fn aud01_defect_bh_fin_side_current() {
    // WORK-FIN-02 inverted the missing owner. Passing this does not ESTABLISH FIN-*.
    let run_src = run_service_src();
    assert!(run_src.contains("RecordSideEffectCommit"));
    let txn = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../core/lokai-transaction/src/service.rs"
    ));
    assert!(txn.contains("task_id: self.task_id.clone()"));
    assert!(txn.contains("attempt_id: self.attempt_id.clone()"));
}

#[test]
fn aud01_bh_debug_execute_spawn_door() {
    // AUD-PATH-004 production spawn door, distinct from in-loop spawn.
    // Direct Agent::turn / execute_spawn is not the destination executor (WORK-02).
    let src = include_str!("turn_execution.rs");
    assert!(src.contains("async fn execute_spawn"));
    source_order_from(
        src,
        "async fn execute_spawn",
        "register_spawn_task",
        "run_spawned_specialist",
    );
    assert!(
        !src.contains("orchestration_in_loop_spawn_agent"),
        "in-loop spawn is not this pin"
    );
    let pack = CodingPack;
    assert!(pack.parse("debugger").is_some());
    assert_eq!(pack.parse("debugger").unwrap(), RoleId::new("debugger"));
}

#[test]
fn aud01_bh_orch_spawn_execute_spawn_door() {
    // PRESERVE workflow door; executor owner DEFECT (WORK-02). Distinct from in-loop spawn.
    let src = include_str!("turn_execution.rs");
    assert!(src.contains("async fn execute_spawn"));
    assert!(src.contains("run_spawned_specialist"));
}

#[test]
fn aud01_bh_resp_explain_turn_skips_verify_commit_app_door() {
    // PRESERVE outcome via explain_turn / role overlay, not task_is_explain_only.
    let pack = CodingPack;
    let planner = RoleId::new("planner");
    assert!(pack.explain_turn(&planner, false));
    let overlay = pack.overlay(&planner);
    assert!(overlay.contains("Do NOT edit"));
    let tools = pack.allowed_tools(&planner).expect("planner read subset");
    assert!(tools.contains(&"search_code".into()));
    assert!(!tools
        .iter()
        .any(|t| t == "edit_file" || t == "write_file" || t == "run_shell"));
}

#[test]
fn aud01_bh_invest_read_search_no_required_mutation() {
    let pack = CodingPack;
    let planner = RoleId::new("planner");
    let tools = pack.allowed_tools(&planner).unwrap();
    assert!(tools.contains(&"read_file".into()));
    assert!(tools.contains(&"search_code".into()));
    assert!(tools.contains(&"grep".into()));
    assert!(!tools.iter().any(|t| t == "edit_file" || t == "write_file"));
}

#[test]
fn aud01_bh_plan_role_overlay_without_edit_tools() {
    let pack = CodingPack;
    let planner = RoleId::new("planner");
    assert!(pack.explain_turn(&planner, false));
    let overlay = pack.overlay(&planner);
    assert!(overlay.contains("plan"));
    assert!(overlay.contains("Do NOT edit"));
    let tools = pack.allowed_tools(&planner).unwrap();
    assert!(!tools.iter().any(|t| t == "edit_file" || t == "run_shell"));
}

#[test]
fn aud01_bh_edit_mode_coder_has_effectful_tools() {
    // PRESERVE edit mode. Must not assert BH-FIN-COMMIT as desired.
    let pack = CodingPack;
    let coder = RoleId::new("coder");
    assert!(
        pack.allowed_tools(&coder).is_none(),
        "coder is unconstrained (includes edit/write)"
    );
    let debugger = RoleId::new("debugger");
    let tools = pack.allowed_tools(&debugger).unwrap();
    assert!(tools.contains(&"edit_file".into()));
    assert!(tools.contains(&"write_file".into()));
}

#[test]
fn aud01_bh_debug_debugger_role_is_product_mode() {
    let pack = CodingPack;
    assert_eq!(pack.parse("debug").unwrap(), RoleId::new("debugger"));
    let overlay = pack.overlay(&RoleId::new("debugger"));
    assert!(overlay.to_lowercase().contains("debugger"));
}

#[test]
fn aud01_bh_review_critic_overlay_approve_revise() {
    let pack = CodingPack;
    let overlay = pack.overlay(&RoleId::new("critic"));
    assert!(overlay.contains("APPROVE") && overlay.contains("REVISE"));
    assert!(pack.explain_turn(&RoleId::new("critic"), false));
}

#[test]
fn aud01_bh_verify_ok_app_door_verify_cmd_field() {
    // App/daemon/agent door only. Eval is ABSENT, not PRESERVE of never-verify.
    let cmd = StartSessionCommand {
        verify_cmd: Some("cargo test".into()),
        ..Default::default()
    };
    assert_eq!(cmd.verify_cmd.as_deref(), Some("cargo test"));
}

#[tokio::test]
async fn aud01_bh_appr_allow_app_door_respond_continues() {
    let svc = DefaultApprovalService::new(None, Arc::new(FakeEventSink));
    let rx = svc.register_request(crate::commands::RegisterApprovalCommand {
        session_id: "s".into(), approval_id: "ap1".into(), call_id: "call_ap1".into(),
        kind: "run_shell".into(), detail: "echo".into(), tool: "run_shell".into(),
        args: serde_json::json!({}), missing_controls: vec![], user_approval_required: false,
        auto_grant_approvals: false, attempt_id: None,
    }).unwrap();
    let delivered = svc
        .respond(ApprovalResponseCommand {
            session_id: "s".into(),
            approval_id: "ap1".into(),
            approved: true,
            remember: false,
            kind: "run_shell".into(),
            detail: "echo".into(),
            channel_delivered: true,
            attempt_id: None,
        })
        .expect("allow");
    assert!(delivered, "allow continues work when channel delivered");
    assert_eq!(rx.await.unwrap(), true);
}

#[tokio::test]
async fn aud01_bh_appr_deny_app_door_respond_rejects() {
    let svc = DefaultApprovalService::new(None, Arc::new(FakeEventSink));
    let rx = svc.register_request(crate::commands::RegisterApprovalCommand {
        session_id: "s".into(), approval_id: "ap2".into(), call_id: "call_ap2".into(),
        kind: "run_shell".into(), detail: "echo".into(), tool: "run_shell".into(),
        args: serde_json::json!({}), missing_controls: vec![], user_approval_required: false,
        auto_grant_approvals: false, attempt_id: None,
    }).unwrap();
    let delivered = svc
        .respond(ApprovalResponseCommand {
            session_id: "s".into(),
            approval_id: "ap2".into(),
            approved: false,
            remember: false,
            kind: "run_shell".into(),
            detail: "echo".into(),
            channel_delivered: true,
            attempt_id: None,
        })
        .expect("deny");
    assert!(delivered);
    assert_eq!(rx.await.unwrap(), false);
}

#[test]
fn aud01_defect_bh_appr_loss_waiter_is_process_local() {
    // DEFECT: process-local oneshot, not durable Attempt-correlated.
    // Fail-closed proof expires PORTAL-01 VERIFY. Passing this does not establish IFACE.
    let live = LiveSession::new(
        Conversation::new(),
        tetonic_orchestrator::OrchestrationMode::Single,
        false,
        false,
        32,
        "fast".into(),
        "hard".into(),
        false,
        empty_plan(),
        std::env::temp_dir(),
        false,
        false,
        false,
    );
    let svc = DefaultApprovalService::new(None, Arc::new(FakeEventSink));
    let rx = svc
        .register_request(crate::commands::RegisterApprovalCommand {
            session_id: "s".into(),
            approval_id: "ap_loss".into(),
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
    assert!(!live.auto_grant_approvals);
    let delivered = svc
        .respond(ApprovalResponseCommand {
            session_id: "s".into(),
            approval_id: "ap_loss".into(),
            approved: true,
            remember: false,
            kind: "run_shell".into(),
            detail: "echo".into(),
            channel_delivered: true,
            attempt_id: None,
        })
        .expect("respond");
    assert!(delivered);
    assert!(rx.blocking_recv().unwrap());
}

#[tokio::test]
async fn aud01_bh_multi_chat_conversation_survives_turns() {
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
    let live = app.sessions.live(&started.session_id).unwrap();
    let _ = live.take_conversation().expect("lease");
    live.restore_conversation(Conversation::from_audit_messages(vec![
        tetonic_inference::Message::user("hello history"),
    ]));
    let again = live.take_conversation().expect("restore");
    assert_eq!(
        again.len(),
        1,
        "Conversation continuity is PRESERVE; Session-as-identity is BH-ID-SESSION"
    );
    live.restore_conversation(again);
}

#[tokio::test]
async fn aud01_bh_cancel_session_stops_in_flight_flag() {
    use std::sync::atomic::Ordering;
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
    let live = app.sessions.live(&started.session_id).unwrap();
    app.sessions
        .cancel_session(crate::commands::CancelRunCommand {
            session_id: started.session_id.clone(),
            pooled_cancel: false,
        })
        .await
        .expect("cancel");
    assert!(live.cancel.load(Ordering::Relaxed));
}

#[test]
fn aud01_bh_cmp_infer_never_constructs_agent_inventory() {
    // Negative control: worker Infer is not an agent job. Optional node file also pins this.
    let src = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../mantle/lokai-node/src/fabric_chat.rs"
    ));
    assert!(!src.contains("tetonic_core::Agent"));
    assert!(!src.contains("Agent::turn"));
    assert!(!src.contains("Agent::new"));
}

#[test]
fn aud01_defect_bh_cmp_fabric_app_bridge_submits_complete_attempt() {
    // WORK-FIN-02 deleted the submit door. Not INV-V4-CMP-001 established.
    let src = include_str!("fabric_run_bridge.rs");
    assert!(!src.contains("async fn apply_complete_attempt"));
    assert!(!src.contains("RunCommand::CompleteAttempt"));
}

#[test]
fn aud01_defect_bh_cmp_patch_app_door_commits() {
    // WORK-FIN-02 deleted the public door. Not INV-V4-CMP-001 established.
    let src = include_str!("lib.rs");
    assert!(!src.contains("pub fn apply_authorized_remote_patch"));
    assert!(!src.contains("apply_verified_remote_patch"));
}

//! CODE-03 IMPLEMENT source pins. Close `014`/`007`/`015`/`024` at CONVERGE only.

use std::fs;
use std::path::PathBuf;

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

fn run_service_src() -> String {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut src = fs::read_to_string(dir.join("run_service.rs")).expect("run_service.rs");
    src.push_str(&fs::read_to_string(dir.join("identity_job.rs")).expect("identity_job.rs"));
    src.push_str(
        &fs::read_to_string(dir.join("turn_finalization.rs")).expect("turn_finalization.rs"),
    );
    src
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

#[test]
fn code03_admit_child_has_no_add_dependency() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::ChildAdmission);
}

#[test]
fn code03_admit_child_starts_attempt_before_execute() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::ChildAdmission);
}

#[test]
fn code03_child_task_is_ready_not_blocked() {
    let src = crate_src("src/identity_job.rs");
    let body = fn_body(&src, "async fn start_child_attempt(");
    assert!(!body.contains("AddDependency"));
    let dag = crate_src("../../mantle/tetonic-run/src/dag.rs");
    assert!(dag.contains("dependencies_satisfied"));
}

#[test]
fn code03_empty_execute_spawn_is_root_job() {
    let src = run_service_src();
    let body = fn_body(&src, "async fn register_spawn_task(");
    assert!(body.contains("begin_job_run"));
    assert!(body.contains("start_child_attempt"));
    assert!(body.contains("find_parent_active"));
}

#[test]
fn code03_infer_hop_addtask_untouched() {
    let src = crate_src("../../mantle/tetonic-run/src/infer_admission.rs");
    assert!(src.contains("job_spec: None"));
    assert!(src.contains("AddTask") || src.contains("add_hop_task"));
}

#[test]
fn code03_turn_rs_has_no_leftover_turn() {
    let src = crate_src("../../mantle/tetonic-orchestrator/src/turn.rs");
    let prod = production_prefix(&src);
    assert!(!prod.contains(".turn("));
    assert!(prod.contains("child_job"));
    assert!(prod.contains("admit_child"));
    assert!(prod.contains("root_execute"));
}

#[test]
fn code03_root_still_app_root_execute() {
    let src = crate_src("src/turn_execution.rs");
    assert!(src.contains("impl RootExecute for AppRootExecute"));
    assert!(src.contains(".execute_attempt("));
}

#[test]
fn code03_root_execute_is_ref_self() {
    let src = crate_src("../../mantle/tetonic-orchestrator/src/turn.rs");
    let body = fn_body(&src, "pub trait RootExecute");
    assert!(body.contains("&self"));
    assert!(!body.contains("&mut self"));
}

#[test]
fn code03_approotexecute_delegates_to_manager_without_owning_attempt() {
    let src = crate_src("src/turn_execution.rs");
    let prod = production_prefix(&src);
    assert!(prod.contains("runs: Arc<dyn RunService>"));
    assert!(prod.contains(".execute_attempt("));
    assert!(!prod.contains("struct AppRootExecute {\n    attempt_id"));
}

#[test]
fn code03_spawn_host_has_no_refcell() {
    let src = crate_src("../../mantle/tetonic-orchestrator/src/spawn_host.rs");
    let prod = production_prefix(&src);
    assert!(prod.contains("child_job: Arc<dyn ChildJob>"));
    assert!(prod.contains("root_execute: Arc<dyn RootExecute>"));
    assert!(!prod.contains("RefCell<dyn RootExecute>"));
    assert!(!prod.contains("RefCell<dyn ChildJob>"));
}

#[test]
fn code03_spawn_host_no_leftover_turn() {
    let src = crate_src("../../mantle/tetonic-orchestrator/src/spawn_host.rs");
    let prod = production_prefix(&src);
    assert!(!prod.contains(".turn("));
    assert!(prod.contains("run_spawned_specialist"));
}

#[test]
fn code03_execute_spawn_still_turn_none() {
    let owned = crate_src("src/turn_execution.rs");
    let src = production_prefix(&owned);
    let body = fn_body(src, "async fn execute_spawn(");
    assert!(body.contains("spawn_hook: None"));
}

#[test]
fn code03_daemon_agent_spawn_rpc_untouched() {
    let src = crate_src("../../litho/tetonicd/src/daemon/handlers/agent.rs");
    assert!(src.contains("fn agent_spawn"));
    assert!(src.contains("submit_spawn"));
    assert!(!src.contains("execute_spawn"));
    assert!(!src.contains("CodingPack"));
}

#[test]
fn code03_llm_route_stamps_parent_fabric() {
    let owned = crate_src("src/turn_execution.rs");
    let src = production_prefix(&owned);
    let body = fn_body(src, "async fn run_turn_body(");
    assert!(body.contains("FabricCallMeta"));
    assert!(body.contains("turn_plan.run_id"));
    assert!(body.contains("turn_plan.task_id"));
    assert!(body.contains("turn_plan.attempt_id"));
}

#[test]
fn code03_llm_route_has_no_fabric_none() {
    let src = crate_src("../../mantle/tetonic-orchestrator/src/router_llm.rs");
    let body = fn_body(&src, "pub async fn llm_route_task(");
    assert!(!body.contains("fabric: None"));
    assert!(body.contains("fabric: Some(fabric)"));
}

#[test]
fn code03_chat_request_fallback_untouched() {
    let src = crate_src("../../mantle/tetonic-broker/src/chat_request.rs");
    let body = fn_body(&src, "pub fn compute_request_from_chat(");
    assert!(body.contains("run_{}"));
    assert!(body.contains("att_{}"));
    assert!(body.contains("Uuid::new_v4"));
    assert!(!body.contains("m.run_id"));
    assert!(!body.contains("m.task_id"));
    assert!(!body.contains("m.attempt_id"));
    assert!(!body.contains("session_id"));
}

#[test]
fn code03_router_submits_no_run_command() {
    let src = crate_src("../../mantle/tetonic-orchestrator/src/router_llm.rs");
    let body = fn_body(&src, "pub async fn llm_route_task(");
    assert!(!body.contains("RunCommand"));
    assert!(!body.contains("Agent::"));
}

#[test]
fn code03_spawn_local_still_live() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::ProductDispatch);
}

#[test]
fn code03_take_restore_still_live() {
    let src = crate_src("src/session_live.rs");
    assert!(src.contains("take_conversation"));
    assert!(src.contains("restore_conversation"));
}

#[test]
fn code03_run_turn_symbol_remains() {
    let src = crate_src("src/run_service.rs");
    assert!(src.contains("async fn run_turn("));
}

#[test]
fn code03_start_identity_job_still_none() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Sessionless);
}

#[test]
fn code03_start_identity_job_still_finish_run_true() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Sessionless);
}

#[test]
fn code03_create_run_still_persist_only() {
    let src = run_service_src();
    let body = fn_body(&src, "async fn create_run(");
    assert!(body.contains("RunCommand::CreateRun"));
    assert!(!body.contains("StartRun"));
    assert!(!body.contains("StartAttempt"));
    assert!(!body.contains("LocalAgentAttemptExecutor"));
}

#[test]
fn code03_execute_spawn_keeps_inner_audit() {
    let owned = crate_src("src/turn_execution.rs");
    let src = production_prefix(&owned);
    let body = fn_body(src, "async fn execute_spawn(");
    assert!(body.contains("build_agent("));
    assert!(body.contains("None,"));
}

#[test]
fn code03_finish_turn_run_has_finish_run_branch() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::FinalizationOrder);
}

#[test]
fn code03_child_cancel_is_fail_attempt_not_finish_run() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Cancellation);
}

#[test]
fn code03_complete_child_does_not_finish_run() {
    let src = crate_src("src/identity_job.rs");
    let body = fn_body(&src, "async fn complete_child_job(");
    assert!(body
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .contains("None,false"));
}

#[test]
fn code03_critic_revision_spawn_use_root_execute() {
    let src = crate_src("../../mantle/tetonic-orchestrator/src/turn.rs");
    let prod = production_prefix(&src);
    assert!(prod.contains("root_execute"));
    assert!(prod.contains("admit_child"));
    assert!(prod.contains("complete_child"));
}

#[test]
fn code03_post_execution_register_gone_or_noop() {
    let owned = crate_src("src/turn_execution.rs");
    let src = production_prefix(&owned);
    let body = fn_body(src, "async fn run_turn_body(");
    assert!(!body.contains("register_spawn_task"));
}

#[test]
fn code03_work001_not_established_portals_remain() {
    let product = crate_src("src/turn_execution.rs");
    assert!(product.contains("async fn execute_spawn("));
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tooling/tetonic-arch-gate/fixtures/v4/ARCH-V4-WORK-001.v4fix");
    assert!(fixture.is_file());
}

#[test]
fn code03_cmp001_not_established() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tooling/tetonic-arch-gate/fixtures/v4/ARCH-V4-CMP-001.v4fix");
    assert!(fixture.is_file());
}

#[test]
fn code03_arch_work_001_still_planted() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tooling/tetonic-arch-gate/fixtures/v4/ARCH-V4-WORK-001.v4fix");
    assert!(fixture.is_file());
}

#[test]
fn code03_arch_cmp_001_still_planted() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tooling/tetonic-arch-gate/fixtures/v4/ARCH-V4-CMP-001.v4fix");
    assert!(fixture.is_file());
}

#[test]
fn code03_bh_id_session_still_defect() {
    let live = crate_src("src/session_live.rs");
    assert!(live.contains("pub struct LiveSession"));
}

#[test]
fn code03_kernel_spawn_agent_name_check_remains() {
    let src = crate_src("../../mantle/tetonic-orchestrator/src/spawn.rs");
    assert!(src.contains("pub fn spawn_agent_tool_name"));
}

#[test]
fn code03_orchestrator_cargo_still_lists_tools() {
    let src = crate_src("../../mantle/tetonic-orchestrator/Cargo.toml");
    assert!(src.contains("tetonic-tools") || src.contains("lokai-tools"));
    assert!(src.contains("tetonic-core") || src.contains("lokai-core"));
}

#[test]
fn code03_eval_kernel_untouched() {
    let src = crate_src("../../tooling/tetonic-eval/src/kernel.rs");
    assert!(src.contains("verify_cmd: None"));
    assert!(!src.contains("struct StoreAudit"));
    assert!(!src.contains("struct AutoGrant"));
}

#[path = "support/lifecycle_contract.rs"]
mod lifecycle_contract;

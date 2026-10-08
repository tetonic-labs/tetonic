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
fn code03_root_execute_is_ref_self() {
    let src = crate_src("../../mantle/tetonic-orchestrator/src/turn.rs");
    let body = fn_body(&src, "pub trait RootExecute");
    assert!(body.contains("&self"));
    assert!(!body.contains("&mut self"));
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
fn code03_start_identity_job_still_none() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Sessionless);
}

#[test]
fn code03_start_identity_job_still_finish_run_true() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Sessionless);
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
fn code03_critic_revision_spawn_use_root_execute() {
    let src = crate_src("../../mantle/tetonic-orchestrator/src/turn.rs");
    let prod = production_prefix(&src);
    assert!(prod.contains("root_execute"));
    assert!(prod.contains("admit_child"));
    assert!(prod.contains("complete_child"));
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

#[path = "support/lifecycle_contract.rs"]
mod lifecycle_contract;

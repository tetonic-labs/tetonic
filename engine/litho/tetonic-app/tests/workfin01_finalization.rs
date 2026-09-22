//! WORK-FIN-01 S6/S7 pins. Close `016`/`026` at CONVERGE only. No ESTABLISH.

use std::fs;
use std::path::{Path, PathBuf};

fn crate_src(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    let mut src = fs::read_to_string(&path).expect(rel);
    if rel == "src/run_service.rs" {
        src.push_str(
            &fs::read_to_string(path.with_file_name("turn_finalization.rs"))
                .expect("turn_finalization.rs"),
        );
    }
    src
}

fn production_prefix(src: &str) -> &str {
    src.split("#[cfg(test)]").next().unwrap_or(src)
}

fn fn_body<'a>(src: &'a str, sig: &str) -> &'a str {
    let start = src.find(sig).expect(sig);
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
    &after[brace..]
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
                continue;
            }
            if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                if let Ok(src) = fs::read_to_string(&path) {
                    visit(&path, &src);
                }
            }
        }
    }
}

#[test]
fn workfin01_try_claim_finalization_does_not_call_apply_winner_selection() {
    let src = crate_src("../../mantle/tetonic-run/src/acceptance.rs");
    let body = fn_body(&src, "pub fn try_claim_finalization");
    assert!(!body.contains("apply_winner_selection"));
}

#[test]
fn workfin01_finalize_claims_before_commit() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::FinalizationOrder);
}

#[test]
fn workfin01_finalize_response_only_skips_verify_and_commit() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::SideEffects);
}

#[test]
fn workfin01_finalize_canceled_skips_claim() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::ClaimFence);
}

#[test]
fn workfin01_finish_order_still_complete_accept_finish() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::FinalizationOrder);
}

#[test]
fn workfin01_coding_build_agent_does_not_bind_completion_check() {
    let owned = crate_src("src/turn_execution.rs");
    let src = production_prefix(&owned);
    let body = fn_body(src, "fn build_agent(");
    assert!(!body.contains("make_completion_check"));
    assert!(!body.contains("with_completion_check"));
    assert!(!body.contains("Tools::new"));
    assert!(body.contains("with_orchestration(build.orchestration_tools)"));
}

#[test]
fn workfin01_build_agent_does_not_construct_tools() {
    let owned = crate_src("src/turn_execution.rs");
    let src = production_prefix(&owned);
    let body = fn_body(src, "fn build_agent(");
    assert!(!body.contains("runtime.build_tools"));
    assert!(body.contains("ensure_turn_tools") || body.contains("turn_tools"));
}

#[test]
fn workfin01_complete_turn_does_not_read_session_rows() {
    let src = crate_src("src/run_service.rs");
    let start = src
        .rfind("async fn complete_turn(")
        .expect("complete_turn impl");
    let body = fn_body(&src[start..], "async fn complete_turn(");
    assert!(!body.contains("attest_turn"));
    assert!(!body.contains("attest_active_turn"));
    assert!(!body.contains("session_file_changes"));
    assert!(!body.contains("last_assistant"));
    assert!(body.contains("finalize_attempt"));
}

#[test]
fn workfin01_session_attest_helpers_are_gone() {
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../mantle/tetonic-run/src/managed/attestation.rs"),
    )
    .expect("manager attestation source");
    let prod = production_prefix(&src);
    assert!(!prod.contains("fn attest_turn"));
    assert!(!prod.contains("fn collect_success_bytes"));
    assert!(!prod.contains("fn last_assistant"));
    assert!(!prod.contains("session_file_changes"));
    assert!(prod.contains("fn encode_candidate_bytes"));
}

#[test]
fn workfin01_complete_turn_seals_candidate_bytes() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::FinalizationOrder);
}

#[test]
fn workfin01_orchestrated_outcome_is_candidate() {
    let src = crate_src("../../mantle/tetonic-orchestrator/src/turn.rs");
    assert!(src.contains("pub outcome: CandidateOutcome"));
    let body = fn_body(&src, "pub async fn run_orchestrated_turn");
    assert!(body.contains("outcome: terminal"));
    assert!(!body.contains("return Err(message)"));
}

#[test]
fn workfin01_terminal_outcome_is_revision_when_revision_ran() {
    let src = crate_src("../../mantle/tetonic-orchestrator/src/turn.rs");
    let body = fn_body(&src, "pub async fn run_orchestrated_turn");
    assert!(body.contains("revision_ran = true"));
    assert!(body.contains("terminal = root_execute"));
}

#[test]
fn workfin01_no_coordinator_commit_before_winner() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    walk_production_rs(&root, &mut |path, src| {
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name.ends_with("_tests.rs") {
            return;
        }
        assert!(
            !src.contains("make_completion_check"),
            "{path:?} still has make_completion_check"
        );
        assert!(
            !path.ends_with("completion_coordinator.rs"),
            "completion_coordinator.rs must be gone"
        );
    });
    assert!(!PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/completion_coordinator.rs")
        .exists());
}

#[test]
fn workfin01_commit_site_is_finalize_after_claim() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::FinalizationOrder);
}

#[test]
fn workfin01_sessionless_has_no_turn_tools() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Sessionless);
}

#[test]
fn workfin01_sessionless_claims_without_session() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Sessionless);
}

#[test]
fn workfin01_explain_root_skips_verify_after_revision() {
    let owned = crate_src("src/turn_execution.rs");
    let src = production_prefix(&owned);
    assert!(src.contains("CodingPack.root_explain_turn"));
    assert!(src.contains("RouteMode::Specialist"));
    assert!(!fn_body(src, "async fn execute_turn(").contains("build.explain_turn"));
}

#[test]
fn workfin01_planner_root_skips_verify_when_text_is_not_explain() {
    let owned = crate_src("src/turn_execution.rs");
    let src = production_prefix(&owned);
    assert!(src.contains("CodingPack.explain_turn(r, false)"));
}

#[test]
fn workfin01_revision_clone_does_not_advertise_spawn_agent() {
    let owned = crate_src("src/turn_execution.rs");
    let src = production_prefix(&owned);
    let ensure = fn_body(src, "fn ensure_turn_tools(");
    assert!(!ensure.contains("with_orchestration"));
    let build = fn_body(src, "fn build_agent(");
    assert!(build.contains("with_orchestration(build.orchestration_tools)"));
}

#[test]
fn workfin01_root_auto_clone_advertises_spawn_agent() {
    let owned = crate_src("src/turn_execution.rs");
    let src = production_prefix(&owned);
    assert!(src.contains("with_orchestration(build.orchestration_tools)"));
}

#[test]
fn workfin01_fin003_not_established_fabric_complete_remains() {
    // WORK-FIN-02 deleted fabric CompleteAttempt submit. FIN-003 stays unestablished.
    let src = crate_src("src/fabric_run_bridge.rs");
    assert!(!src.contains("apply_complete_attempt"));
    let turn = crate_src("../../mantle/tetonic-orchestrator/src/turn.rs");
    assert!(turn.contains("child_job"));
    assert!(turn.contains("complete_child"));
}

#[test]
fn workfin01_app002_not_established_run_turn_remains() {
    let owned = crate_src("src/turn_execution.rs");
    let src = production_prefix(&owned);
    assert!(src.contains("async fn execute_turn"));
    let run = crate_src("src/run_service.rs");
    assert!(run.contains("async fn run_turn("));
}

#[test]
fn workfin01_id001_not_established_session_chat_remains() {
    let live = crate_src("src/session_live.rs");
    assert!(live.contains("pub struct LiveSession"));
}

#[test]
fn workfin01_fin_skip_not_inverted() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Failures);
}

#[test]
fn workfin01_arch_fin_001_still_planted() {
    let fix = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tooling/tetonic-arch-gate/fixtures/v4/ARCH-V4-FIN-001.v4fix");
    assert!(fix.exists(), "ARCH-V4-FIN-001.v4fix must stay planted");
    let corpus = crate_src("../../tooling/tetonic-arch-gate/src/v4_corpus.rs");
    assert!(corpus.contains("ARCH-V4-FIN-001.v4fix"));
}

#[test]
fn workfin01_leftover_turn_remains() {
    let src = crate_src("../../mantle/tetonic-orchestrator/src/turn.rs");
    let prod = src.split("#[cfg(test)]").next().unwrap_or(&src);
    assert!(!prod.contains(".turn("));
    assert!(prod.contains("run_spawned_specialist"));
    assert!(prod.contains("admit_child"));
}

#[test]
fn workfin01_failed_reaches_finalize_as_candidate() {
    let src = crate_src("../../mantle/tetonic-orchestrator/src/turn.rs");
    let body = fn_body(&src, "pub async fn run_orchestrated_turn");
    assert!(body.contains("outcome: terminal"));
    assert!(!body.contains("if let CandidateOutcome::Failed { message } = result"));
}

#[path = "support/lifecycle_contract.rs"]
mod lifecycle_contract;

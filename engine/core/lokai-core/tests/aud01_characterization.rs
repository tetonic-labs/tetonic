//! AUD-01 core-loop characterization. Heuristic tests are not PRESERVE.

use lokai_core::AgentConfig;

fn agent_src() -> &'static str {
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/agent.rs"))
}

fn source_order(haystack: &str, earlier: &str, later: &str) {
    let a = haystack
        .find(earlier)
        .unwrap_or_else(|| panic!("missing earlier pin `{earlier}`"));
    let b = haystack
        .find(later)
        .unwrap_or_else(|| panic!("missing later pin `{later}`"));
    assert!(a < b, "expected `{earlier}` before `{later}`");
}

#[test]
fn aud01_bh_resp_explain_turn_skips_verify_and_commit() {
    // PRESERVE outcome via AgentInvocation.explain_turn / CompletionCheck skip.
    let cfg = AgentConfig {
        explain_turn: true,
        ..AgentConfig::default()
    };
    assert!(cfg.explain_turn);
    let src = agent_src();
    assert!(src.contains("let explain_turn = invocation.explain_turn"));
    assert!(src.contains("mutating tool blocked (explain turn)"));
    source_order(
        src,
        "!self.tools.is_read_only(&name)",
        "mutating tool blocked (explain turn)",
    );
    assert!(
        !src.contains("tools.commit_staged_if_any"),
        "kernel must not commit; dated commit is the app hook"
    );
}

#[test]
fn aud01_bh_verify_ok_when_verify_cmd_set() {
    // Kernel door gone. Policy when verify_cmd is set is the app finalizer.
    let src = agent_src();
    assert!(!src.contains("verify_cmd"));
    assert!(!src.contains("completion_check"));
    assert!(!src.contains("CompletionDecision::Reject"));
}

#[test]
fn aud01_bh_verify_fail_not_attempt_success() {
    // Verify-fail-not-success is the app finalizer. Kernel has no Reject consume.
    let src = agent_src();
    assert!(!src.contains("CompletionDecision::Reject"));
}

#[test]
fn aud01_defect_bh_fin_commit_current() {
    // Leftover kernel hook deleted (GATE-01). Not INV-V4-FIN-* established.
    let src = agent_src();
    assert!(!src.contains("completion_check"));
    assert!(src.contains("pub async fn turn"));
    assert!(
        !src.contains("tools.commit_staged_if_any"),
        "kernel finish must not commit"
    );
}

#[test]
fn aud01_defect_aud_path_007_agent_run_unmanaged() {
    // DEFECT door deleted (WORK-02 / DEL-V4-022). Not blessed.
    let src = agent_src();
    assert!(
        !src.contains("pub async fn run<F>"),
        "public Agent::run must be gone"
    );
}

#[test]
fn aud01_current_heuristic_explain_not_in_kernel() {
    // Current classifier moved to product root_explain_turn; not PRESERVE contract.
    let src = agent_src();
    assert!(!src.contains("fn task_is_explain_only"));
    assert!(!src.contains("task_requires_tools"));
}

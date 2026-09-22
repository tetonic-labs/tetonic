//! Post-edit critic (D12) — read-only review pass after mutating work.

use crate::run::TurnTracker;
use crate::specialist::{RoleId, SpecialistPack};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CriticOutcome {
    Approved,
    Revise(String),
    Skipped,
}

/// Parse a critic `finish` summary into approve vs revise.
pub fn parse_critic_verdict(summary: &str) -> CriticOutcome {
    let s = summary.trim();
    // The protocol permits one exact approval token. Prose, negations, empty
    // responses, and conflicting verdicts must never authorize approval.
    if s.eq_ignore_ascii_case("APPROVE") {
        return CriticOutcome::Approved;
    }
    if let Some((prefix, issues)) = s.split_once(':') {
        if prefix.trim().eq_ignore_ascii_case("REVISE") && !issues.trim().is_empty() {
            return CriticOutcome::Revise(issues.trim().to_string());
        }
    }
    CriticOutcome::Revise(if s.is_empty() {
        "Critic returned no verdict; review required".into()
    } else {
        s.to_string()
    })
}

pub fn critic_user_prompt(edited_tools: &[String]) -> String {
    format!(
        "Review the edits made this turn (tools: {}). Read the affected files. \
Do not modify anything. Respond via `finish` with exactly `APPROVE` and no other text, or `REVISE: <issues>`.",
        if edited_tools.is_empty() {
            "(unknown)".into()
        } else {
            edited_tools.join(", ")
        }
    )
}

pub fn should_run_critic(
    pack: &dyn SpecialistPack,
    role: &RoleId,
    mutating_edits: usize,
    critic_enabled: bool,
) -> bool {
    if !critic_enabled || mutating_edits == 0 {
        return false;
    }
    pack.should_run_critic(role)
}

/// D12 v3/v4: run critic when LSP reported diagnostics, or when verify failed and
/// the session configured a verify-before-finish command (`verify_gated`).
pub fn should_run_critic_enhanced(
    pack: &dyn SpecialistPack,
    role: &RoleId,
    tracker: &TurnTracker,
    verify_gated: bool,
    critic_enabled: bool,
) -> bool {
    if !should_run_critic(pack, role, tracker.mutating_edits, critic_enabled) {
        return false;
    }
    if tracker.lsp_diagnostic_issues > 0 {
        return true;
    }
    verify_gated && tracker.verify_ever_failed
}

/// Build critic prompt from turn telemetry (edited tools, verify, LSP).
pub fn critic_prompt_from_tracker(tracker: &TurnTracker) -> String {
    let tools = if tracker.mutating_tools.is_empty() {
        vec!["edit_file".into(), "write_file".into()]
    } else {
        tracker.mutating_tools.clone()
    };
    let mut prompt = critic_user_prompt(&tools);
    if tracker.lsp_diagnostic_snippets.is_empty() {
        if tracker.lsp_calls > 0 {
            prompt.push_str(&format!(
                "\n\nThe specialist used LSP tools ({} call(s)); weigh type/lint issues in your review.",
                tracker.lsp_calls
            ));
        }
    } else {
        prompt.push_str("\n\nLSP diagnostics reported during the turn:\n");
        for (i, snippet) in tracker.lsp_diagnostic_snippets.iter().take(5).enumerate() {
            prompt.push_str(&format!("{}. {}\n", i + 1, snippet));
        }
    }
    match tracker.verify_passed {
        Some(true) => prompt.push_str(
            "\n\nVerification passed before finish; focus on style and edge cases rather than build/test failures.",
        ),
        Some(false) => prompt.push_str(
            "\n\nVerification did not pass during the turn — the work may be incomplete.",
        ),
        None => {}
    }
    prompt
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::specialist::{RoleId, TestCodingPack};

    fn pack() -> TestCodingPack {
        TestCodingPack
    }

    fn coder() -> RoleId {
        RoleId::new("coder")
    }

    #[test]
    fn parses_approve_and_revise() {
        assert_eq!(parse_critic_verdict("APPROVE"), CriticOutcome::Approved);
        assert!(matches!(
            parse_critic_verdict("REVISE: missing edge case for empty input"),
            CriticOutcome::Revise(_)
        ));
    }

    #[test]
    fn rejects_negated_conflicting_empty_and_short_verdicts() {
        for summary in [
            "",
            "FAIL",
            "BUG",
            "SYNTAX ERROR",
            "LGTM",
            "looks good",
            "REVISE: do not say LGTM; critical SQL injection",
            "APPROVE but tests fail",
            "APPROVED",
            "APPROVE\nREVISE: unsafe",
            "Do not APPROVE",
            "REVISE:",
        ] {
            assert!(
                matches!(parse_critic_verdict(summary), CriticOutcome::Revise(_)),
                "{summary}"
            );
        }
        assert_eq!(parse_critic_verdict("  approve\n"), CriticOutcome::Approved);
        assert_eq!(
            parse_critic_verdict("REVISE: Preserve Case"),
            CriticOutcome::Revise("Preserve Case".into())
        );
    }

    #[test]
    fn should_run_critic_rules() {
        let p = pack();
        assert!(!should_run_critic(&p, &coder(), 0, true));
        assert!(!should_run_critic(&p, &coder(), 1, false));
        assert!(!should_run_critic(&p, &RoleId::new("planner"), 2, true));
        assert!(should_run_critic(&p, &coder(), 1, true));
        assert!(should_run_critic(&p, &RoleId::new("debugger"), 1, true));
    }

    #[test]
    fn critic_prompt_lists_edited_tools() {
        let p = critic_user_prompt(&["edit_file".into()]);
        assert!(p.contains("edit_file"));
        assert!(p.contains("APPROVE"));
    }

    #[test]
    fn enhanced_critic_skips_when_clean() {
        let mut t = TurnTracker::default();
        t.mutating_edits = 2;
        t.verify_passed = Some(true);
        assert!(!should_run_critic_enhanced(
            &pack(),
            &coder(),
            &t,
            true,
            true
        ));
    }

    #[test]
    fn enhanced_critic_runs_after_verify_fail_or_lsp() {
        let mut t = TurnTracker::default();
        t.mutating_edits = 1;
        t.verify_ever_failed = true;
        assert!(should_run_critic_enhanced(
            &pack(),
            &coder(),
            &t,
            true,
            true
        ));
        t.verify_ever_failed = false;
        t.verify_passed = Some(true);
        t.lsp_diagnostic_issues = 2;
        assert!(should_run_critic_enhanced(
            &pack(),
            &coder(),
            &t,
            true,
            true
        ));
    }

    #[test]
    fn enhanced_critic_skips_verify_fail_when_not_gated() {
        let mut t = TurnTracker::default();
        t.mutating_edits = 1;
        t.verify_ever_failed = true;
        assert!(!should_run_critic_enhanced(
            &pack(),
            &coder(),
            &t,
            false,
            true
        ));
    }

    #[test]
    fn enhanced_critic_runs_on_lsp_without_verify_gated() {
        let mut t = TurnTracker::default();
        t.mutating_edits = 1;
        t.lsp_diagnostic_issues = 1;
        assert!(should_run_critic_enhanced(
            &pack(),
            &coder(),
            &t,
            false,
            true
        ));
    }

    #[test]
    fn critic_prompt_includes_lsp_snippets() {
        let mut t = TurnTracker::default();
        t.mutating_edits = 1;
        t.mutating_tools.push("edit_file".into());
        t.lsp_diagnostic_snippets
            .push("3 diagnostic(s) in src/lib.rs".into());
        let p = critic_prompt_from_tracker(&t);
        assert!(p.contains("src/lib.rs"));
    }

    #[test]
    fn critic_prompt_from_tracker_includes_lsp() {
        let mut t = TurnTracker::default();
        t.mutating_tools.push("write_file".into());
        t.lsp_calls = 2;
        t.verify_passed = Some(true);
        let p = critic_prompt_from_tracker(&t);
        assert!(p.contains("write_file"));
        assert!(p.contains("LSP"));
    }
}

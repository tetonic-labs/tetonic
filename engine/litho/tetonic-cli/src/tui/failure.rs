//! Human-readable turn failure and capacity-warning copy.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnFailureCopy {
    pub headline: &'static str,
    pub summary: String,
    pub hint: Option<String>,
}

/// Employee-visible failure text. A persistence, tool, or internal body is not repeated.
pub fn visible_failure(raw: &str) -> String {
    let trimmed = raw.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("persistence failed:")
        || lower.starts_with("tool execution failed:")
        || lower.starts_with("internal invariant violation:")
    {
        return "request failed".into();
    }
    trimmed.to_string()
}

pub fn explain_turn_failure(raw: &str) -> TurnFailureCopy {
    let raw = visible_failure(raw);
    let lower = raw.to_ascii_lowercase();
    if (lower.contains("capability stale") && lower.contains("node_local"))
        || lower.contains("worker capabilities not cached")
    {
        return TurnFailureCopy {
            headline: "Turn failed — no model reply.",
            summary: "An enrolled worker is unreachable, and local fallback was blocked before Ollama ran.".into(),
            hint: Some(
                "Check workers with `tetonic estate status`. Remove a dead node with `tetonic estate worker remove <label>`."
                    .into(),
            ),
        };
    }
    if lower.contains("fabric connect timed out") || lower.contains("no eligible remote") {
        return TurnFailureCopy {
            headline: "Turn failed — worker unreachable.",
            summary: "An enrolled fabric worker did not respond in time.".into(),
            hint: Some(
                "Local Ollama should still run. If this repeats, `tetonic estate status` and remove the down worker."
                    .into(),
            ),
        };
    }
    if lower.contains("gpu spill") {
        return TurnFailureCopy {
            headline: "Turn failed — model too large for GPU.",
            summary: "The loaded model spilled out of VRAM, so inference was aborted.".into(),
            hint: Some(
                "Run `/doctor` and pick a smaller model, or free VRAM with `/evict`.".into(),
            ),
        };
    }
    if lower.contains("hop lease") && lower.contains("run not found") {
        return TurnFailureCopy {
            headline: "Turn failed — the run journal missed this hop.",
            summary: "Local inference never started because the hop lease could not find this turn's run.".into(),
            hint: Some("Rebuild this CLI (`cargo run -p tetonic-cli`) — hop lease now creates a missing run instead of aborting.".into()),
        };
    }
    if lower.contains("egress") {
        return TurnFailureCopy {
            headline: "Turn failed — network policy denied the call.",
            summary: sanitize_error(&raw),
            hint: Some("Use `/egress` to inspect allow rules. Loopback Ollama should not need extra rules.".into()),
        };
    }
    TurnFailureCopy {
        headline: "Turn failed.",
        summary: sanitize_error(&raw),
        hint: None,
    }
}

pub fn explain_capacity_warning(raw: &str) -> TurnFailureCopy {
    TurnFailureCopy {
        headline: "Capacity warning — chat continues.",
        summary: sanitize_error(raw),
        hint: Some(
            "Inference will abort if this model spills VRAM. Run `/doctor` or free VRAM with `/evict`."
                .into(),
        ),
    }
}

pub fn inspector_failure_text(raw: &str) -> String {
    let raw = visible_failure(raw);
    let copy = explain_turn_failure(&raw);
    let mut out = String::from("Turn failed\n\n");
    out.push_str(&copy.summary);
    out.push('\n');
    if let Some(hint) = &copy.hint {
        out.push('\n');
        out.push_str(hint);
        out.push('\n');
    }
    out.push_str("\nTechnical detail\n");
    out.push_str(raw.trim());
    out.push('\n');
    out
}

pub(crate) fn sanitize_error(raw: &str) -> String {
    raw.trim()
        .trim_start_matches("Invalid request: ")
        .trim_start_matches("provider: ")
        .trim()
        .to_string()
}

pub fn is_same_failure(prev: &str, next: &str) -> bool {
    let a = sanitize_error(prev);
    let b = sanitize_error(next);
    !a.is_empty() && !b.is_empty() && (a == b || a.contains(&b) || b.contains(&a))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persistence_and_tool_bodies_are_not_shown() {
        for raw in [
            "Persistence failed: sqlite: PRIVATECANARY",
            "Tool execution failed: stdout PRIVATECANARY",
            "Internal invariant violation: PRIVATECANARY",
        ] {
            let copy = explain_turn_failure(raw);
            let shown = inspector_failure_text(raw);
            assert_eq!(copy.summary, "request failed");
            assert!(!shown.contains("PRIVATECANARY"), "{shown}");
            assert!(shown.contains("request failed"), "{shown}");
        }
        assert!(inspector_failure_text("Invalid request: unknown session_id")
            .contains("unknown session_id"));
    }

    #[test]
    fn execution_failure_does_not_claim_the_model_never_replied() {
        let copy = explain_turn_failure("effort cap reached (8 steps)");
        assert_eq!(copy.headline, "Turn failed.");
        assert_eq!(copy.summary, "effort cap reached (8 steps)");
    }

    #[test]
    fn stale_local_capability_is_explained() {
        let copy = explain_turn_failure(
            "provider: capability stale for worker node_local: worker capabilities not cached",
        );
        assert_eq!(copy.headline, "Turn failed — no model reply.");
        assert!(copy.summary.contains("enrolled worker"));
        assert!(copy.hint.unwrap().contains("estate status"));
    }

    #[test]
    fn wrapped_app_error_is_the_same_failure() {
        assert!(is_same_failure(
            "provider: capability stale for worker node_local: worker capabilities not cached",
            "Invalid request: provider: capability stale for worker node_local: worker capabilities not cached",
        ));
    }

    #[test]
    fn hop_lease_missing_run_is_explained() {
        let copy = explain_turn_failure(
            "provider: RunSupervisor snapshot for hop lease: run not found: run_abc",
        );
        assert!(copy.headline.contains("run journal"));
        assert!(copy.summary.contains("hop lease"));
    }

    #[test]
    fn capacity_warn_and_proceed_reuses_failure_tone() {
        let copy = explain_capacity_warning(
            "capacity: saved profile for `qwen` is degraded. Chat continues; inference will abort if this model spills VRAM.",
        );
        assert!(copy.headline.contains("chat continues"));
        assert!(copy.summary.contains("spills VRAM"));
        assert!(copy.hint.unwrap().contains("/doctor"));
    }
}

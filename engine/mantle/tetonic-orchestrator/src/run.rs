//! Orchestrated turn helpers (D11/D12).

use tetonic_core::{AgentConfig, Step};

use crate::critic::{parse_critic_verdict, CriticOutcome};
use crate::router::{RouteDecision, RouteMode};
use crate::specialist::{RoleId, SpecialistPack};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrchestrationMode {
    Single,
    Auto,
}

impl OrchestrationMode {
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "auto" | "swarm" | "orchestrate" => Self::Auto,
            _ => Self::Single,
        }
    }
}

/// Per-turn spawn budget for in-loop `spawn_agent` (A13 v4).
#[derive(Debug, Clone, Copy)]
pub struct SpawnLimits {
    /// Max `spawn_agent` tool calls per user turn.
    pub max_per_turn: u32,
    /// Max nesting depth (`a0` = 0, `a0_s0` = 1, …).
    pub max_depth: u32,
}

impl Default for SpawnLimits {
    fn default() -> Self {
        Self {
            max_per_turn: 4,
            max_depth: 2,
        }
    }
}

impl SpawnLimits {
    /// Read spawn budget from the environment. **Binaries only** — call once at
    /// startup and pass the result through orchestration context; libraries must
    /// not read `std::env` directly.
    pub fn from_env() -> Self {
        let max_per_turn = std::env::var("LOKAI_MAX_SPAWN_PER_TURN")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(4);
        let max_depth = std::env::var("LOKAI_MAX_SPAWN_DEPTH")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2);
        Self {
            max_per_turn: max_per_turn.max(1),
            max_depth,
        }
    }
}

/// Count `_s` segments in an agent id (root `a0` → 0).
pub fn spawn_depth(agent_id: &str) -> u32 {
    agent_id.matches("_s").count() as u32
}

/// Track mutating edits, verify outcome, and LSP usage from step events.
#[derive(Debug, Default)]
pub struct TurnTracker {
    pub mutating_edits: usize,
    pub mutating_tools: Vec<String>,
    pub last_finish_summary: Option<String>,
    /// `Some(true)` when verify-before-finish passed; `Some(false)` on failure.
    pub verify_passed: Option<bool>,
    pub lsp_calls: usize,
    /// Non-zero when `lsp_diagnostics` reported issues this turn.
    pub lsp_diagnostic_issues: usize,
    /// Set when verify-before-finish failed at least once this turn.
    pub verify_ever_failed: bool,
    /// Workspace paths touched (read/edit/write) for spawn handoff pointers.
    pub touched_paths: Vec<String>,
    /// Index search queries issued this turn.
    pub index_queries: Vec<String>,
    /// Last LSP diagnostic summaries (for critic v4).
    pub lsp_diagnostic_snippets: Vec<String>,
}

impl TurnTracker {
    pub fn on_step(&mut self, step: &Step) {
        match step {
            Step::ToolCall { name, args, .. } => {
                if name.starts_with("lsp_") {
                    self.lsp_calls += 1;
                }
                if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
                    self.record_path(path);
                }
                if matches!(
                    name.as_str(),
                    "search_code" | "find_definition" | "semantic_search"
                ) {
                    if let Some(q) = args
                        .get("query")
                        .or_else(|| args.get("name"))
                        .and_then(|v| v.as_str())
                    {
                        if !self.index_queries.iter().any(|x| x == q) {
                            self.index_queries.push(q.to_string());
                        }
                    }
                }
            }
            Step::Note(msg) => {
                if let Some(cmd) = msg.strip_prefix("verify `") {
                    if let Some(rest) = cmd.split_once('`') {
                        let _cmd = rest.0;
                        if msg.contains(": passed") {
                            self.verify_passed = Some(true);
                        } else if msg.contains("FAILED") {
                            self.verify_passed = Some(false);
                            self.verify_ever_failed = true;
                        }
                    }
                }
            }
            Step::ToolResult {
                name,
                ok: true,
                summary,
                ..
            } if name == "lsp_diagnostics" => {
                if summary.contains("diagnostic(s)") {
                    if let Some(n) = parse_leading_count(summary) {
                        self.lsp_diagnostic_issues += n;
                    }
                }
                if !summary.is_empty() && !self.lsp_diagnostic_snippets.iter().any(|s| s == summary)
                {
                    self.lsp_diagnostic_snippets.push(summary.clone());
                }
            }
            Step::ToolResult { name, ok: true, .. }
                if matches!(name.as_str(), "edit_file" | "write_file") =>
            {
                self.mutating_edits += 1;
                if !self.mutating_tools.iter().any(|t| t == name) {
                    self.mutating_tools.push(name.clone());
                }
            }
            Step::ToolResult {
                name,
                ok: false,
                summary,
                ..
            } if name == "finish" && summary.contains("verify failed") => {
                self.verify_passed = Some(false);
                self.verify_ever_failed = true;
            }
            Step::Stopped(msg) if msg.starts_with("finished:") => {
                self.last_finish_summary =
                    Some(msg.trim_start_matches("finished:").trim().to_string());
            }
            _ => {}
        }
    }

    fn record_path(&mut self, path: &str) {
        let p = path.trim();
        if p.is_empty() {
            return;
        }
        if !self.touched_paths.iter().any(|x| x == p) {
            self.touched_paths.push(p.to_string());
        }
    }
}

fn parse_leading_count(summary: &str) -> Option<usize> {
    summary.split_whitespace().next()?.parse().ok()
}

pub fn specialist_agent_config(
    base: &AgentConfig,
    pack: &dyn SpecialistPack,
    role: &RoleId,
    agent_id: &str,
) -> AgentConfig {
    AgentConfig {
        agent_id: agent_id.to_string(),
        specialist_role: Some(role.as_str().to_string()),
        system_overlay: Some(pack.overlay(role)),
        max_steps: pack.max_steps(role, base.max_steps),
        explain_turn: pack.explain_turn(role, base.explain_turn),
        ..base.clone()
    }
}

pub fn next_child_agent_id(parent: &str, n: u32) -> String {
    format!("{parent}_s{n}")
}

pub fn route_label(decision: &RouteDecision) -> String {
    match &decision.mode {
        RouteMode::Single => format!("single: {}", decision.reason),
        RouteMode::Specialist(r) => format!("{}: {}", r.as_str(), decision.reason),
    }
}

pub fn critic_outcome_from_steps(tracker: &TurnTracker) -> CriticOutcome {
    parse_critic_verdict(tracker.last_finish_summary.as_deref().unwrap_or(""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::{route_task, RouteMode};
    use tetonic_core::{AgentConfig, Step};

    #[test]
    fn spawn_depth_counts_segments() {
        assert_eq!(spawn_depth("a0"), 0);
        assert_eq!(spawn_depth("a0_s0"), 1);
        assert_eq!(spawn_depth("a0_s1_s2"), 2);
    }

    #[test]
    fn spawn_limits_default_is_reasonable() {
        let l = SpawnLimits::default();
        assert!(l.max_per_turn >= 1);
        assert_eq!(l.max_depth, 2);
    }

    #[test]
    fn orchestration_mode_parse() {
        assert_eq!(OrchestrationMode::parse("auto"), OrchestrationMode::Auto);
        assert_eq!(OrchestrationMode::parse("swarm"), OrchestrationMode::Auto);
        assert_eq!(
            OrchestrationMode::parse("single"),
            OrchestrationMode::Single
        );
    }

    #[test]
    fn turn_tracker_counts_edits_and_finish() {
        let mut t = TurnTracker::default();
        t.on_step(&Step::ToolResult {
            call_id: "tc1".into(),
            name: "edit_file".into(),
            ok: true,
            summary: String::new(),
        });
        t.on_step(&Step::ToolResult {
            call_id: "tc2".into(),
            name: "read_file".into(),
            ok: true,
            summary: String::new(),
        });
        t.on_step(&Step::Stopped("finished: done".into()));
        assert_eq!(t.mutating_edits, 1);
        assert_eq!(t.mutating_tools, vec!["edit_file"]);
        assert_eq!(t.last_finish_summary.as_deref(), Some("done"));
    }

    #[test]
    fn turn_tracker_records_verify_and_lsp() {
        let mut t = TurnTracker::default();
        t.on_step(&Step::Note("verify `cargo test`: passed".into()));
        t.on_step(&Step::ToolCall {
            call_id: "tc3".into(),
            name: "lsp_diagnostics".into(),
            args: serde_json::json!({}),
        });
        assert_eq!(t.verify_passed, Some(true));
        assert_eq!(t.lsp_calls, 1);
    }

    #[test]
    fn specialist_config_sets_overlay_and_id() {
        let base = AgentConfig {
            model: "mock".into(),
            max_steps: 16,
            ..AgentConfig::default()
        };
        let cfg = specialist_agent_config(
            &base,
            &crate::specialist::TestCodingPack,
            &crate::specialist::RoleId::new("planner"),
            "a0_s3",
        );
        assert_eq!(cfg.agent_id, "a0_s3");
        assert_eq!(cfg.specialist_role.as_deref(), Some("planner"));
        assert!(cfg.system_overlay.unwrap().contains("planner"));
        assert_eq!(cfg.max_steps, 8);
    }

    #[test]
    fn child_agent_ids_are_stable() {
        assert_eq!(next_child_agent_id("a0", 0), "a0_s0");
        assert_eq!(next_child_agent_id("a0_s1", 2), "a0_s1_s2");
    }

    #[test]
    fn route_label_includes_role() {
        let d = route_task("Implement foo", true, &crate::specialist::TestCodingPack);
        let label = route_label(&d);
        assert!(label.starts_with("coder:"), "{label}");
        assert_eq!(
            d.mode,
            RouteMode::Specialist(crate::specialist::RoleId::new("coder"))
        );
    }

    #[test]
    fn critic_outcome_from_finish_summary() {
        assert!(matches!(
            critic_outcome_from_steps(&TurnTracker::default()),
            CriticOutcome::Revise(_)
        ));
        let mut t = TurnTracker::default();
        t.on_step(&Step::Stopped("finished: REVISE: add tests".into()));
        assert!(matches!(
            critic_outcome_from_steps(&t),
            CriticOutcome::Revise(_)
        ));
    }
}

//! Per-turn heuristics: empty-tool retries, no-progress detection, search/write discipline.
//! Tool names and copy come from product-supplied `LoopDiscipline`. Empty tables are silent.

use std::collections::HashMap;

use lokai_domain::{LoopDiscipline, ToolOutcome};
use serde_json::Value;

use crate::config::AgentConfig;

/// Result of recording one tool execution for progress tracking.
pub struct ToolProgressReport {
    pub append_to_model: String,
    pub stuck: bool,
}

/// Tracks loop-heuristic state for a single user turn.
pub struct HeuristicMonitor {
    empty_tool_retry_limit: u32,
    no_progress_limit: u32,
    search_miss_streak_limit: u32,
    write_repeat_limit: u32,
    empty_tool_nudge_text: Option<String>,
    whole_file_tools: Vec<String>,
    search_tools: Vec<String>,
    search_miss_nudge: Option<String>,
    write_repeat_feedback: Option<String>,
    empty_tool_retries: u32,
    call_counts: HashMap<String, u32>,
    no_progress: u32,
    search_miss_streak: u32,
    write_file_counts: HashMap<String, u32>,
}

impl HeuristicMonitor {
    pub fn new(config: &AgentConfig, discipline: &LoopDiscipline) -> Self {
        Self {
            empty_tool_retry_limit: config.empty_tool_retry_limit,
            no_progress_limit: config.no_progress_limit,
            search_miss_streak_limit: discipline.limits.search_miss_streak.unwrap_or(u32::MAX),
            write_repeat_limit: discipline.limits.write_repeat.unwrap_or(u32::MAX),
            empty_tool_nudge_text: discipline.empty_tool_nudge_text.clone(),
            whole_file_tools: discipline.whole_file_tools.clone(),
            search_tools: discipline.search_tools.clone(),
            search_miss_nudge: discipline.notes.search_miss_nudge.clone(),
            write_repeat_feedback: discipline.notes.write_repeat_feedback.clone(),
            empty_tool_retries: 0,
            call_counts: HashMap::new(),
            no_progress: 0,
            search_miss_streak: 0,
            write_file_counts: HashMap::new(),
        }
    }

    /// Nudge when the model returns no tool calls and product supplied text.
    pub fn empty_tool_nudge(
        &mut self,
        compiled_nudge: bool,
        message_count: usize,
    ) -> Option<String> {
        if self.empty_tool_retries >= self.empty_tool_retry_limit {
            return None;
        }
        if !compiled_nudge {
            return None;
        }
        if message_count > 6 {
            return None;
        }
        let text = self.empty_tool_nudge_text.as_ref()?.clone();
        self.empty_tool_retries += 1;
        Some(text)
    }

    pub fn empty_tool_retry_label(&self) -> String {
        format!(
            "model returned no tool calls — retry {}/{}",
            self.empty_tool_retries, self.empty_tool_retry_limit
        )
    }

    pub fn empty_tool_retries(&self) -> u32 {
        self.empty_tool_retries
    }

    /// Block write fragmentation before executing a mutating tool.
    pub fn check_write_repeat(&mut self, path: &str) -> Option<String> {
        let n = self.write_file_counts.entry(path.to_string()).or_insert(0);
        *n += 1;
        if *n > self.write_repeat_limit {
            self.write_repeat_feedback
                .as_ref()
                .map(|tmpl| tmpl.replace("{path}", path).replace("{n}", &n.to_string()))
        } else {
            None
        }
    }

    pub fn record_no_progress_event(&mut self) {
        self.no_progress += 1;
    }

    pub fn record_tool_execution(
        &mut self,
        name: &str,
        args: &Value,
        args_json: &str,
        path_arg: Option<&str>,
        whole_file_read: bool,
        outcome: &ToolOutcome,
    ) -> ToolProgressReport {
        let mut append = String::new();
        let is_whole = self.whole_file_tools.iter().any(|t| t == name);
        let is_search = self.search_tools.iter().any(|t| t == name);

        let progress_key = if is_whole {
            if whole_file_read {
                format!("{name}:whole:{}", path_arg.unwrap_or(""))
            } else {
                format!(
                    "{name}:partial:{}:{}:{}",
                    path_arg.unwrap_or(""),
                    args.get("start_line")
                        .map(|v| v.to_string())
                        .unwrap_or_default(),
                    args.get("end_line")
                        .map(|v| v.to_string())
                        .unwrap_or_default(),
                )
            }
        } else if is_search {
            format!(
                "{name}:{}",
                args.get("query").and_then(|v| v.as_str()).unwrap_or("")
            )
        } else {
            format!("{name}:{args_json}")
        };

        let times = {
            let c = self.call_counts.entry(progress_key).or_insert(0);
            *c += 1;
            *c
        };

        if times >= 2 {
            append.push_str(&format!(
                "\n\n[note] You have already made this exact tool call {times} times and the \
result will not change. Try a different tool or different arguments, or call the completion tool if the task is done."
            ));
        }

        if is_search && outcome.ok && outcome.summary.contains("no matches") {
            self.search_miss_streak += 1;
            if self.search_miss_streak >= self.search_miss_streak_limit {
                if let Some(note) = &self.search_miss_nudge {
                    append.push_str(note);
                }
            }
        } else if is_search {
            self.search_miss_streak = 0;
        }

        let made_change = outcome.ok && outcome.change.is_some();
        let path_repeat = is_whole && whole_file_read && times >= 2;
        if path_repeat {
            self.no_progress += 1;
        } else if made_change || (outcome.ok && times == 1) {
            self.no_progress = 0;
        } else {
            self.no_progress += 1;
        }

        let stuck = self.no_progress >= self.no_progress_limit;
        ToolProgressReport {
            append_to_model: append,
            stuck,
        }
    }

    pub fn stuck_reason(&self) -> &'static str {
        "stopping early: no progress over several steps (repeated or failing tool calls)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lokai_domain::{LoopDiscipline, LoopDisciplineLimits, LoopNotes, ToolOutcome};
    use serde_json::json;

    fn silent() -> LoopDiscipline {
        LoopDiscipline::default()
    }

    fn with_nudge(text: &str) -> LoopDiscipline {
        LoopDiscipline {
            empty_tool_nudge_text: Some(text.into()),
            ..LoopDiscipline::default()
        }
    }

    fn with_write_limit(n: u32) -> LoopDiscipline {
        LoopDiscipline {
            notes: LoopNotes {
                write_repeat_feedback: Some("too many writes to {path} ({n})".into()),
                ..LoopNotes::default()
            },
            limits: LoopDisciplineLimits {
                write_repeat: Some(n),
                ..LoopDisciplineLimits::default()
            },
            ..LoopDiscipline::default()
        }
    }

    #[test]
    fn empty_tool_nudge_respects_limit() {
        let config = AgentConfig {
            empty_tool_retry_limit: 2,
            ..AgentConfig::default()
        };
        let mut m = HeuristicMonitor::new(&config, &with_nudge("use tools"));
        assert!(m.empty_tool_nudge(true, 3).is_some());
        assert!(m.empty_tool_nudge(true, 5).is_some());
        assert!(m.empty_tool_nudge(true, 7).is_none());
        let mut m2 = HeuristicMonitor::new(&config, &with_nudge("use tools"));
        assert!(m2.empty_tool_nudge(false, 3).is_none());
    }

    #[test]
    fn sub04_monitor_empty_discipline_is_silent() {
        let config = AgentConfig::default();
        let mut m = HeuristicMonitor::new(&config, &silent());
        assert!(m.empty_tool_nudge(true, 3).is_none());
        assert!(m.check_write_repeat("src/lib.rs").is_none());
        assert!(m.check_write_repeat("src/lib.rs").is_none());
    }

    #[test]
    fn no_progress_triggers_stuck() {
        let config = AgentConfig {
            no_progress_limit: 3,
            ..AgentConfig::default()
        };
        let mut m = HeuristicMonitor::new(&config, &silent());
        let fail = ToolOutcome {
            ok: false,
            summary: "fail".into(),
            content: "ERROR".into(),
            error_kind: Some("denied".into()),
            change: None,
        };
        for _ in 0..2 {
            let r = m.record_tool_execution("grep", &json!({}), "{}", None, false, &fail);
            assert!(!r.stuck);
        }
        let r = m.record_tool_execution("grep", &json!({}), "{}", None, false, &fail);
        assert!(r.stuck);
    }

    #[test]
    fn write_file_fragmentation_blocked() {
        let config = AgentConfig::default();
        let mut m = HeuristicMonitor::new(&config, &with_write_limit(2));
        assert!(m.check_write_repeat("src/lib.rs").is_none());
        assert!(m.check_write_repeat("src/lib.rs").is_none());
        assert!(m.check_write_repeat("src/lib.rs").is_some());
    }
}

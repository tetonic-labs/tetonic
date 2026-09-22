//! Conversation transcript: stable prefixes, cap, token streaming.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentRoleKind {
    Primary,
    Specialist(String),
    Critic,
    Revision(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineKind {
    You,
    Lokai,
    Tool,
    Thought {
        collapsed: bool,
    },
    SubagentHeader {
        role: AgentRoleKind,
        agent_id: String,
        label: String,
    },
    SubagentStep {
        indent: usize,
    },
    SubagentFooter {
        ok: bool,
        summary: String,
    },
    Error,
    Warn,
    Sys,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptLine {
    pub kind: LineKind,
    pub text: String,
}

pub const TRANSCRIPT_CAP: usize = 400;
pub const TRANSCRIPT_BYTES: usize = 8 * 1024 * 1024;
pub const OMITTED: &str = "earlier turn output omitted";

impl TranscriptLine {
    pub fn new(mut kind: LineKind, text: impl Into<String>) -> Self {
        let mut text = text.into();
        super::retention::bound(&mut text, super::retention::DISPLAY_BYTES);
        match &mut kind {
            LineKind::SubagentHeader {
                role,
                agent_id,
                label,
            } => {
                super::retention::bound(agent_id, 4096);
                super::retention::bound(label, 4096);
                if let AgentRoleKind::Specialist(name) = role {
                    super::retention::bound(name, 4096);
                }
            }
            LineKind::SubagentFooter { summary, .. } => {
                super::retention::bound(summary, super::retention::DISPLAY_BYTES)
            }
            _ => {}
        }
        Self { kind, text }
    }

    fn retained_bytes(&self) -> usize {
        self.text.len()
            + match &self.kind {
                LineKind::SubagentHeader {
                    role,
                    agent_id,
                    label,
                } => {
                    agent_id.len()
                        + label.len()
                        + match role {
                            AgentRoleKind::Specialist(name) => name.len(),
                            _ => 0,
                        }
                }
                LineKind::SubagentFooter { summary, .. } => summary.len(),
                _ => 0,
            }
    }

    pub fn prefix(&self) -> &'static str {
        match &self.kind {
            LineKind::You => "you",
            LineKind::Lokai => "lokai",
            LineKind::Tool => "tool",
            LineKind::Thought { .. } => "thought",
            LineKind::SubagentHeader { .. } => "agent",
            LineKind::SubagentStep { .. } => "step",
            LineKind::SubagentFooter { .. } => "agent",
            LineKind::Error => "error",
            LineKind::Warn => "warn",
            LineKind::Sys => "sys",
        }
    }

    pub fn display(&self) -> String {
        match &self.kind {
            LineKind::Thought { collapsed } if *collapsed => {
                let count = self.text.chars().count();
                format!(
                    "{:<5} [thought: {count} chars · Ctrl+O to expand]",
                    self.prefix()
                )
            }
            LineKind::SubagentHeader { role, agent_id, .. } => match role {
                AgentRoleKind::Critic => format!("      ┌─ Critic Review ({agent_id})"),
                AgentRoleKind::Specialist(r) => format!("      ┌─ Specialist: {r} ({agent_id})"),
                AgentRoleKind::Revision(i) => format!("      ┌─ Revision {i} ({agent_id})"),
                AgentRoleKind::Primary => format!("      ┌─ Agent ({agent_id})"),
            },
            LineKind::SubagentStep { .. } => {
                format!("      │  {}", self.text)
            }
            LineKind::SubagentFooter { ok: _, summary } => {
                format!("      └─ {summary}")
            }
            _ => format!("{:<5} {}", self.prefix(), self.text),
        }
    }
}

pub fn push_line(lines: &mut Vec<TranscriptLine>, line: TranscriptLine) {
    lines.push(line);
    enforce_limits(lines);
}

fn enforce_limits(lines: &mut Vec<TranscriptLine>) {
    let mut bytes: usize = lines.iter().map(TranscriptLine::retained_bytes).sum();
    if lines.len() <= TRANSCRIPT_CAP && bytes <= TRANSCRIPT_BYTES {
        return;
    }
    let mut remove = 0;
    while remove < lines.len()
        && (lines.len() - remove >= TRANSCRIPT_CAP || bytes + OMITTED.len() > TRANSCRIPT_BYTES)
    {
        bytes -= lines[remove].retained_bytes();
        remove += 1;
    }
    lines.drain(..remove);
    lines.insert(0, TranscriptLine::new(LineKind::Sys, OMITTED));
}

pub fn append_token(lines: &mut Vec<TranscriptLine>, token: &str) {
    if let Some(last) = lines.last_mut() {
        if last.kind == LineKind::Lokai {
            super::retention::append(&mut last.text, token, super::retention::DISPLAY_BYTES);
            enforce_limits(lines);
            return;
        }
    }
    push_line(lines, TranscriptLine::new(LineKind::Lokai, token));
}

pub fn append_thought_token(lines: &mut Vec<TranscriptLine>, token: &str, collapsed: bool) {
    if let Some(last) = lines.last_mut() {
        if let LineKind::Thought { collapsed: c } = last.kind {
            super::retention::append(&mut last.text, token, super::retention::DISPLAY_BYTES);
            last.kind = LineKind::Thought { collapsed: c };
            enforce_limits(lines);
            return;
        }
    }
    push_line(
        lines,
        TranscriptLine::new(LineKind::Thought { collapsed }, token),
    );
}

pub fn set_thought_collapsed(lines: &mut [TranscriptLine], collapsed: bool) {
    for line in lines.iter_mut() {
        if let LineKind::Thought { .. } = line.kind {
            line.kind = LineKind::Thought { collapsed };
        }
    }
}

pub fn history_line(role: &str, content: &str) -> Option<TranscriptLine> {
    let text = content.trim();
    if text.is_empty() {
        return None;
    }
    match role {
        "user" => Some(TranscriptLine::new(LineKind::You, text)),
        "assistant" => {
            if text.starts_with('{') && text.contains("tool_calls") {
                None
            } else {
                Some(TranscriptLine::new(LineKind::Lokai, text))
            }
        }
        "system" | "tool" => None,
        _ => None,
    }
}

pub fn tool_summary(tool: &str, args: &serde_json::Value) -> String {
    match tool {
        "read_file" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let start = args.get("start_line").and_then(|v| v.as_u64());
            let end = args.get("end_line").and_then(|v| v.as_u64());
            match (start, end) {
                (Some(s), Some(e)) => format!("read_file {path}:{s}-{e}"),
                (Some(s), None) => format!("read_file {path}:{s}+"),
                _ => format!("read_file {path}"),
            }
        }
        "edit_file" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            format!("edit_file {path}")
        }
        "write_file" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            format!("write_file {path}")
        }
        "search_code" => {
            let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
            let file_pattern = args.get("file_pattern").and_then(|v| v.as_str());
            if let Some(pat) = file_pattern {
                format!("search_code \"{query}\" ({pat})")
            } else {
                format!("search_code \"{query}\"")
            }
        }
        "find_definition" => {
            let symbol = args.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
            format!("find_definition symbol=\"{symbol}\"")
        }
        "outline" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            format!("outline {path}")
        }
        "run_shell" => {
            let cmd = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
            let one_line = cmd.split_whitespace().collect::<Vec<_>>().join(" ");
            if one_line.chars().count() > 72 {
                let clipped: String = one_line.chars().take(71).collect();
                format!("run_shell {clipped}…")
            } else {
                format!("run_shell {one_line}")
            }
        }
        "spawn_agent" => {
            let role = args
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("specialist");
            let task = args.get("task").and_then(|v| v.as_str()).unwrap_or("");
            let task_preview = if task.chars().count() > 40 {
                let c: String = task.chars().take(39).collect();
                format!("{c}…")
            } else {
                task.to_string()
            };
            format!("spawn_agent ({role}) \"{task_preview}\"")
        }
        "finish" => "finish".to_string(),
        _ => {
            if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
                format!("{tool} {path}")
            } else if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
                let one_line = cmd.split_whitespace().collect::<Vec<_>>().join(" ");
                if one_line.chars().count() > 72 {
                    let clipped: String = one_line.chars().take(71).collect();
                    format!("{tool} {clipped}…")
                } else {
                    format!("{tool} {one_line}")
                }
            } else {
                tool.to_string()
            }
        }
    }
}

pub fn apply_tool_result(lines: &mut Vec<TranscriptLine>, tool: &str, ok: bool, summary: &str) {
    let outcome = if ok && !summary.trim().is_empty() {
        format!("ok — {}", summary.trim())
    } else if ok {
        "ok".to_string()
    } else if summary.trim().is_empty() {
        "failed".to_string()
    } else {
        format!("failed — {}", summary.trim())
    };
    let line = if let Some(last) = lines.last_mut() {
        if (last.kind == LineKind::Tool || matches!(last.kind, LineKind::SubagentStep { .. }))
            && last.text.starts_with(tool)
            && !last.text.contains(" — ")
            && !last.text.ends_with(" ok")
            && !last.text.ends_with(" failed")
        {
            last.text = format!("{} {outcome}", last.text);
            clip_tool_line(&mut last.text);
            enforce_limits(lines);
            return;
        }
        format!("{tool} {outcome}")
    } else {
        format!("{tool} {outcome}")
    };
    let mut line = line;
    clip_tool_line(&mut line);
    push_line(lines, TranscriptLine::new(LineKind::Tool, line));
}

pub const TOOL_LINE_MAX_CHARS: usize = 160;

fn clip_tool_line(text: &mut String) {
    if text.chars().count() <= TOOL_LINE_MAX_CHARS {
        return;
    }
    *text = format!(
        "{}…",
        text.chars()
            .take(TOOL_LINE_MAX_CHARS.saturating_sub(1))
            .collect::<String>()
    );
}

#[allow(dead_code)]
pub fn classify_diagnostic(message: &str) -> LineKind {
    if message.trim_start().starts_with("capacity:") {
        LineKind::Warn
    } else {
        LineKind::Sys
    }
}

/// Telemetry stays out of the transcript unless it is a capacity warn-and-proceed.
pub fn diagnostic_is_transcript(message: &str) -> bool {
    message.trim_start().starts_with("capacity:")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn alternating_streams_obey_count_limit() {
        let mut lines = Vec::new();
        for _ in 0..TRANSCRIPT_CAP {
            append_token(&mut lines, "answer");
            append_thought_token(&mut lines, "thought", true);
        }
        assert!(lines.len() <= TRANSCRIPT_CAP);
        assert_eq!(lines.first().unwrap().text, OMITTED);
        assert_eq!(lines.last().unwrap().text, "thought");
    }

    #[test]
    fn transcript_obeys_total_payload_and_single_stream_limits() {
        let mut lines = Vec::new();
        for i in 0..100 {
            push_line(
                &mut lines,
                TranscriptLine::new(LineKind::Lokai, format!("{i}:{}", "x".repeat(200_000))),
            );
        }
        assert!(
            lines
                .iter()
                .map(TranscriptLine::retained_bytes)
                .sum::<usize>()
                <= TRANSCRIPT_BYTES
        );
        assert_eq!(lines.first().unwrap().text, OMITTED);
        assert!(lines.last().unwrap().text.starts_with("99:"));
        for _ in 0..100 {
            append_token(&mut lines, &"🦀".repeat(10_000));
        }
        assert!(lines.last().unwrap().text.len() <= super::super::retention::DISPLAY_BYTES);
        assert!(lines
            .last()
            .unwrap()
            .text
            .ends_with(super::super::retention::TRUNCATED));
        assert!(
            lines
                .iter()
                .map(TranscriptLine::retained_bytes)
                .sum::<usize>()
                <= TRANSCRIPT_BYTES
        );
    }

    #[test]
    fn prefixes_are_stable() {
        assert_eq!(TranscriptLine::new(LineKind::You, "hi").prefix(), "you");
        assert_eq!(TranscriptLine::new(LineKind::Lokai, "ok").prefix(), "lokai");
        assert_eq!(TranscriptLine::new(LineKind::Tool, "ps").prefix(), "tool");
        assert_eq!(TranscriptLine::new(LineKind::Error, "x").prefix(), "error");
        assert_eq!(
            TranscriptLine::new(LineKind::You, "hi").display(),
            "you   hi"
        );
    }

    #[test]
    fn cap_injects_omitted_marker() {
        let mut lines = Vec::new();
        for i in 0..(TRANSCRIPT_CAP + 5) {
            push_line(
                &mut lines,
                TranscriptLine::new(LineKind::Lokai, format!("{i}")),
            );
        }
        assert_eq!(lines.len(), TRANSCRIPT_CAP);
        assert_eq!(lines[0].kind, LineKind::Sys);
        assert_eq!(lines[0].text, OMITTED);
        assert_eq!(
            lines.last().unwrap().text,
            format!("{}", TRANSCRIPT_CAP + 4)
        );
    }

    #[test]
    fn tokens_append_to_last_assistant_line() {
        let mut lines = vec![TranscriptLine::new(LineKind::You, "hi")];
        append_token(&mut lines, "Hel");
        append_token(&mut lines, "lo");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].text, "Hello");
        assert_eq!(lines[1].kind, LineKind::Lokai);
    }

    #[test]
    fn tool_summary_prefers_path_and_command() {
        assert_eq!(
            tool_summary("read_file", &json!({"path": "src/main.rs"})),
            "read_file src/main.rs"
        );
        assert_eq!(
            tool_summary(
                "read_file",
                &json!({"path": "src/main.rs", "start_line": 10, "end_line": 30})
            ),
            "read_file src/main.rs:10-30"
        );
        assert_eq!(
            tool_summary(
                "search_code",
                &json!({"query": "InferenceProvider", "file_pattern": "*.rs"})
            ),
            "search_code \"InferenceProvider\" (*.rs)"
        );
        assert_eq!(
            tool_summary("find_definition", &json!({"symbol": "TurnPhase"})),
            "find_definition symbol=\"TurnPhase\""
        );
        assert_eq!(
            tool_summary(
                "spawn_agent",
                &json!({"role": "coder", "task": "fix bug in mathx.rs"})
            ),
            "spawn_agent (coder) \"fix bug in mathx.rs\""
        );
        assert_eq!(
            tool_summary("run_shell", &json!({"command": "cargo test"})),
            "run_shell cargo test"
        );
    }

    #[test]
    fn capacity_diagnostics_are_transcript_warnings() {
        let msg = "capacity: saved profile for `qwen` is degraded. Chat continues; inference will abort if this model spills VRAM.";
        assert!(diagnostic_is_transcript(msg));
        assert_eq!(classify_diagnostic(msg), LineKind::Warn);
        assert!(!diagnostic_is_transcript(
            "router: single: orchestration disabled"
        ));
    }

    #[test]
    fn history_skips_system_and_tool_role() {
        assert!(history_line("system", "You are an AI assistant...").is_none());
        assert!(history_line("tool", "ERROR: already read").is_none());
        assert_eq!(history_line("user", "howdy").unwrap().kind, LineKind::You);
        assert_eq!(
            history_line("assistant", "hello").unwrap().kind,
            LineKind::Lokai
        );
    }

    #[test]
    fn tool_result_merges_onto_the_call_line() {
        let mut lines = vec![TranscriptLine::new(LineKind::Tool, "read_file src/lib.rs")];
        apply_tool_result(&mut lines, "read_file", false, "duplicate read: src/lib.rs");
        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0].text,
            "read_file src/lib.rs failed — duplicate read: src/lib.rs"
        );
        apply_tool_result(&mut lines, "outline", true, "12 symbols");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].text, "outline ok — 12 symbols");
        let mut dump = vec![TranscriptLine::new(LineKind::Tool, "run_shell ls")];
        apply_tool_result(
            &mut dump,
            "run_shell",
            false,
            &format!("sandbox outcome={{{}}} {}", "x".repeat(400), "audit"),
        );
        assert!(dump[0].text.chars().count() <= TOOL_LINE_MAX_CHARS);
        assert!(dump[0].text.ends_with('…'));
    }

    #[test]
    fn staged_write_is_not_presented_as_an_applied_edit() {
        let mut lines = vec![TranscriptLine::new(
            LineKind::Tool,
            "write_file dice/go.mod",
        )];
        apply_tool_result(
            &mut lines,
            "write_file",
            true,
            "staged create dice/go.mod (12 bytes)",
        );
        assert_eq!(lines.len(), 1);
        assert!(lines[0].text.contains("ok — staged create"));
    }

    #[test]
    fn subagent_tree_lines_render_box_drawing_connectors() {
        let header = TranscriptLine::new(
            LineKind::SubagentHeader {
                role: AgentRoleKind::Specialist("Coder".into()),
                agent_id: "a0.1".into(),
                label: "Specialist: Coder".into(),
            },
            "Specialist: Coder",
        );
        assert_eq!(header.prefix(), "agent");
        assert!(header.display().contains("┌─ Specialist: Coder (a0.1)"));

        let critic = TranscriptLine::new(
            LineKind::SubagentHeader {
                role: AgentRoleKind::Critic,
                agent_id: "a0.2".into(),
                label: "Critic Review".into(),
            },
            "Critic Review",
        );
        assert!(critic.display().contains("┌─ Critic Review (a0.2)"));

        let step = TranscriptLine::new(
            LineKind::SubagentStep { indent: 0 },
            "read_file src/lib.rs ok",
        );
        assert_eq!(step.prefix(), "step");
        assert_eq!(step.display(), "      │  read_file src/lib.rs ok");

        let footer = TranscriptLine::new(
            LineKind::SubagentFooter {
                ok: true,
                summary: "Succeeded".into(),
            },
            "Succeeded",
        );
        assert_eq!(footer.prefix(), "agent");
        assert_eq!(footer.display(), "      └─ Succeeded");
    }

    #[test]
    fn test_large_transcript_culling_performance() {
        let mut transcript = Vec::new();
        for i in 0..1000 {
            transcript.push(TranscriptLine::new(
                LineKind::Lokai,
                format!("Message number {i} with some code tokens"),
            ));
        }
        assert_eq!(transcript.len(), 1000);
        let start = std::time::Instant::now();
        let display_sample: Vec<String> = transcript.iter().take(50).map(|t| t.display()).collect();
        let elapsed = start.elapsed();
        assert_eq!(display_sample.len(), 50);
        assert!(
            elapsed.as_millis() < 50,
            "50 visible items must render in < 50ms (was {:?})",
            elapsed
        );
    }
}

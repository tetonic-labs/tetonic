//! Session resume rehydration (H3-1).
//!
//! One implementation for CLI and daemon. A resumed transcript is faithful
//! (tool_call_id / tool_name preserved) or explicitly marked as lossy.

use tetonic_inference::{Message, ToolCall};

/// Newest-N cap for session resume. Truncation injects a visible system note.
pub const RESUME_MESSAGE_CAP: u32 = 200;

/// Convert stored audit rows into inference messages.
///
/// `total_eligible` is the uncapped count matching `list_messages_for_resume`'s
/// filter. When it exceeds `rows.len()`, a system note records how many earlier
/// messages were omitted.
pub fn rehydrate_messages(
    rows: &[tetonic_memory::StoredMessageRow],
    total_eligible: u32,
) -> Vec<Message> {
    let mut out = Vec::with_capacity(rows.len().saturating_add(1));
    let loaded = rows.len() as u32;
    if total_eligible > loaded {
        let omitted = total_eligible.saturating_sub(loaded);
        out.push(Message::system(format!(
            "[resume] {omitted} earlier messages omitted (showing the newest {loaded} of {total_eligible})."
        )));
    }
    for r in rows {
        out.push(row_to_message(r));
    }
    // The product commits the initial user message when planning the turn,
    // before the agent persists its system prompt. Inference receives the
    // opposite order: system, user. Replaying audit order changes the very
    // beginning of the prompt and invalidates the entire warm prefix on resume.
    // Only repair this initial, complete-transcript pattern; later system
    // messages and explicitly truncated transcripts retain their positions.
    if total_eligible <= loaded
        && rows.len() >= 2
        && rows[0].role == "user"
        && rows[1].role == "system"
    {
        out.swap(0, 1);
    }
    out
}

fn row_to_message(r: &tetonic_memory::StoredMessageRow) -> Message {
    let mut m = match r.role.as_str() {
        "system" => Message::system(&r.content),
        "user" => Message::user(&r.content),
        "assistant" => Message::assistant(&r.content),
        "tool" => {
            let name = r
                .tool_name
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("tool");
            Message::tool(name, &r.content)
        }
        other => Message::system(format!(
            "[resume] unrecognized role `{other}`: {}",
            r.content
        )),
    };
    if let Some(id) = r.tool_call_id.as_deref().filter(|s| !s.is_empty()) {
        m = m.with_tool_call_id(id);
    }
    if let Some(ref tcj) = r.tool_calls_json {
        if !tcj.is_empty() {
            if let Ok(calls) = serde_json::from_str::<Vec<ToolCall>>(tcj) {
                m.tool_calls = Some(calls);
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use tetonic_memory::StoredMessageRow;

    fn row(
        role: &str,
        content: &str,
        tool_name: Option<&str>,
        tool_call_id: Option<&str>,
        tool_calls_json: Option<&str>,
    ) -> StoredMessageRow {
        StoredMessageRow {
            role: role.into(),
            content: content.into(),
            tool_calls_json: tool_calls_json.map(str::to_string),
            tool_name: tool_name.map(str::to_string),
            tool_call_id: tool_call_id.map(str::to_string),
        }
    }

    #[test]
    fn tool_call_id_round_trips() {
        let rows = vec![
            row("user", "read it", None, None, None),
            row(
                "assistant",
                "",
                None,
                None,
                Some(r#"[{"function":{"name":"read_file","arguments":"{}"}}]"#),
            ),
            row(
                "tool",
                "file contents",
                Some("read_file"),
                Some("tc_1"),
                None,
            ),
        ];
        let msgs = rehydrate_messages(&rows, 3);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[2].role, "tool");
        assert_eq!(msgs[2].tool_name.as_deref(), Some("read_file"));
        assert_eq!(msgs[2].tool_call_id.as_deref(), Some("tc_1"));
        assert_eq!(msgs[1].tool_calls.as_ref().map(|c| c.len()), Some(1));
    }

    #[test]
    fn truncation_injects_elision_note() {
        let rows = vec![row("user", "newest", None, None, None)];
        let msgs = rehydrate_messages(&rows, 201);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "system");
        assert!(msgs[0].content.contains("200 earlier messages omitted"));
        assert!(msgs[0].content.contains("newest 1 of 201"));
        assert_eq!(msgs[1].content, "newest");
    }

    #[test]
    fn unrecognized_role_is_visible() {
        let rows = vec![row("narrator", "once upon a time", None, None, None)];
        let msgs = rehydrate_messages(&rows, 1);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "system");
        assert!(msgs[0].content.contains("unrecognized role `narrator`"));
        assert!(msgs[0].content.contains("once upon a time"));
    }

    #[test]
    fn kernel_rehydrate_is_deterministic() {
        let rows = vec![
            row("tool", "ok", Some("grep"), Some("tc_ab"), None),
            row("user", "again", None, None, None),
        ];
        let a = rehydrate_messages(&rows, 2);
        let b = rehydrate_messages(&rows, 2);
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x.role, y.role);
            assert_eq!(x.content, y.content);
            assert_eq!(x.tool_name, y.tool_name);
            assert_eq!(x.tool_call_id, y.tool_call_id);
        }
    }

    #[test]
    fn resume_preserves_the_live_inference_prefix_without_dropping_history() {
        let rows = vec![
            row("user", "inspect", None, None, None),
            row("system", "instructions", None, None, None),
            row("assistant", "answer", None, None, None),
            row(
                "tool",
                "full evidence",
                Some("read_file"),
                Some("tc_1"),
                None,
            ),
        ];
        let messages = rehydrate_messages(&rows, rows.len() as u32);
        let expected = vec![
            Message::system("instructions"),
            Message::user("inspect"),
            Message::assistant("answer"),
            Message::tool("read_file", "full evidence").with_tool_call_id("tc_1"),
        ];
        assert_eq!(
            serde_json::to_value(messages).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }

    #[test]
    fn resume_does_not_reorder_truncated_or_later_system_messages() {
        let rows = vec![
            row("user", "earlier", None, None, None),
            row("system", "later instructions", None, None, None),
        ];
        let truncated = rehydrate_messages(&rows, 3);
        assert_eq!(truncated[1].content, "earlier");
        assert_eq!(truncated[2].content, "later instructions");
        let rows = vec![
            row("user", "earlier", None, None, None),
            row("assistant", "answer", None, None, None),
            row("system", "later instructions", None, None, None),
        ];
        let complete = rehydrate_messages(&rows, 3);
        assert_eq!(complete[0].content, "earlier");
        assert_eq!(complete[2].content, "later instructions");
    }
}

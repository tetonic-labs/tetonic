//! Limits for retained presentation state, independent of model context policy.
pub const INPUT_BYTES: usize = 1024 * 1024;
pub const HISTORY_BYTES: usize = 4 * 1024 * 1024;
pub const HISTORY_ITEMS: usize = 100;
pub const DISPLAY_BYTES: usize = 256 * 1024;
pub const INSPECTOR_BYTES: usize = 1024 * 1024;
pub const TRUNCATED: &str = "\n[display limit reached; further text omitted]";

pub fn bound(text: &mut String, limit: usize) {
    if text.len() <= limit {
        return;
    }
    let mut end = limit.saturating_sub(TRUNCATED.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str(TRUNCATED);
    text.shrink_to_fit();
}

pub fn append(text: &mut String, incoming: &str, limit: usize) {
    if text.ends_with(TRUNCATED) {
        return;
    }
    let room = limit.saturating_sub(text.len());
    let mut end = room.min(incoming.len());
    while !incoming.is_char_boundary(end) {
        end -= 1;
    }
    text.push_str(&incoming[..end]);
    if end < incoming.len() {
        // Force the same visible truncation marker without copying the payload.
        text.push_str(TRUNCATED);
        bound(text, limit);
    }
}

pub fn remember(history: &mut Vec<String>, text: &str) {
    if history.last().is_some_and(|last| last == text) {
        return;
    }
    history.push(text.to_string());
    let mut bytes: usize = history.iter().map(String::len).sum();
    let mut remove = 0;
    while history.len() - remove > HISTORY_ITEMS || bytes > HISTORY_BYTES {
        bytes -= history[remove].len();
        remove += 1;
    }
    history.drain(..remove);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streamed_unicode_is_bounded_and_omission_is_visible() {
        let mut text = String::new();
        for _ in 0..100 {
            append(&mut text, &"🦀".repeat(100), 1000);
        }
        assert!(text.len() <= 1000);
        assert!(text.ends_with(TRUNCATED));
        let before = text.clone();
        append(&mut text, "more", 1000);
        assert_eq!(text, before);
    }

    #[test]
    fn history_bounds_count_and_bytes_and_keeps_newest() {
        let mut history = Vec::new();
        for i in 0..200 {
            remember(&mut history, &i.to_string());
        }
        assert_eq!(history.len(), HISTORY_ITEMS);
        for i in 0..10 {
            remember(&mut history, &format!("{i}{}", "x".repeat(INPUT_BYTES - 1)));
        }
        assert!(history.iter().map(String::len).sum::<usize>() <= HISTORY_BYTES);
        assert!(history.last().unwrap().starts_with('9'));
    }

    #[test]
    fn inspector_stream_and_activity_have_payload_limits() {
        let mut app = super::super::App::test_stub();
        let coordinator = crate::app_kernel::TerminalApprovalCoordinator::new();
        for _ in 0..20 {
            super::super::events::apply(
                &mut app,
                tetonic_app::events::ApplicationEvent::InspectorUpdate {
                    text: "x".repeat(100_000),
                },
                &coordinator,
            );
            app.push_activity("x".repeat(100_000));
        }
        assert!(app.inspector_text.len() <= INSPECTOR_BYTES);
        assert!(app.inspector_text.ends_with(TRUNCATED));
        assert!(app
            .activity
            .iter()
            .all(|line| line.len() <= 16 * 1024 && line.ends_with(TRUNCATED)));
    }
}

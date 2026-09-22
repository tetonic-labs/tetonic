//! View state and navigation, independent of execution policy.
use super::transcript::{LineKind, TranscriptLine};
use super::{App, LayoutMode};
use ratatui::layout::Rect;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Focus {
    #[default]
    Composer,
    Chat,
    Inspector,
}

#[derive(Default)]
pub(crate) struct ViewState {
    pub(super) chat_layout: Option<std::rc::Rc<super::ui::ChatLayout>>,
    pub focus: Focus,
    pub chat: Rect,
    pub inspector: Rect,
    pub composer: Rect,
    pub terminal_width: u16,
    pub max_scroll: u16,
    pub inspector_max_scroll: u16,
    pub inspector_follow: bool,
    pub turn_rows: Vec<usize>,
    pub copy_entry: Option<usize>,
    pub show_preferences: bool,
    pub preferences_dirty: bool,
    pub help_scroll: u16,
    pub overlay_max_scroll: u16,
    pub copy: Option<Vec<(String, String)>>,
    pub copy_part: usize,
    pub copy_scroll: u16,
    pub copy_notice: Option<String>,
}

pub fn latest(app: &mut App) {
    app.chat_scroll_offset = 0;
    app.reading = None;
}

pub fn scroll_chat(app: &mut App, delta: i32) {
    let max = {
        let view = app.view.borrow();
        if view.chat.height == 0 {
            u16::MAX
        } else {
            view.max_scroll
        }
    };
    let offset = (i32::from(app.chat_scroll_offset) + delta).clamp(0, i32::from(max)) as u16;
    if offset > 0 && app.reading.is_none() {
        app.reading = Some(app.transcript.clone());
    }
    app.chat_scroll_offset = offset;
    if offset == 0 {
        app.reading = None;
    }
}

pub fn toggle_sidebar(app: &mut App) {
    app.preferences.sidebar = app.layout_mode == LayoutMode::FullChat;
    app.layout_mode = if app.preferences.sidebar {
        LayoutMode::Split
    } else {
        LayoutMode::FullChat
    };
    let view = app.view.get_mut();
    view.preferences_dirty = true;
    view.focus = if app.preferences.sidebar && view.terminal_width < 100 {
        Focus::Inspector
    } else {
        Focus::Composer
    };
}

pub fn cycle_focus(app: &mut App, backwards: bool) {
    let sidebar = app.layout_mode != LayoutMode::FullChat;
    let view = app.view.get_mut();
    view.focus = match (view.focus, backwards, sidebar) {
        (Focus::Composer, false, _) | (Focus::Inspector, true, _) => Focus::Chat,
        (Focus::Chat, false, true) | (Focus::Composer, true, true) => Focus::Inspector,
        (Focus::Chat, true, _) | (Focus::Inspector, false, _) | (Focus::Chat, false, false) => {
            Focus::Composer
        }
        (Focus::Composer, true, false) => Focus::Chat,
    };
    if view.focus == Focus::Chat && app.layout_mode == LayoutMode::FullInspector {
        app.layout_mode = LayoutMode::Split;
    }
}

pub fn jump_turn(app: &mut App, next: bool) {
    let view = app.view.borrow();
    let current = usize::from(view.max_scroll.saturating_sub(app.chat_scroll_offset));
    let target = if next {
        view.turn_rows.iter().find(|row| **row > current)
    } else {
        view.turn_rows.iter().rev().find(|row| **row < current)
    }
    .copied();
    let offset = target.map(|row| usize::from(view.max_scroll).saturating_sub(row) as u16);
    drop(view);
    if let Some(offset) = offset {
        if app.reading.is_none() {
            app.reading = Some(app.transcript.clone());
        }
        app.chat_scroll_offset = offset;
    } else if next {
        latest(app);
    }
}

pub fn open_copy(app: &mut App) {
    let transcript: &[TranscriptLine] = app.reading.as_deref().unwrap_or(&app.transcript);
    let selected = app.view.borrow().copy_entry;
    let answer = selected
        .and_then(|i| transcript.get(i))
        .filter(|e| e.kind == LineKind::Lokai)
        .or_else(|| transcript.iter().rev().find(|e| e.kind == LineKind::Lokai));
    let Some(answer) = answer else {
        app.status_hint = Some("No assistant response to copy yet".into());
        app.hint_ticks = 400;
        return;
    };
    let parts = copy_parts(&answer.text);
    let view = app.view.get_mut();
    view.copy = Some(parts);
    view.copy_part = 0;
    view.copy_scroll = 0;
    view.copy_notice = None;
}

fn copy_parts(text: &str) -> Vec<(String, String)> {
    let mut parts = vec![("Answer (Markdown)".into(), text.to_owned())];
    let mut fence: Option<(char, usize, String, String)> = None;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if let Some((marker, count, _, body)) = fence.as_mut() {
            let n = trimmed.chars().take_while(|c| c == marker).count();
            if n >= *count && trimmed[n..].trim().is_empty() {
                if let Some((_, _, label, body)) = fence.take() {
                    parts.push((format!("Code {} - {label}", parts.len()), body));
                }
            } else {
                body.push_str(line);
            }
        } else if let Some(marker) = trimmed.chars().next().filter(|c| *c == '`' || *c == '~') {
            let count = trimmed.chars().take_while(|c| *c == marker).count();
            if count >= 3 {
                fence = Some((marker, count, trimmed[count..].trim().into(), String::new()));
            }
        }
    }
    if let Some((_, _, label, body)) = fence {
        parts.push((format!("Code {} - {label} (streaming)", parts.len()), body));
    }
    parts
}

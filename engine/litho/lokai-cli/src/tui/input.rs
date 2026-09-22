//! Input routing follows the visible focus or modal; typing never accepts an approval.
use super::{
    composer,
    interaction::{self, Focus},
    slash, App, LayoutMode,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

pub enum InputEffect {
    OpenModels,
    SelectModel {
        id: String,
        revision: u64,
    },
    AuthenticateAndSelectModel {
        provider_id: String,
        key: String,
        model: String,
    },
    None,
    Submit(String),
    Quit,
    CancelTurn,
    Copy(String),
    RespondApproval {
        approved: bool,
        remember: bool,
    },
}

pub fn handle_key(app: &mut App, key: KeyEvent) -> InputEffect {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    if ctrl && key.code == KeyCode::Char('q') {
        return InputEffect::Quit;
    }
    if ctrl && key.code == KeyCode::Char('c') {
        if app.thinking || app.pending_approval.is_some() {
            return InputEffect::CancelTurn;
        }
        app.status_hint = Some("No turn to cancel - Ctrl+Q quits; F3 opens copy view".into());
        app.hint_ticks = 400;
        return InputEffect::None;
    }
    if app.pending_approval.is_some() {
        return handle_approval_key(app, key);
    }
    if app.model_picker.is_some() {
        return super::models::handle_key(app, key);
    }
    if app.show_help || app.view.borrow().show_preferences || app.view.borrow().copy.is_some() {
        return handle_overlay_key(app, key);
    }
    match key.code {
        KeyCode::F(5) => return InputEffect::OpenModels,
        KeyCode::F(1) => {
            app.show_help = true;
            app.view.get_mut().help_scroll = 0;
        }
        KeyCode::F(2) => {
            app.view.get_mut().show_preferences = true;
            app.view.get_mut().help_scroll = 0;
        }
        KeyCode::F(3) => interaction::open_copy(app),
        KeyCode::F(4) => {
            app.preferences.mouse = !app.preferences.mouse;
            app.view.get_mut().preferences_dirty = true;
        }
        KeyCode::Char('e') if ctrl => interaction::toggle_sidebar(app),
        KeyCode::Char('l') if ctrl => {
            app.show_activity = !app.show_activity;
            app.inspector_scroll_offset = 0;
            app.view.get_mut().inspector_follow = app.show_activity;
            if app.layout_mode == LayoutMode::FullChat {
                interaction::toggle_sidebar(app);
            }
            app.view.get_mut().focus = Focus::Inspector;
        }
        KeyCode::F(6) => {
            app.layout_mode = if app.layout_mode == LayoutMode::FullInspector {
                LayoutMode::Split
            } else {
                LayoutMode::FullInspector
            };
            app.view.get_mut().focus = Focus::Inspector;
        }
        KeyCode::Char('o') if ctrl => {
            app.expand_thoughts = !app.expand_thoughts;
            super::transcript::set_thought_collapsed(&mut app.transcript, !app.expand_thoughts);
            if let Some(reading) = &mut app.reading {
                super::transcript::set_thought_collapsed(reading, !app.expand_thoughts);
            }
        }
        KeyCode::Char('t') if ctrl => {
            app.preferences.hide_tools = !app.preferences.hide_tools;
            app.view.get_mut().preferences_dirty = true;
        }
        KeyCode::Char('p') if ctrl => interaction::jump_turn(app, false),
        KeyCode::Char('n') if ctrl => interaction::jump_turn(app, true),
        KeyCode::End if ctrl => interaction::latest(app),
        KeyCode::BackTab => interaction::cycle_focus(app, true),
        KeyCode::Tab
            if app.view.borrow().focus == Focus::Composer && app.input_buffer.starts_with('/') =>
        {
            let prefix = app
                .tab_prefix
                .clone()
                .unwrap_or_else(|| app.input_buffer.clone());
            if app.tab_prefix.is_none() {
                app.tab_prefix = Some(prefix.clone());
            }
            if let Some((filled, next)) = slash::cycle(&prefix, app.tab_cycle) {
                app.input_buffer = filled;
                app.tab_cycle = next;
                app.input_cursor = app.input_buffer.chars().count();
            }
        }
        KeyCode::Tab => interaction::cycle_focus(app, false),
        KeyCode::Esc => {
            app.view.get_mut().focus = Focus::Composer;
        }
        KeyCode::PageUp => scroll_focused(app, -page_size(app)),
        KeyCode::PageDown => scroll_focused(app, page_size(app)),
        KeyCode::Up if ctrl => interaction::scroll_chat(app, 1),
        KeyCode::Down if ctrl => interaction::scroll_chat(app, -1),
        KeyCode::Up if key.modifiers.contains(KeyModifiers::ALT) => history_up(app),
        KeyCode::Down if key.modifiers.contains(KeyModifiers::ALT) => history_down(app),
        KeyCode::Char('[') if app.view.borrow().focus == Focus::Inspector => {
            scroll_focused(app, -5)
        }
        KeyCode::Char(']') if app.view.borrow().focus == Focus::Inspector => scroll_focused(app, 5),
        KeyCode::Up if app.view.borrow().focus != Focus::Composer => scroll_focused(app, -1),
        KeyCode::Down if app.view.borrow().focus != Focus::Composer => scroll_focused(app, 1),
        _ if app.view.borrow().focus != Focus::Composer => {}
        KeyCode::Enter
            if key
                .modifiers
                .intersects(KeyModifiers::ALT | KeyModifiers::SHIFT) =>
        {
            insert_text(app, "\n");
        }
        KeyCode::Char('j') if ctrl => insert_text(app, "\n"),
        KeyCode::Char('v') | KeyCode::Char('V') if ctrl => {
            if let Some(clip) = super::clipboard::read_clipboard_text() {
                handle_paste(app, &clip);
            }
        }
        KeyCode::Insert if key.modifiers.contains(KeyModifiers::SHIFT) => {
            if let Some(clip) = super::clipboard::read_clipboard_text() {
                handle_paste(app, &clip);
            }
        }
        KeyCode::Enter => return submit(app),
        KeyCode::Up if composer_is_multiline(app) => move_vertical(app, false),
        KeyCode::Down if composer_is_multiline(app) => move_vertical(app, true),
        KeyCode::Up => history_up(app),
        KeyCode::Down => history_down(app),
        KeyCode::Left => app.input_cursor = app.input_cursor.saturating_sub(1),
        KeyCode::Right => {
            app.input_cursor = (app.input_cursor + 1).min(app.input_buffer.chars().count())
        }
        KeyCode::Home => {
            app.input_cursor = app
                .input_buffer
                .chars()
                .take(app.input_cursor)
                .enumerate()
                .filter(|(_, c)| *c == '\n')
                .map(|(i, _)| i + 1)
                .last()
                .unwrap_or(0);
        }
        KeyCode::End => {
            app.input_cursor += app
                .input_buffer
                .chars()
                .skip(app.input_cursor)
                .take_while(|c| *c != '\n')
                .count();
        }
        KeyCode::Backspace => {
            if app.input_cursor > 0 {
                let mut chars: Vec<_> = app.input_buffer.chars().collect();
                app.input_cursor = app.input_cursor.min(chars.len()).saturating_sub(1);
                chars.remove(app.input_cursor);
                app.input_buffer = chars.into_iter().collect();
                typed(app);
            }
        }
        KeyCode::Delete => {
            let mut chars: Vec<_> = app.input_buffer.chars().collect();
            if app.input_cursor < chars.len() {
                chars.remove(app.input_cursor);
                app.input_buffer = chars.into_iter().collect();
                typed(app);
            }
        }
        KeyCode::Char(c) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) => {
            insert_text(app, &c.to_string())
        }
        _ => {}
    }
    InputEffect::None
}

fn handle_approval_key(app: &mut App, key: KeyEvent) -> InputEffect {
    let high_risk = app
        .pending_approval
        .as_ref()
        .is_some_and(|p| p.user_approval_required);
    let choices = if high_risk { 2 } else { 3 };
    match key.code {
        KeyCode::Left | KeyCode::BackTab => {
            app.approval_selection = (app.approval_selection + choices - 1) % choices
        }
        KeyCode::Right | KeyCode::Tab => {
            app.approval_selection = (app.approval_selection + 1) % choices
        }
        KeyCode::Esc => {
            return InputEffect::RespondApproval {
                approved: false,
                remember: false,
            }
        }
        KeyCode::Enter => {
            let selection = app.approval_selection.min(choices - 1);
            return InputEffect::RespondApproval {
                approved: selection != 0,
                remember: selection == 2,
            };
        }
        KeyCode::Char('[') | KeyCode::PageUp | KeyCode::Up => {
            app.approval_scroll_offset = app.approval_scroll_offset.saturating_sub(5)
        }
        KeyCode::Char(']') | KeyCode::PageDown | KeyCode::Down => {
            app.approval_scroll_offset = app
                .approval_scroll_offset
                .saturating_add(5)
                .min(app.view.borrow().overlay_max_scroll)
        }
        _ => {}
    }
    InputEffect::None
}

fn handle_overlay_key(app: &mut App, key: KeyEvent) -> InputEffect {
    if matches!(key.code, KeyCode::Esc | KeyCode::F(1)) {
        app.show_help = false;
        let view = app.view.get_mut();
        view.show_preferences = false;
        view.copy = None;
        return InputEffect::None;
    }
    if app.view.borrow().copy.is_some() {
        let view = app.view.get_mut();
        if key.code == KeyCode::Tab {
            let count = view.copy.as_ref().map_or(1, Vec::len);
            view.copy_part = (view.copy_part + 1) % count;
            view.copy_scroll = 0;
            view.copy_notice = None;
        } else if key.code == KeyCode::Enter {
            if let Some((_, text)) = view
                .copy
                .as_ref()
                .and_then(|parts| parts.get(view.copy_part))
            {
                return InputEffect::Copy(text.clone());
            }
        }
    } else if app.view.borrow().show_preferences {
        match key.code {
            KeyCode::Char('m') => app.preferences.reduced_motion = !app.preferences.reduced_motion,
            KeyCode::Char('c') => app.preferences.plain_colors = !app.preferences.plain_colors,
            KeyCode::Char('t') => app.preferences.hide_tools = !app.preferences.hide_tools,
            KeyCode::Char('s') => interaction::toggle_sidebar(app),
            KeyCode::Char('f') => app.preferences.mouse = !app.preferences.mouse,
            KeyCode::F(2) => app.view.get_mut().show_preferences = false,
            _ => {}
        }
        app.view.get_mut().preferences_dirty = true;
    }
    let delta = match key.code {
        KeyCode::PageUp => -8,
        KeyCode::PageDown => 8,
        KeyCode::Up => -1,
        KeyCode::Down => 1,
        _ => 0,
    };
    scroll_overlay(app, delta);
    InputEffect::None
}

fn scroll_overlay(app: &mut App, delta: i32) {
    let view = app.view.get_mut();
    let scroll = if view.copy.is_some() {
        &mut view.copy_scroll
    } else {
        &mut view.help_scroll
    };
    *scroll = (i32::from(*scroll) + delta).clamp(0, i32::from(view.overlay_max_scroll)) as u16;
}

fn page_size(app: &App) -> i32 {
    let view = app.view.borrow();
    i32::from(
        if view.focus == Focus::Inspector {
            view.inspector.height
        } else {
            view.chat.height
        }
        .saturating_sub(3)
        .max(1),
    )
}
fn scroll_focused(app: &mut App, delta: i32) {
    let focus = app.view.borrow().focus;
    if focus == Focus::Inspector {
        let view = app.view.get_mut();
        let current = if app.show_activity && view.inspector_follow {
            view.inspector_max_scroll
        } else {
            app.inspector_scroll_offset
        };
        app.inspector_scroll_offset =
            (i32::from(current) + delta).clamp(0, i32::from(view.inspector_max_scroll)) as u16;
        view.inspector_follow =
            app.show_activity && app.inspector_scroll_offset == view.inspector_max_scroll;
    } else {
        interaction::scroll_chat(app, -delta);
    }
}

fn composer_is_multiline(app: &App) -> bool {
    composer::layout(
        &app.input_buffer,
        app.view.borrow().composer.width.saturating_sub(6) as usize,
    )
    .rows
    .len()
        > 1
}

pub fn handle_mouse(app: &mut App, mouse: MouseEvent) {
    if app.model_picker.is_some() && app.pending_approval.is_none() {
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                super::models::handle_key(app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
            }
            MouseEventKind::ScrollDown => {
                super::models::handle_key(app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
            }
            _ => {}
        }
        return;
    }
    let delta = match mouse.kind {
        MouseEventKind::ScrollUp => -3,
        MouseEventKind::ScrollDown => 3,
        _ => 0,
    };
    if app.pending_approval.is_some() {
        app.approval_scroll_offset = (i32::from(app.approval_scroll_offset) + delta)
            .clamp(0, i32::from(app.view.borrow().overlay_max_scroll))
            as u16;
        return;
    }
    if app.show_help || app.view.borrow().show_preferences || app.view.borrow().copy.is_some() {
        scroll_overlay(app, delta);
        return;
    }
    let position = ratatui::layout::Position::new(mouse.column, mouse.row);
    let focus = {
        let view = app.view.borrow();
        if view.inspector.contains(position) {
            Some(Focus::Inspector)
        } else if view.chat.contains(position) {
            Some(Focus::Chat)
        } else if view.composer.contains(position) {
            Some(Focus::Composer)
        } else {
            None
        }
    };
    if let Some(focus) = focus {
        if delta != 0 || mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            app.view.get_mut().focus = focus;
        }
        if delta != 0 && focus != Focus::Composer {
            scroll_focused(app, delta);
        }
    }
}

pub fn handle_paste(app: &mut App, text: &str) {
    if let Some(picker) = &mut app.model_picker {
        if let Some(auth) = &mut picker.auth_prompt {
            let clean: String = text.chars().filter(|c| !c.is_control()).collect();
            let remaining = 1024_usize.saturating_sub(auth.key_input.len());
            auth.key_input.extend(clean.chars().take(remaining));
            auth.notice = None;
            return;
        }
        if app.pending_approval.is_none() {
            if text.len() > 4096 || picker.query.len().saturating_add(text.len()) > 4096 {
                picker.notice = Some("Search limit reached; query unchanged".into());
                return;
            }
            picker
                .query
                .extend(text.chars().filter(|c| !c.is_control()));
            picker.selected = 0;
        }
        return;
    }
    if app.pending_approval.is_some()
        || app.show_help
        || app.view.borrow().show_preferences
        || app.view.borrow().copy.is_some()
    {
        return;
    }
    if text.len() > super::retention::INPUT_BYTES {
        app.status_hint = Some("Paste exceeds the 1 MiB input limit; draft unchanged".into());
        app.hint_ticks = 400;
        return;
    }
    let clean: String = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\t', "    ")
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .collect();
    app.view.get_mut().focus = Focus::Composer;
    insert_text(app, &clean);
}

fn submit(app: &mut App) -> InputEffect {
    let msg = app.input_buffer.trim().to_string();
    if msg.is_empty() {
        return InputEffect::None;
    }
    if slash::is_quit_line(&msg) {
        return InputEffect::Quit;
    }
    if app.thinking && !msg.starts_with('/') {
        app.status_hint =
            Some("Still working - draft kept. Send it when this turn finishes.".into());
        app.hint_ticks = 400;
        return InputEffect::None;
    }
    app.input_buffer.clear();
    app.input_cursor = 0;
    typed(app);
    super::retention::remember(&mut app.input_history, &msg);
    app.history_index = None;
    app.draft_before_history = None;
    if msg.starts_with('/') {
        app.layout_mode = LayoutMode::Split;
        app.view.get_mut().focus = Focus::Inspector;
    } else {
        interaction::latest(app);
    }
    InputEffect::Submit(msg)
}
fn typed(app: &mut App) {
    app.tab_prefix = None;
    app.tab_cycle = 0;
}
fn history_up(app: &mut App) {
    if app.input_history.is_empty() {
        return;
    }
    let i = match app.history_index {
        None => {
            app.draft_before_history = Some((app.input_buffer.clone(), app.input_cursor));
            app.input_history.len() - 1
        }
        Some(i) => i.saturating_sub(1),
    };
    app.history_index = Some(i);
    app.input_buffer = app.input_history[i].clone();
    app.input_cursor = app.input_buffer.chars().count();
    typed(app);
}
fn history_down(app: &mut App) {
    let Some(i) = app.history_index else {
        return;
    };
    if i + 1 < app.input_history.len() {
        app.history_index = Some(i + 1);
        app.input_buffer = app.input_history[i + 1].clone();
        app.input_cursor = app.input_buffer.chars().count();
    } else {
        app.history_index = None;
        let (text, cursor) = app.draft_before_history.take().unwrap_or_default();
        app.input_buffer = text;
        app.input_cursor = cursor;
    }
    typed(app);
}
fn insert_text(app: &mut App, text: &str) {
    if text.len() > super::retention::INPUT_BYTES.saturating_sub(app.input_buffer.len()) {
        app.status_hint = Some("Input limit reached (1 MiB); draft unchanged".into());
        app.hint_ticks = 400;
        return;
    }
    let byte = app
        .input_buffer
        .char_indices()
        .nth(app.input_cursor)
        .map_or(app.input_buffer.len(), |(i, _)| i);
    app.input_buffer.insert_str(byte, text);
    app.input_cursor += text.chars().count();
    typed(app);
}
fn move_vertical(app: &mut App, down: bool) {
    let width = app.view.borrow().composer.width.saturating_sub(6) as usize;
    app.input_cursor = composer::move_vertical(&app.input_buffer, app.input_cursor, width, down);
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;

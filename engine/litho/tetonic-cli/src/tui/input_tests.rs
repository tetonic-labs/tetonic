use super::*;
use crate::tui::transcript::{LineKind, TranscriptLine};
use crossterm::event::KeyEventKind;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn press(app: &mut App, code: KeyCode) -> InputEffect {
    let mut k = key(code);
    k.kind = KeyEventKind::Press;
    handle_key(app, k)
}

fn dummy_app() -> App {
    App::test_stub()
}

#[test]
fn oversized_paste_and_expanding_tabs_preserve_existing_draft() {
    let mut app = dummy_app();
    app.input_buffer = "keep me".into();
    app.input_cursor = 4;
    handle_paste(
        &mut app,
        &"x".repeat(crate::tui::retention::INPUT_BYTES + 1),
    );
    assert_eq!(app.input_buffer, "keep me");
    assert_eq!(app.input_cursor, 4);
    handle_paste(
        &mut app,
        &"\t".repeat(crate::tui::retention::INPUT_BYTES / 2),
    );
    assert_eq!(app.input_buffer, "keep me");
    assert_eq!(app.input_cursor, 4);
    assert!(app.status_hint.as_ref().unwrap().contains("limit"));
}

#[test]
fn typing_cannot_grow_a_full_draft_and_deletion_reopens_space() {
    let mut app = dummy_app();
    app.input_buffer = "x".repeat(crate::tui::retention::INPUT_BYTES);
    app.input_cursor = app.input_buffer.len();
    press(&mut app, KeyCode::Char('é'));
    assert_eq!(app.input_buffer.len(), crate::tui::retention::INPUT_BYTES);
    press(&mut app, KeyCode::Backspace);
    press(&mut app, KeyCode::Backspace);
    press(&mut app, KeyCode::Char('é'));
    assert_eq!(app.input_buffer.len(), crate::tui::retention::INPUT_BYTES);
    assert!(app.input_buffer.ends_with('é'));
}

#[test]
fn left_right_move_caret_not_inspector() {
    let mut app = dummy_app();
    press(&mut app, KeyCode::Char('a'));
    press(&mut app, KeyCode::Char('b'));
    press(&mut app, KeyCode::Char('c'));
    assert_eq!(app.input_buffer, "abc");
    assert_eq!(app.input_cursor, 3);
    press(&mut app, KeyCode::Left);
    press(&mut app, KeyCode::Left);
    assert_eq!(app.input_cursor, 1);
    press(&mut app, KeyCode::Char('X'));
    assert_eq!(app.input_buffer, "aXbc");
    assert_eq!(app.inspector_scroll_offset, 0);
}

#[test]
fn ctrl_c_idle_does_not_quit() {
    let mut app = dummy_app();
    let mut k = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    k.kind = KeyEventKind::Press;
    let effect = handle_key(&mut app, k);
    assert!(matches!(effect, InputEffect::None));
    assert!(!app.should_quit);
    assert!(app.status_hint.as_deref().unwrap().contains("Ctrl+Q"));
}

#[test]
fn ctrl_o_toggles_expand_thoughts() {
    let mut app = dummy_app();
    app.transcript.push(TranscriptLine::new(
        LineKind::Thought { collapsed: true },
        "reasoning",
    ));
    assert!(!app.expand_thoughts);

    let mut k = KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL);
    k.kind = KeyEventKind::Press;
    let effect = handle_key(&mut app, k);
    assert!(matches!(effect, InputEffect::None));
    assert!(app.expand_thoughts);
    assert_eq!(
        app.transcript[0].kind,
        LineKind::Thought { collapsed: false }
    );

    let effect = handle_key(&mut app, k);
    assert!(matches!(effect, InputEffect::None));
    assert!(!app.expand_thoughts);
    assert_eq!(
        app.transcript[0].kind,
        LineKind::Thought { collapsed: true }
    );
}

//! Regression tests for user work preservation, modal safety, and reading stability.
use super::{
    approval::PendingApproval,
    input::{self, InputEffect},
    interaction::{self, Focus},
    transcript::{self, LineKind, TranscriptLine},
    ui, App, LayoutMode,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::{backend::TestBackend, Terminal};

fn press(app: &mut App, code: KeyCode) -> InputEffect {
    input::handle_key(app, KeyEvent::new(code, KeyModifiers::NONE))
}
fn ctrl(app: &mut App, c: char) -> InputEffect {
    input::handle_key(app, KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
}
fn draw(app: &App, width: u16, height: u16) -> ratatui::buffer::Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| ui::draw(f, app)).unwrap();
    terminal.backend().buffer().clone()
}
fn pending(high_risk: bool) -> PendingApproval {
    PendingApproval {
        session_id: "s".into(),
        approval_id: "a".into(),
        kind: "run_shell".into(),
        detail: "cargo test\n".repeat(100),
        missing_controls: vec![],
        user_approval_required: high_risk,
    }
}

#[test]
fn busy_submit_preserves_draft_caret_and_history_until_accepted() {
    let mut app = App::test_stub();
    app.begin_turn();
    input::handle_paste(&mut app, "follow up\nwith more details");
    let draft = app.input_buffer.clone();
    app.input_cursor = 4;
    assert!(matches!(press(&mut app, KeyCode::Enter), InputEffect::None));
    assert_eq!(app.input_buffer, draft);
    assert_eq!(app.input_cursor, 4);
    assert!(app.input_history.is_empty());
    app.finish_turn_ok();
    assert!(matches!(press(&mut app, KeyCode::Enter), InputEffect::Submit(text) if text == draft));
    assert!(app.input_buffer.is_empty());
}

#[test]
fn history_round_trip_restores_multiline_draft_and_caret() {
    let mut app = App::test_stub();
    app.input_history = vec!["oldest".into(), "newest".into()];
    app.input_buffer = "my\ndraft".into();
    app.input_cursor = 2;
    for _ in 0..2 {
        input::handle_key(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::ALT));
    }
    assert_eq!(app.input_buffer, "oldest");
    for _ in 0..2 {
        input::handle_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
    }
    assert_eq!(app.input_buffer, "my\ndraft");
    assert_eq!(app.input_cursor, 2);
}

#[test]
fn paste_is_text_and_ctrl_j_adds_newline_without_submission() {
    let mut app = App::test_stub();
    input::handle_paste(&mut app, "one\r\ntwo\tthree\u{1b}");
    assert_eq!(app.input_buffer, "one\ntwo    three");
    assert!(matches!(ctrl(&mut app, 'j'), InputEffect::None));
    assert!(app.input_buffer.ends_with('\n'));
    assert!(app.input_history.is_empty());
    let draft = app.input_buffer.clone();
    app.pending_approval = Some(pending(false));
    input::handle_paste(&mut app, "yes\ny\n");
    assert_eq!(app.input_buffer, draft);
}

#[test]
fn approval_ignores_typing_defaults_to_deny_and_requires_navigation() {
    let mut app = App::test_stub();
    app.input_buffer = "draft".into();
    app.pending_approval = Some(pending(false));
    for c in ['y', 'Y', 'a', 'A', 'n'] {
        assert!(matches!(
            press(&mut app, KeyCode::Char(c)),
            InputEffect::None
        ));
    }
    assert!(matches!(
        press(&mut app, KeyCode::Enter),
        InputEffect::RespondApproval {
            approved: false,
            remember: false
        }
    ));
    press(&mut app, KeyCode::Right);
    assert!(matches!(
        press(&mut app, KeyCode::Enter),
        InputEffect::RespondApproval {
            approved: true,
            remember: false
        }
    ));
    press(&mut app, KeyCode::Right);
    assert!(matches!(
        press(&mut app, KeyCode::Enter),
        InputEffect::RespondApproval {
            approved: true,
            remember: true
        }
    ));
    assert!(matches!(
        press(&mut app, KeyCode::Esc),
        InputEffect::RespondApproval {
            approved: false,
            remember: false
        }
    ));
    assert_eq!(app.input_buffer, "draft");
    app.pending_approval = Some(pending(true));
    app.approval_selection = 0;
    for _ in 0..6 {
        press(&mut app, KeyCode::Right);
        assert!(!matches!(
            press(&mut app, KeyCode::Enter),
            InputEffect::RespondApproval { remember: true, .. }
        ));
    }
}

#[test]
fn reading_view_stays_still_as_streaming_output_arrives() {
    let mut app = App::test_stub();
    app.transcript.push(TranscriptLine::new(
        LineKind::Lokai,
        (0..80).map(|i| format!("Row {i}\n")).collect::<String>(),
    ));
    draw(&app, 80, 24);
    press(&mut app, KeyCode::PageUp);
    let before = draw(&app, 80, 24);
    transcript::append_token(&mut app.transcript, "More output\n".repeat(20).as_str());
    let after = draw(&app, 80, 24);
    for y in 4..18 {
        for x in 1..79 {
            assert_eq!(before[(x, y)], after[(x, y)]);
        }
    }
    let header: String = (0..80).map(|x| after[(x, 3)].symbol()).collect();
    assert!(header.contains("New output below"));
    interaction::latest(&mut app);
    assert!(app.reading.is_none());
    assert_ne!(draw(&app, 80, 24), before);
}

#[test]
fn mouse_scroll_targets_hovered_pane_and_modal_without_touching_chat() {
    let mut app = App::test_stub();
    app.layout_mode = LayoutMode::Split;
    app.inspector_text = "details\n".repeat(100);
    draw(&app, 120, 24);
    let inspector = app.view.borrow().inspector;
    let mouse = MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: inspector.x + 2,
        row: inspector.y + 2,
        modifiers: KeyModifiers::NONE,
    };
    input::handle_mouse(&mut app, mouse);
    assert_eq!(app.inspector_scroll_offset, 3);
    assert_eq!(app.chat_scroll_offset, 0);
    assert_eq!(app.view.borrow().focus, Focus::Inspector);
    app.pending_approval = Some(pending(false));
    draw(&app, 120, 24);
    input::handle_mouse(&mut app, mouse);
    assert_eq!(app.approval_scroll_offset, 3);
    assert_eq!(app.inspector_scroll_offset, 3);
}

#[test]
fn narrow_focus_navigation_can_reach_sidebar_and_restore_composer() {
    let mut app = App::test_stub();
    draw(&app, 60, 20);
    ctrl(&mut app, 'e');
    draw(&app, 60, 20);
    assert!(app.view.borrow().inspector.width > 0);
    assert_eq!(app.view.borrow().chat.width, 0);
    press(&mut app, KeyCode::Esc);
    draw(&app, 60, 20);
    assert!(app.view.borrow().chat.width > 0);
    ctrl(&mut app, 'e');
    assert_eq!(app.layout_mode, LayoutMode::FullChat);
}

#[test]
fn help_scrolls_on_short_screens_and_plain_colors_have_no_rgb() {
    let mut app = App::test_stub();
    app.preferences.plain_colors = true;
    press(&mut app, KeyCode::F(1));
    let before = draw(&app, 60, 12);
    press(&mut app, KeyCode::PageDown);
    let after = draw(&app, 60, 12);
    assert_ne!(before, after);
    assert!(after
        .content
        .iter()
        .all(|c| c.fg == ratatui::style::Color::Reset && c.bg == ratatui::style::Color::Reset));
}

#[test]
fn copy_view_copies_original_answer_and_separate_code_without_touching_draft() {
    let mut app = App::test_stub();
    app.input_buffer = "draft".into();
    let answer = "## Answer\n```rust\nlet n = 1;\n```";
    app.transcript
        .push(TranscriptLine::new(LineKind::Lokai, answer));
    draw(&app, 80, 24);
    press(&mut app, KeyCode::F(3));
    assert!(matches!(press(&mut app, KeyCode::Enter), InputEffect::Copy(text) if text == answer));
    press(&mut app, KeyCode::Tab);
    assert!(
        matches!(press(&mut app, KeyCode::Enter), InputEffect::Copy(text) if text == "let n = 1;\n")
    );
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.input_buffer, "draft");
}

#[test]
fn multiline_editor_moves_caret_and_completion_is_recorded_once() {
    let mut app = App::test_stub();
    input::handle_paste(&mut app, "first\nsecond\nthird");
    draw(&app, 80, 24);
    press(&mut app, KeyCode::Up);
    assert_eq!(app.input_cursor, 11);
    press(&mut app, KeyCode::Home);
    assert_eq!(app.input_cursor, 6);
    app.begin_turn();
    app.finish_turn_ok();
    app.finish_turn_ok();
    assert_eq!(
        app.transcript
            .iter()
            .filter(|e| e.text.starts_with("Completed"))
            .count(),
        1
    );
}
#[test]
fn slash_reports_open_context_without_forgetting_sidebar_toggle() {
    let mut app = App::test_stub();
    input::handle_paste(&mut app, "/status");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.view.borrow().focus, Focus::Inspector);
    assert_eq!(app.layout_mode, LayoutMode::Split);
    ctrl(&mut app, 'e');
    assert_eq!(app.layout_mode, LayoutMode::FullChat);
}

#[test]
fn composer_exact_width_newlines_do_not_insert_blank_rows() {
    let layout = super::composer::layout("abcd\nefgh\n", 4);
    assert_eq!(layout.rows, vec!["abcd", "efgh", ""]);
}

#[test]
fn reduced_motion_and_tiny_modal_layouts_remain_usable_without_panics() {
    let mut app = App::test_stub();
    app.begin_turn();
    app.preferences.reduced_motion = true;
    let buffer = draw(&app, 80, 24);
    let status: String = (0..80).map(|x| buffer[(x, 19)].symbol()).collect();
    assert!(!status.starts_with('['));
    for (width, height) in [(1, 1), (10, 4), (40, 12)] {
        app.show_help = true;
        draw(&app, width, height);
        app.show_help = false;
        app.view.get_mut().show_preferences = true;
        draw(&app, width, height);
        app.view.get_mut().show_preferences = false;
        app.pending_approval = Some(pending(true));
        draw(&app, width, height);
        app.pending_approval = None;
    }
}

#[test]
#[ignore = "manual visual review: export actual TUI cell buffers"]
fn export_ux_frames() {
    let dir = std::env::var("LOKAI_TUI_PREVIEW_DIR").unwrap();
    for name in [
        "reader",
        "editor",
        "approval",
        "help",
        "preferences",
        "copy",
        "history",
        "plain",
    ] {
        let mut app = App::test_stub();
        app.model = "local model".into();
        app.workspace_root = "~/projects/lokai".into();
        app.transcript.push(TranscriptLine::new(
            LineKind::You,
            "Help me improve the terminal experience.",
        ));
        app.transcript.push(TranscriptLine::new(LineKind::Lokai, "## A calmer workspace\nThe conversation comes first. Your drafts and reading position stay safe while Lokai works.\n\n- **Compose naturally** with multiple lines and safe paste\n- **Stay in control** with deliberate approval decisions\n- **Keep your place** while new output arrives\n\n```rust\nfn main() {\n    println!(\"Hello, Lokai!\");\n}\n```\n\nUse `F3` to copy this answer or its code. Open `F2` for presentation preferences."));
        app.finish_turn_ok();
        match name {
            "editor" => {
                app.begin_turn();
                input::handle_paste(&mut app, "Please add a regression test for:\n- Draft preservation during generation\n- Navigating history without losing my text\n\nKeep the existing keyboard shortcuts where possible.");
            }
            "approval" => {
                input::handle_paste(&mut app, "An unfinished draft stays here.");
                app.pending_approval = Some(PendingApproval {
                    detail:
                        "cargo test -p lokai-cli\n\nRun the affected CLI tests in the workspace."
                            .into(),
                    ..pending(false)
                });
            }
            "help" => {
                app.show_help = true;
            }
            "preferences" => {
                app.view.get_mut().show_preferences = true;
            }
            "copy" => interaction::open_copy(&mut app),
            "history" => {
                app.transcript.push(TranscriptLine::new(
                    LineKind::Lokai,
                    "More context\n".repeat(30),
                ));
                draw(&app, 120, 32);
                press(&mut app, KeyCode::PageUp);
                app.transcript.push(TranscriptLine::new(
                    LineKind::Lokai,
                    "New output after the reading view was paused.",
                ));
            }
            "plain" => app.preferences.plain_colors = true,
            _ => {}
        }
        let (width, height) = if name == "help" { (72, 16) } else { (120, 32) };
        let buffer = draw(&app, width, height);
        let cells: Vec<_> = buffer.content.iter().map(|cell| serde_json::json!({"s": cell.symbol(), "fg": format!("{:?}",cell.fg), "bg": format!("{:?}",cell.bg), "bold": cell.modifier.contains(ratatui::style::Modifier::BOLD)})).collect();
        std::fs::write(
            std::path::Path::new(&dir).join(format!("ux-{name}.json")),
            serde_json::json!({"width":width,"height":height,"cells":cells}).to_string(),
        )
        .unwrap();
    }
}

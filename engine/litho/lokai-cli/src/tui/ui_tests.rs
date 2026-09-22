use super::super::transcript::TranscriptLine;
use super::*;
use ratatui::style::Modifier;
use ratatui::{backend::TestBackend, Terminal};

fn screen(app: &App, width: u16, height: u16) -> (String, (u16, u16)) {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| draw(f, app)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for y in 0..height {
        for x in 0..width {
            text.push_str(buffer[(x, y)].symbol());
        }
        text.push('\n');
    }
    let cursor = terminal.get_cursor_position().unwrap();
    (text, (cursor.x, cursor.y))
}

#[test]
fn layout_cache_reuses_unchanged_content_and_invalidates_visible_changes() {
    let mut app = App::test_stub();
    app.push_transcript(TranscriptLine::new(
        LineKind::Lokai,
        "# Heading\n\nAnswer with **emphasis** and a long wrapped line.",
    ));
    let initial = screen(&app, 80, 24);
    let cached = app.view.borrow().chat_layout.clone().unwrap();
    assert_eq!(screen(&app, 80, 24), initial);
    assert!(std::rc::Rc::ptr_eq(
        &cached,
        app.view.borrow().chat_layout.as_ref().unwrap()
    ));
    // Height affects the viewport, not text wrapping.
    screen(&app, 80, 30);
    assert!(std::rc::Rc::ptr_eq(
        &cached,
        app.view.borrow().chat_layout.as_ref().unwrap()
    ));
    screen(&app, 60, 24);
    assert!(!std::rc::Rc::ptr_eq(
        &cached,
        app.view.borrow().chat_layout.as_ref().unwrap()
    ));
    app.transcript[0].text.push_str("\nNew output");
    let updated = screen(&app, 60, 24);
    app.view.get_mut().chat_layout = None;
    assert_eq!(screen(&app, 60, 24), updated);
    let before_filter = app.view.borrow().chat_layout.clone().unwrap();
    app.preferences.hide_tools = !app.preferences.hide_tools;
    screen(&app, 60, 24);
    assert!(!std::rc::Rc::ptr_eq(
        &before_filter,
        app.view.borrow().chat_layout.as_ref().unwrap()
    ));
}

#[test]
fn frozen_reading_layout_survives_new_live_output() {
    let mut app = App::test_stub();
    app.push_transcript(TranscriptLine::new(LineKind::Lokai, "Reading this answer"));
    app.reading = Some(app.transcript.clone());
    screen(&app, 80, 24);
    let cached = app.view.borrow().chat_layout.clone().unwrap();
    app.push_transcript(TranscriptLine::new(LineKind::Lokai, "New live answer"));
    let (view, _) = screen(&app, 80, 24);
    assert!(view.contains("New output below"));
    assert!(view.contains("Reading this answer"));
    assert!(!view.contains("New live answer"));
    assert!(std::rc::Rc::ptr_eq(
        &cached,
        app.view.borrow().chat_layout.as_ref().unwrap()
    ));
    app.reading = None;
    assert!(screen(&app, 80, 24).0.contains("New live answer"));
    assert!(!std::rc::Rc::ptr_eq(
        &cached,
        app.view.borrow().chat_layout.as_ref().unwrap()
    ));
}

#[test]
fn welcome_and_responsive_inspector() {
    let mut app = App::test_stub();
    let (default_view, _) = screen(&app, 120, 32);
    assert!(!default_view.contains(" Context "));
    app.layout_mode = LayoutMode::Split;
    let (wide, _) = screen(&app, 120, 32);
    assert!(wide.contains("Make room for your next idea."));
    assert!(wide.contains(" Context "));
    let (narrow, _) = screen(&app, 72, 24);
    assert!(!narrow.contains(" Context "));
    assert!(narrow.contains("Conversation"));
}

#[test]
fn long_unicode_prompt_keeps_caret_inside_at_every_position() {
    let mut app = App::test_stub();
    app.input_buffer = "ab界🙂e\u{301}".repeat(30);
    for cursor in 0..=app.input_buffer.chars().count() {
        app.input_cursor = cursor;
        let (_, (x, y)) = screen(&app, 40, 18);
        assert!((4..38).contains(&x), "cursor {cursor}: {x}");
        assert!(y < 18);
    }
}

#[test]
fn wrapped_history_matches_unculled_render() {
    let lines: Vec<_> = (0..180)
        .map(|i| {
            Line::from(format!(
                "{i}: words and wide 界 characters wrap across several rows"
            ))
        })
        .collect();
    for width in [12, 31, 60] {
        for offset in [0, 5, 50, 1000, u16::MAX] {
            let mut expected = Terminal::new(TestBackend::new(width, 10)).unwrap();
            let paragraph = Paragraph::new(lines.clone()).wrap(Wrap { trim: false });
            let scroll = paragraph
                .line_count(width)
                .saturating_sub(10)
                .saturating_sub(offset as usize);
            expected
                .draw(|f| f.render_widget(paragraph.scroll((scroll as u16, 0)), f.area()))
                .unwrap();
            let mut actual = Terminal::new(TestBackend::new(width, 10)).unwrap();
            actual
                .draw(|f| render_scrolled(f, lines.clone(), f.area(), offset))
                .unwrap();
            assert_eq!(
                actual.backend().buffer(),
                expected.backend().buffer(),
                "width {width}, offset {offset}"
            );
        }
    }
}

#[test]
fn all_layouts_survive_small_terminal_and_resize() {
    let mut app = App::test_stub();
    app.transcript
        .push(TranscriptLine::new(LineKind::You, "A question"));
    app.transcript
        .push(TranscriptLine::new(LineKind::Lokai, "An answer"));
    for mode in [
        LayoutMode::Split,
        LayoutMode::FullChat,
        LayoutMode::FullInspector,
    ] {
        app.layout_mode = mode;
        for (width, height) in [(1, 1), (10, 4), (40, 12), (80, 24), (160, 40)] {
            screen(&app, width, height);
        }
    }
}

#[test]
fn approval_keeps_decision_keys_visible_and_draft_frozen() {
    let mut app = App::test_stub();
    app.input_buffer = "draft".into();
    app.pending_approval = Some(super::super::approval::PendingApproval {
        session_id: "s".into(),
        approval_id: "a".into(),
        kind: "run_shell".into(),
        detail: "command detail ".repeat(200),
        missing_controls: vec![],
        user_approval_required: true,
    });
    let (text, _) = screen(&app, 80, 24);
    assert!(text.contains("Approve once"));
    assert!(text.contains("Esc deny"));
    assert!(!text.contains("Always allow pattern"));
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    app.inspector_scroll_offset = 20;
    super::super::input::handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE),
    );
    assert_eq!(app.approval_scroll_offset, 5);
    assert_eq!(app.inspector_scroll_offset, 20);
    super::super::input::handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE),
    );
    assert_eq!(app.approval_scroll_offset, 0);
}

#[test]
#[ignore = "manual design review: writes plain terminal frames to a requested directory"]
fn export_design_frames() {
    let dir = std::env::var("LOKAI_TUI_PREVIEW_DIR").unwrap();
    let mut app = App::test_stub();
    app.model = "local model".into();
    app.workspace_root = "~/projects/lokai".into();
    let names = ["welcome", "conversation", "compact"]
        .map(str::to_owned)
        .into_iter()
        .chain((0..18).map(|i| format!("loading{i:02}")));
    for name in names {
        if let Some(frame) = name.strip_prefix("loading") {
            app.thinking = true;
            app.phase = TurnPhase::Generating;
            app.turn_start = Some(
                std::time::Instant::now()
                    - std::time::Duration::from_millis(frame.parse::<u64>().unwrap() * 100),
            );
        }
        if name == "conversation" {
            app.transcript.push(TranscriptLine::new(
                LineKind::You,
                "Help me improve the terminal experience.",
            ));
            app.transcript.push(TranscriptLine::new(LineKind::Lokai, "## A clearer terminal workspace\nThe output now supports **readable Markdown**, with a warmer palette.\n\n- **Headings** give responses structure\n- Inline code like `src/tui/ui.rs` stands apart\n\n```rust\nfn main() {\n    println!(\"Hello, Lokai!\");\n}\n```\n\n| Change | Result |\n| --- | --- |\n| Agent output | Formatted as it streams |\n| Narrow terminals | Tables become labeled rows |"));
            app.transcript.push(TranscriptLine::new(
                LineKind::Tool,
                "read_file src/tui/ui.rs ok",
            ));
            app.inspector_text = "Layout review\n\nConversation first\nAdaptive context panel\nVisible keyboard controls\n\nReady for the next change.".into();
        }
        let (text, _) = screen(&app, if name == "compact" { 72 } else { 120 }, 32);
        let width = if name == "compact" { 72 } else { 120 };
        let mut terminal = Terminal::new(TestBackend::new(width, 32)).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();
        let cells: Vec<_> = terminal.backend().buffer().content.iter().map(|cell| {
            serde_json::json!({"s": cell.symbol(), "fg": format!("{:?}", cell.fg), "bg": format!("{:?}", cell.bg), "bold": cell.modifier.contains(Modifier::BOLD)})
        }).collect();
        std::fs::write(
            std::path::Path::new(&dir).join(format!("{name}.json")),
            serde_json::to_string(
                &serde_json::json!({"width": width, "height": 32, "cells": cells}),
            )
            .unwrap(),
        )
        .unwrap();
        std::fs::write(std::path::Path::new(&dir).join(format!("{name}.txt")), text).unwrap();
    }
}

#[test]
fn keyboard_help_preserves_draft_and_artifact_and_closes_with_escape() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = App::test_stub();
    app.input_buffer = "draft".into();
    app.inspector_text = "important report".into();
    super::super::input::handle_key(&mut app, KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
    assert!(app.show_help);
    assert!(screen(&app, 100, 32).0.contains("Ctrl+C cancel"));
    super::super::input::handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
    );
    super::super::input::handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!app.show_help);
    assert_eq!(app.input_buffer, "draft");
    assert_eq!(app.inspector_text, "important report");
    app.chat_scroll_offset = 100;
    super::super::input::handle_key(&mut app, KeyEvent::new(KeyCode::End, KeyModifiers::CONTROL));
    assert_eq!(app.chat_scroll_offset, 0);
}

#[test]
fn agent_markdown_is_rendered_but_user_input_stays_literal() {
    let mut app = App::test_stub();
    app.transcript.push(TranscriptLine::new(
        LineKind::You,
        "Keep **my input** literal",
    ));
    app.transcript.push(TranscriptLine::new(
        LineKind::Lokai,
        "## Summary\nUse **formatted output** and `code`.",
    ));
    let (text, _) = screen(&app, 120, 32);
    assert!(text.contains("Keep **my input** literal"));
    assert!(text.contains("Summary"));
    assert!(text.contains("Use formatted output and code."));
    assert!(!text.contains("## Summary"));
}

#[test]
fn scope_has_stable_width_and_a_smooth_repeatable_cycle() {
    use std::time::Duration;
    let first = oscilloscope(Duration::ZERO, 12);
    let later = oscilloscope(Duration::from_millis(400), 12);
    let cycle = oscilloscope(Duration::from_millis(1800), 12);
    assert!(first.iter().all(|span| span.content.is_ascii()));
    assert_ne!(first, later);
    assert_eq!(first, cycle);
    for time in (0..1800).step_by(50) {
        for width in [6, 12] {
            assert_eq!(
                Line::from(oscilloscope(Duration::from_millis(time), width)).width(),
                width + 2
            );
        }
    }
}

#[test]
fn scope_only_runs_during_work_and_keeps_phase_readable() {
    let mut app = App::test_stub();
    app.begin_turn();
    let has_wave = |text: &str| {
        text.lines()
            .any(|line| line.starts_with('[') && line.contains(']') && line.contains("Generating"))
    };
    assert!(has_wave(&screen(&app, 100, 24).0));
    assert!(screen(&app, 40, 18).0.contains("Generating"));
    assert!(!has_wave(&screen(&app, 30, 18).0));
    app.phase = TurnPhase::WaitingApproval;
    assert!(!has_wave(&screen(&app, 100, 24).0));
    app.finish_turn_ok();
    assert!(!has_wave(&screen(&app, 100, 24).0));
}

#[test]
fn role_bands_cover_wrapped_rows_and_follow_history_scroll() {
    let mut app = App::test_stub();
    app.transcript
        .push(TranscriptLine::new(LineKind::You, "user words ".repeat(30)));
    app.transcript.push(TranscriptLine::new(
        LineKind::Lokai,
        "assistant words ".repeat(30),
    ));
    for width in [40, 80, 120] {
        for offset in [0, 3, u16::MAX] {
            app.chat_scroll_offset = offset;
            let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
            terminal.draw(|f| draw(f, &app)).unwrap();
            let buffer = terminal.backend().buffer();
            let mut band_rows = 0;
            for y in 4..18 {
                let band = buffer[(2, y)].bg;
                if band == ACCENT || band == response_theme::ACCENT {
                    band_rows += 1;
                    assert_eq!(buffer[(2, y)].symbol(), "|");
                    assert_eq!(
                        buffer[(3, y)].bg,
                        if band == response_theme::ACCENT {
                            response_theme::BG
                        } else {
                            PANEL
                        }
                    );
                    assert_eq!(
                        buffer[(width.min(30) - 1, y)].bg,
                        if band == response_theme::ACCENT {
                            response_theme::BG
                        } else {
                            PANEL
                        }
                    );
                }
                let row: String = (4..width.min(30))
                    .map(|x| buffer[(x, y)].symbol())
                    .collect();
                if row.contains("user words") {
                    assert_eq!(band, ACCENT);
                }
                if row.contains("assistant words") {
                    assert_eq!(band, response_theme::ACCENT);
                }
            }
            assert!(band_rows > 2);
        }
    }
}

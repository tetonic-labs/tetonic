//! Scrollable, keyboard-driven overlays. Decision controls stay outside scrolling content.
use super::ui::{panel, ACCENT, BG, GOLD, MUTED};
use super::{approval, status, App};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Wrap},
};

pub fn render(f: &mut ratatui::Frame, app: &App) {
    if app.model_picker.is_some() && app.pending_approval.is_none() {
        super::models::render(f, app);
        return;
    }
    let view = app.view.borrow();
    if app.pending_approval.is_none()
        && !app.show_help
        && !view.show_preferences
        && view.copy.is_none()
    {
        return;
    }
    let area = f.area();
    let width = area.width.min(96);
    let height = if let Some(pending) = &app.pending_approval {
        let content_rows = Paragraph::new(approval::dialog_text(pending))
            .wrap(Wrap { trim: false })
            .line_count(width.saturating_sub(4));
        (content_rows.saturating_add(9).min(u16::MAX as usize) as u16).min(area.height)
    } else {
        area.height
    };
    let area = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let is_copy = view.copy.is_some();
    let is_preferences = view.show_preferences;
    drop(view);
    for cell in &mut f.buffer_mut().content {
        cell.set_style(
            Style::default()
                .fg(Color::Rgb(105, 103, 116))
                .bg(BG)
                .remove_modifier(Modifier::BOLD),
        );
    }
    f.render_widget(Clear, area);
    if let Some(pending) = &app.pending_approval {
        let title = if app.queued_approvals.is_empty() {
            " Approval - draft preserved ".to_string()
        } else {
            format!(" Approval - {} more waiting ", app.queued_approvals.len())
        };
        let block = panel(title).border_style(Style::default().fg(GOLD));
        let inner = block.inner(area);
        f.render_widget(block, area);
        let mut labels = vec!["Deny", "Approve once"];
        if !pending.user_approval_required {
            labels.push("Always allow pattern");
        }
        let mut spans = Vec::new();
        for (i, label) in labels.iter().enumerate() {
            spans.push(Span::styled(
                if app.approval_selection == i {
                    format!("> {label} <")
                } else {
                    format!(" {label} ")
                },
                if app.approval_selection == i {
                    Style::default().fg(BG).bg(GOLD).bold()
                } else {
                    Style::default().fg(MUTED)
                },
            ));
            spans.push(Span::raw(" "));
        }
        let mut footer_lines = vec![
            Line::from(spans),
            Line::from("Left/Right select - Enter confirm - Esc deny"),
            Line::from("Up/Down or PgUp/PgDn scroll details"),
        ];
        if let Some(error) = app
            .status_hint
            .as_ref()
            .filter(|hint| hint.starts_with("Approval response failed:"))
        {
            footer_lines.push(Line::from(error.clone()));
        }
        let footer = Paragraph::new(footer_lines).wrap(Wrap { trim: false });
        let footer_height = footer.line_count(inner.width).min(u16::MAX as usize) as u16;
        let regions =
            Layout::vertical([Constraint::Min(0), Constraint::Length(footer_height)]).split(inner);
        scroll_body(
            f,
            app,
            approval::dialog_text(pending),
            regions[0],
            app.approval_scroll_offset,
        );
        f.render_widget(footer, regions[1]);
        return;
    }
    let view = app.view.borrow();
    let (title, body, footer, scroll) = if is_copy {
        let (label, text) = view
            .copy
            .as_ref()
            .and_then(|p| p.get(view.copy_part))
            .cloned()
            .unwrap_or_default();
        (format!(" Copy - {label} "), text,
         view.copy_notice.clone().unwrap_or_else(|| "Enter: request terminal clipboard copy. Tab: answer / code blocks. PgUp/PgDn scroll. Mouse selection is enabled; terminal Copy works as a fallback. Esc closes.".into()), view.copy_scroll)
    } else if is_preferences {
        let on = |v| if v { "on" } else { "off" };
        (" Preferences - saved on this computer ".into(),
         format!("Presentation\n\n[M] Reduced motion: {}\n[C] Terminal colors (no RGB): {}\n[T] Collapse tool details: {}\n[S] Sidebar: {}\n[F] App mouse handling: {}\n\nTurn app mouse handling off to select text with your terminal.\n\nTerminal font and text size are controlled by your terminal settings.\n\nNO_COLOR enables terminal colors at startup.", on(app.preferences.reduced_motion), on(app.preferences.plain_colors), on(app.preferences.hide_tools), on(app.preferences.sidebar), on(app.preferences.mouse)),
         "Press a letter to toggle. Up/Down or PgUp/PgDn scroll. Esc closes.".into(), view.help_scroll)
    } else {
        (" Keyboard help ".into(), format!("Write\nEnter sends (busy drafts are kept)\nCtrl+J or Alt+Enter inserts a newline\nShift+Enter also works in supporting terminals\nPaste keeps line breaks without submitting\nAlt+Up / Alt+Down browses prompt history\nUp / Down moves through multiline drafts\n\nNavigate\nTab / Shift+Tab changes focus\nEsc returns to the composer\nPgUp / PgDn scrolls the focused pane\nCtrl+P / Ctrl+N previous / next turn\nCtrl+End returns to live output\nCtrl+E shows / hides the sidebar\nF6 expands / restores the inspector\nF5 chooses a model (type to search)\n\n{}\n\nCopy and preferences\nF3 opens the visible answer and code for copying\nF4 toggles app mouse handling for native selection\nF2 opens preferences (motion, colors, tools, sidebar)\nF1 opens / closes this help\n\nApproval\nLeft / Right selects; Enter confirms\nDeny is selected initially; Esc always denies\nTyping letters and pasting never approves", status::KEY_CHROME.replace(" · ", "\n")),
         "Up/Down or PgUp/PgDn scroll - Esc / F1 closes".into(), view.help_scroll)
    };
    drop(view);
    let block = panel(title).border_style(Style::default().fg(ACCENT));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let footer_rows = Paragraph::new(footer.clone())
        .wrap(Wrap { trim: false })
        .line_count(inner.width)
        .min(5) as u16;
    let regions =
        Layout::vertical([Constraint::Min(0), Constraint::Length(footer_rows)]).split(inner);
    scroll_body(f, app, body, regions[0], scroll);
    f.render_widget(
        Paragraph::new(footer)
            .style(Style::default().fg(MUTED))
            .wrap(Wrap { trim: false }),
        regions[1],
    );
}

fn scroll_body(f: &mut ratatui::Frame, app: &App, text: String, area: Rect, scroll: u16) {
    let paragraph = Paragraph::new(text).wrap(Wrap { trim: false });
    let max = paragraph
        .line_count(area.width)
        .saturating_sub(area.height as usize)
        .min(u16::MAX as usize) as u16;
    app.view.borrow_mut().overlay_max_scroll = max;
    f.render_widget(paragraph.scroll((scroll.min(max), 0)), area);
}

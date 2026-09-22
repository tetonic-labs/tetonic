//! Lokai's terminal workspace: conversation first, context when there is room.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph, Wrap};

use super::slash;
use super::status::{self, Health, TurnPhase};
use super::transcript::LineKind;
use super::{composer, interaction::Focus, overlays, App, LayoutMode};

#[path = "markdown.rs"]
mod markdown;
#[path = "response_theme.rs"]
mod response_theme;

pub(super) const BG: Color = Color::Rgb(18, 17, 21);
pub(super) const PANEL: Color = Color::Rgb(27, 25, 31);
pub(super) const EDGE: Color = Color::Rgb(94, 84, 107);
pub(super) const TEXT: Color = Color::Rgb(245, 242, 247);
pub(super) const MUTED: Color = Color::Rgb(176, 170, 188);
pub(super) const ACCENT: Color = Color::Rgb(255, 173, 66);
pub(super) const GOLD: Color = Color::Rgb(255, 217, 112);
const LAVENDER: Color = Color::Rgb(205, 160, 255);
const RED: Color = Color::Rgb(255, 119, 139);

pub(super) fn panel(title: impl Into<Line<'static>>) -> Block<'static> {
    Block::default()
        .title(title)
        .title_style(Style::default().fg(MUTED).bold())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(EDGE))
        .style(Style::default().fg(TEXT).bg(PANEL))
        .padding(Padding::horizontal(1))
}

pub fn draw(f: &mut ratatui::Frame, app: &App) {
    let area = f.area();
    f.render_widget(
        Block::default().style(Style::default().bg(BG).fg(TEXT)),
        area,
    );
    app.view.borrow_mut().terminal_width = area.width;
    let prompt_rows =
        composer::layout(&app.input_buffer, usize::from(area.width.saturating_sub(6)))
            .rows
            .len();
    let prompt_height = (prompt_rows.min(6) as u16).clamp(1, (area.height / 3).clamp(1, 6)) + 2;
    let chunks = Layout::vertical([
        Constraint::Length(if area.height < 18 { 2 } else { 3 }),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(prompt_height),
        Constraint::Length(1),
    ])
    .split(area);
    app.view.borrow_mut().composer = chunks[3];
    render_title(f, app, chunks[0]);
    render_main(f, app, chunks[1]);
    render_status(f, app, chunks[2]);
    render_prompt(f, app, chunks[3]);
    render_keys(f, app, chunks[4]);
    overlays::render(f, app);
    if app.preferences.plain_colors {
        for cell in &mut f.buffer_mut().content {
            cell.set_fg(Color::Reset).set_bg(Color::Reset);
        }
    }
}

fn render_title(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let h = current_health(app);
    let title = Line::from(vec![
        Span::styled(" L / ", Style::default().fg(BG).bg(ACCENT).bold()),
        Span::styled(" LOKAI  ", Style::default().fg(ACCENT).bold()),
        Span::styled(
            status::display_workspace(&app.workspace_root),
            Style::default().fg(TEXT),
        ),
    ]);
    let detail = Line::from(vec![
        Span::styled(
            format!(" {} ", status::health_label(h)),
            Style::default().fg(health_color(h)).bold(),
        ),
        Span::styled(format!(" · {}", app.model), Style::default().fg(MUTED)),
    ]);
    f.render_widget(Paragraph::new(vec![title, detail]), area);
}

fn render_main(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let split = app.layout_mode == LayoutMode::Split && area.width >= 100;
    let regions = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(if split {
            vec![
                Constraint::Min(60),
                Constraint::Length((area.width / 3).clamp(30, 44)),
            ]
        } else {
            vec![Constraint::Percentage(100), Constraint::Length(0)]
        })
        .split(area);
    let inspector_only = app.layout_mode == LayoutMode::FullInspector
        || (!split
            && app.layout_mode == LayoutMode::Split
            && app.view.borrow().focus == Focus::Inspector);
    {
        let mut view = app.view.borrow_mut();
        view.chat = if inspector_only {
            Rect::default()
        } else {
            regions[0]
        };
        view.inspector = if inspector_only {
            regions[0]
        } else {
            regions[1]
        };
    }
    if inspector_only {
        render_inspector(f, app, regions[0]);
    } else {
        render_chat(f, app, regions[0]);
        if split {
            render_inspector(f, app, regions[1]);
        }
    }
}

fn render_chat(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let title = if app
        .reading
        .as_ref()
        .is_some_and(|snapshot| snapshot != &app.transcript)
    {
        " New output below - Ctrl+End live "
    } else if app.chat_scroll_offset > 0 {
        " History - Ctrl+End live "
    } else {
        " Conversation "
    };
    let title = if app.view.borrow().focus == Focus::Chat {
        format!("{title}[focus] ")
    } else {
        title.to_owned()
    };
    let block = panel(title).border_style(Style::default().fg(
        if app.view.borrow().focus == Focus::Chat {
            ACCENT
        } else {
            EDGE
        },
    ));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let transcript = app.reading.as_deref().unwrap_or(&app.transcript);
    if transcript.is_empty() {
        let lines = vec![
            Line::from(Span::styled(
                "L / LOKAI",
                Style::default().fg(ACCENT).bold(),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Make room for your next idea.",
                Style::default().fg(TEXT).bold(),
            )),
            Line::from("Explore a codebase. Work through a problem. Build something."),
            Line::from(""),
            Line::from(Span::styled(
                "START WITH A QUESTION",
                Style::default().fg(ACCENT),
            )),
            Line::from("Explain how this project fits together"),
            Line::from("Find the cause of a failing test"),
            Line::from("Help me plan a change"),
            Line::from(""),
            Line::from(Span::styled(
                "/help  commands    /status  session    /doctor  diagnostics",
                Style::default().fg(MUTED),
            )),
        ];
        let top = inner.height.saturating_sub(13) / 2;
        let welcome = Rect::new(
            inner.x,
            inner.y + top,
            inner.width,
            inner.height.saturating_sub(top),
        );
        f.render_widget(
            Paragraph::new(lines).centered().wrap(Wrap { trim: false }),
            welcome,
        );
        return;
    }
    // Reserve a separate gutter, so role bands also cover wrapped continuation rows.
    let content = Rect::new(
        inner.x.saturating_add(2),
        inner.y,
        inner.width.saturating_sub(2),
        inner.height,
    );
    let layout = {
        let mut view = app.view.borrow_mut();
        let reusable = view.chat_layout.as_ref().is_some_and(|cached| {
            cached.width == content.width
                && cached.hide_tools == app.preferences.hide_tools
                && cached.source == transcript
        });
        if !reusable {
            view.chat_layout = Some(std::rc::Rc::new(build_chat_layout(
                transcript,
                content.width,
                app.preferences.hide_tools,
            )));
        }
        view.chat_layout
            .as_ref()
            .expect("layout initialized")
            .clone()
    };
    let ChatLayout {
        lines,
        bands,
        entry_lines,
        turn_lines,
        row_prefix,
        ..
    } = layout.as_ref();
    {
        let mut view = app.view.borrow_mut();
        view.max_scroll = row_prefix
            .last()
            .copied()
            .unwrap_or(0)
            .saturating_sub(content.height as usize)
            .min(u16::MAX as usize) as u16;
        view.turn_rows = turn_lines.iter().map(|line| row_prefix[*line]).collect();
        let bottom = usize::from(view.max_scroll.saturating_sub(app.chat_scroll_offset))
            + content.height as usize;
        view.copy_entry = entry_lines
            .iter()
            .rev()
            .find(|(index, line)| {
                row_prefix[*line] < bottom && transcript[*index].kind == LineKind::Lokai
            })
            .map(|(index, _)| *index);
    }
    render_bands(
        f,
        row_prefix,
        bands,
        inner,
        content.width,
        app.chat_scroll_offset,
    );
    render_scrolled_layout(f, lines, row_prefix, content, app.chat_scroll_offset);
}

/// One cached layout only: resizing or changing the source replaces it.
pub(super) struct ChatLayout {
    source: Vec<super::transcript::TranscriptLine>,
    width: u16,
    hide_tools: bool,
    lines: Vec<Line<'static>>,
    bands: Vec<Option<Color>>,
    entry_lines: Vec<(usize, usize)>,
    turn_lines: Vec<usize>,
    row_prefix: Vec<usize>,
}

fn build_chat_layout(
    transcript: &[super::transcript::TranscriptLine],
    width: u16,
    hide_tools: bool,
) -> ChatLayout {
    let mut lines = Vec::new();
    let mut bands = Vec::new();
    let mut entry_lines = Vec::new();
    let mut turn_lines = Vec::new();
    let latest_answer = transcript.iter().rposition(|e| e.kind == LineKind::Lokai);
    let mut hidden_tools = 0;
    for (index, entry) in transcript.iter().enumerate() {
        if hide_tools
            && matches!(entry.kind, LineKind::Tool | LineKind::SubagentStep { .. })
            && !entry.text.contains("failed")
        {
            hidden_tools += 1;
            continue;
        }
        if hidden_tools > 0 {
            lines.push(Line::from(Span::styled(
                format!("{hidden_tools} tool updates hidden - Ctrl+T to show"),
                Style::default().fg(MUTED),
            )));
            bands.push(None);
            hidden_tools = 0;
        }
        let color = match &entry.kind {
            LineKind::You => ACCENT,
            LineKind::Error => RED,
            LineKind::Warn | LineKind::Tool | LineKind::SubagentStep { .. } => GOLD,
            LineKind::Thought { .. } | LineKind::Sys => MUTED,
            LineKind::SubagentFooter { ok: false, .. } => RED,
            LineKind::SubagentHeader { .. } | LineKind::SubagentFooter { .. } => ACCENT,
            LineKind::Lokai if Some(index) == latest_answer => response_theme::ACCENT,
            LineKind::Lokai => Color::Rgb(125, 155, 196),
        };
        if matches!(entry.kind, LineKind::You | LineKind::Lokai) {
            if !lines.is_empty() {
                lines.push(Line::from(""));
                bands.push(None);
            }
            entry_lines.push((index, lines.len()));
            if entry.kind == LineKind::You {
                turn_lines.push(lines.len());
            }
            lines.push(Line::from(Span::styled(
                if entry.kind == LineKind::You {
                    "YOU"
                } else {
                    "LOKAI"
                },
                Style::default().fg(color).bold(),
            )));
            if entry.kind == LineKind::Lokai {
                lines.extend(markdown::render(&entry.text, width));
            } else {
                for line in entry.text.split('\n') {
                    lines.push(Line::from(Span::styled(
                        line.to_string(),
                        Style::default().fg(TEXT),
                    )));
                }
            }
        } else {
            for line in entry.display().split('\n') {
                lines.push(Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(color),
                )));
            }
        }
        let band = if matches!(entry.kind, LineKind::You | LineKind::Lokai) {
            Some(color)
        } else {
            None
        };
        bands.resize(lines.len(), band);
    }
    if hidden_tools > 0 {
        lines.push(Line::from(Span::styled(
            format!("{hidden_tools} tool updates hidden - Ctrl+T to show"),
            Style::default().fg(MUTED),
        )));
        bands.push(None);
    }
    let mut row_prefix = vec![0usize];
    for line in &lines {
        row_prefix.push(
            row_prefix.last().copied().unwrap_or(0)
                + Paragraph::new(line.clone())
                    .wrap(Wrap { trim: false })
                    .line_count(width),
        );
    }
    ChatLayout {
        source: transcript.to_vec(),
        width,
        hide_tools,
        lines,
        bands,
        entry_lines,
        turn_lines,
        row_prefix,
    }
}

fn render_bands(
    f: &mut ratatui::Frame,
    row_prefix: &[usize],
    bands: &[Option<Color>],
    area: Rect,
    width: u16,
    offset: u16,
) {
    if width == 0 || area.height == 0 {
        return;
    }
    let start = row_prefix
        .last()
        .copied()
        .unwrap_or(0)
        .saturating_sub(area.height as usize)
        .saturating_sub(offset as usize);
    let end = start + area.height as usize;
    for (bounds, band) in row_prefix.windows(2).zip(bands) {
        let row = bounds[0];
        let height = bounds[1] - row;
        if row >= end {
            break;
        }
        if let Some(color) = band {
            for visible_row in row.max(start)..(row + height).min(end) {
                if *color == response_theme::ACCENT {
                    f.render_widget(
                        Block::default().style(Style::default().bg(response_theme::BG)),
                        Rect::new(
                            area.x + 1,
                            area.y + (visible_row - start) as u16,
                            area.width.saturating_sub(1),
                            1,
                        ),
                    );
                }
                // A colored space needs no Unicode glyph or special terminal font.
                f.render_widget(
                    Paragraph::new("|").style(Style::default().fg(*color).bg(*color)),
                    Rect::new(area.x, area.y + (visible_row - start) as u16, 1, 1),
                );
            }
        }
    }
}

/// Cull by actual wrapped rows, never by treating a row offset as a source index.
fn visible_lines(
    lines: &[Line<'static>],
    row_prefix: &[usize],
    height: u16,
    offset: u16,
) -> (Vec<Line<'static>>, u16) {
    if height == 0 {
        return (Vec::new(), 0);
    }
    let total = row_prefix.last().copied().unwrap_or(0);
    let start = total
        .saturating_sub(height as usize)
        .saturating_sub(offset as usize);
    let end = start + height as usize;
    let mut scroll = 0;
    let mut visible = Vec::new();
    for (line, bounds) in lines.iter().zip(row_prefix.windows(2)) {
        let row = bounds[0];
        let count = bounds[1] - row;
        if row >= end {
            break;
        }
        if row + count > start {
            if visible.is_empty() {
                scroll = start.saturating_sub(row).min(u16::MAX as usize) as u16;
            }
            visible.push(line.clone());
        }
    }
    (visible, scroll)
}

fn render_scrolled_layout(
    f: &mut ratatui::Frame,
    lines: &[Line<'static>],
    row_prefix: &[usize],
    area: Rect,
    offset: u16,
) {
    if area.width == 0 {
        return;
    }
    let (visible, scroll) = visible_lines(lines, row_prefix, area.height, offset);
    f.render_widget(
        Paragraph::new(Text::from(visible))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        area,
    );
}

#[cfg(test)]
fn render_scrolled(f: &mut ratatui::Frame, lines: Vec<Line<'static>>, area: Rect, offset: u16) {
    let mut prefix = vec![0];
    for line in &lines {
        prefix.push(
            prefix.last().copied().unwrap_or(0)
                + Paragraph::new(line.clone())
                    .wrap(Wrap { trim: false })
                    .line_count(area.width),
        );
    }
    render_scrolled_layout(f, &lines, &prefix, area, offset);
}

fn render_inspector(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let title = if app.show_activity {
        " Activity "
    } else if app.turn_failed {
        " Error details "
    } else if app.inspector_text.starts_with("read_file")
        || app.inspector_text.starts_with("edit_file")
    {
        " File details "
    } else {
        " Context "
    };
    let title = if app.view.borrow().focus == Focus::Inspector {
        format!("{title}[focus] ")
    } else {
        title.to_owned()
    };
    let block = panel(title).border_style(Style::default().fg(
        if app.view.borrow().focus == Focus::Inspector {
            ACCENT
        } else {
            EDGE
        },
    ));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let body = if app.show_activity {
        if app.activity.is_empty() {
            "No activity yet.".into()
        } else {
            app.activity.join("\n")
        }
    } else {
        app.inspector_text.clone()
    };
    let paragraph = Paragraph::new(body).wrap(Wrap { trim: false });
    let max = paragraph
        .line_count(inner.width)
        .saturating_sub(inner.height as usize)
        .min(u16::MAX as usize) as u16;
    app.view.borrow_mut().inspector_max_scroll = max;
    let scroll = if app.show_activity && app.view.borrow().inspector_follow {
        max
    } else {
        app.inspector_scroll_offset.min(max)
    };
    f.render_widget(paragraph.scroll((scroll, 0)), inner);
}

fn health_color(h: Health) -> Color {
    match h {
        Health::Ready => ACCENT,
        Health::Busy => GOLD,
        Health::Degraded => RED,
        Health::Recovery => GOLD,
    }
}

fn render_status(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let resume = if app.recovery_chip && matches!(app.phase, TurnPhase::Idle) {
        status::resume_banner(&app.resume_state)
    } else {
        None
    };
    let mut text = status::status_text(
        &app.phase,
        app.thinking,
        app.turn_start,
        app.status_hint.as_deref(),
        resume.as_deref(),
    );
    if matches!(app.phase, TurnPhase::Idle) && app.status_hint.is_none() {
        if let Some(elapsed) = app.last_turn_elapsed {
            text = format!(
                " Completed in {:.1}s - ready for your next message",
                elapsed.as_secs_f32()
            );
        }
    }
    let running = app.thinking
        && app.pending_approval.is_none()
        && !matches!(app.phase, TurnPhase::WaitingApproval);
    let mut spans = if running && !app.preferences.reduced_motion && area.width >= 36 {
        oscilloscope(
            app.turn_start.map(|s| s.elapsed()).unwrap_or_default(),
            if area.width < 64 { 6 } else { 12 },
        )
    } else {
        vec![Span::styled(
            " ·",
            Style::default().fg(health_color(current_health(app))),
        )]
    };
    spans.push(Span::styled(
        text,
        Style::default().fg(if running {
            TEXT
        } else {
            health_color(current_health(app))
        }),
    ));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// ASCII trace works in PowerShell hosts whose fonts lack braille glyphs.
/// Wall time keeps the motion steady during token bursts.
fn oscilloscope(elapsed: std::time::Duration, width: usize) -> Vec<Span<'static>> {
    let phase = (elapsed.as_millis() % 1800) as f64 / 1800.0 * std::f64::consts::TAU;
    let mut spans = vec![Span::styled("[", Style::default().fg(MUTED))];
    for x in 0..width {
        let sample = ((x as f64 * 0.8 - phase).sin() * 1.95 + 2.0).round() as usize;
        let trace = ['\'', '`', '-', '.', '_'][sample];
        let color = if x + 2 >= width { GOLD } else { LAVENDER };
        spans.push(Span::styled(trace.to_string(), Style::default().fg(color)));
    }
    spans.push(Span::styled("]", Style::default().fg(MUTED)));
    spans
}

fn render_prompt(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let frozen = app.pending_approval.is_some();
    let focused = app.view.borrow().focus == Focus::Composer;
    let block = panel(if frozen {
        " Draft preserved - awaiting decision "
    } else if app.thinking {
        " Draft - send after this turn finishes "
    } else {
        if focused {
            " Message [focus] "
        } else {
            " Message "
        }
    })
    .border_style(Style::default().fg(if focused && !frozen { ACCENT } else { EDGE }));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.width < 3 || inner.height == 0 {
        return;
    }
    let layout = composer::layout(&app.input_buffer, inner.width.saturating_sub(2) as usize);
    let (col, row) = layout.positions[app.input_cursor.min(layout.positions.len() - 1)];
    let top = row.saturating_sub(inner.height.saturating_sub(1) as usize);
    let mut lines: Vec<_> = layout
        .rows
        .iter()
        .skip(top)
        .take(inner.height as usize)
        .enumerate()
        .map(|(i, text)| {
            Line::from(vec![
                Span::styled(
                    if i + top == 0 { "> " } else { "  " },
                    Style::default().fg(MUTED),
                ),
                Span::raw(text.clone()),
            ])
        })
        .collect();
    if app.input_buffer.is_empty() {
        lines = vec![Line::from(Span::styled(
            "> Ask, build, or type / for commands...",
            Style::default().fg(MUTED),
        ))];
    } else if !frozen && layout.rows.len() == 1 {
        if let Some(rest) = slash::ghost(&app.input_buffer, app.tab_cycle) {
            if let Some(line) = lines.last_mut() {
                line.spans
                    .push(Span::styled(rest, Style::default().fg(MUTED)));
            }
        }
    }
    f.render_widget(Paragraph::new(lines), inner);
    let view = app.view.borrow();
    if focused
        && !frozen
        && !app.show_help
        && !view.show_preferences
        && view.copy.is_none()
        && app.model_picker.is_none()
    {
        f.set_cursor_position((inner.x + 2 + col as u16, inner.y + (row - top) as u16));
    }
}

fn render_keys(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let focus = app.view.borrow().focus;
    let primary = if app.thinking {
        ("Ctrl+C", "cancel")
    } else if focus == Focus::Composer {
        ("Enter", "send")
    } else {
        ("Esc", "write")
    };
    let mut keys = vec![
        primary,
        ("Ctrl+J", "newline"),
        ("Tab", "focus"),
        ("Ctrl+E", "sidebar"),
        ("F3", "copy"),
        ("F5", "model"),
        ("F1", "help"),
    ];
    if area.width < 90 {
        keys = vec![primary, ("F1", "help"), ("Tab", "focus")];
    }
    let mut spans = vec![Span::raw(" ")];
    for (index, (key, label)) in keys.into_iter().enumerate() {
        spans.push(Span::styled(
            format!(" {key} "),
            if index == 0 {
                Style::default().fg(BG).bg(ACCENT).bold()
            } else {
                Style::default().fg(MUTED).bold()
            },
        ));
        spans.push(Span::styled(
            format!(" {label}  "),
            Style::default().fg(MUTED),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn current_health(app: &App) -> Health {
    status::health(
        app.recovery_chip,
        app.degraded,
        app.thinking,
        app.pending_approval.is_some(),
    )
}

#[cfg(test)]
#[path = "ui_tests.rs"]
mod tests;

//! Local activation view. It draws the launch receipt's terminal outcome and
//! journal event names. It does not invent a running status or show payloads.
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use tetonic_app::RegisteredLaunchReceipt;

pub fn draw(frame: &mut Frame, receipt: &RegisteredLaunchReceipt) {
    let area = frame.area();
    let block = Block::default()
        .title(" Activation ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(255, 173, 66)));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let paragraph = Paragraph::new(lines(receipt)).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

pub fn activation_text(receipt: &RegisteredLaunchReceipt) -> String {
    let width = 72u16;
    let height = (lines(receipt).len() as u16).saturating_add(4).max(8);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height))
        .expect("activation view");
    terminal.draw(|frame| draw(frame, receipt)).expect("draw");
    buffer_text(terminal.backend().buffer(), width, height)
}

fn lines(receipt: &RegisteredLaunchReceipt) -> Vec<Line<'static>> {
    let mut rows = vec![
        Line::from(Span::raw(format!("Outcome: {}", outcome_label(receipt)))),
        Line::from(Span::raw(format!("Run: {}", receipt.run_id))),
        Line::from(Span::raw("Events:")),
    ];
    if receipt.events.is_empty() {
        rows.push(Line::from(Span::raw("  (none)")));
    } else {
        for event in &receipt.events {
            rows.push(Line::from(Span::raw(format!(
                "  {} {}",
                event.sequence, event.event_type
            ))));
        }
    }
    rows
}

fn outcome_label(receipt: &RegisteredLaunchReceipt) -> &'static str {
    match receipt.outcome.as_deref() {
        Some("completed") => "completed",
        Some("canceled") => "canceled",
        Some("limited") => "limited",
        Some("failed") => "failed",
        _ => "unavailable",
    }
}

fn buffer_text(buffer: &ratatui::buffer::Buffer, width: u16, height: u16) -> String {
    let mut text = String::new();
    for y in 0..height {
        for x in 0..width {
            text.push_str(buffer[(x, y)].symbol());
        }
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use tetonic_app::job_launch::RegisteredEventReceipt;

    fn receipt(outcome: &str) -> RegisteredLaunchReceipt {
        RegisteredLaunchReceipt {
            run_id: "run-1".into(),
            task_id: "task-1".into(),
            audit_session_id: "audit-1".into(),
            launched: true,
            outcome: Some(outcome.into()),
            events: vec![
                RegisteredEventReceipt {
                    sequence: 1,
                    event_type: "run.created".into(),
                    payload_digest: "digest-1".into(),
                },
                RegisteredEventReceipt {
                    sequence: 2,
                    event_type: "attempt.started".into(),
                    payload_digest: "digest-2".into(),
                },
            ],
        }
    }

    #[test]
    fn activation_view_shows_journal_events_and_not_a_running_status() {
        let shown = activation_text(&receipt("completed"));
        assert!(shown.contains("Activation"));
        assert!(shown.contains("Outcome: completed"));
        assert!(shown.contains("1 run.created"));
        assert!(shown.contains("2 attempt.started"));
        assert!(!shown.contains("digest-1"));
        assert!(!shown.to_ascii_lowercase().contains("running"));

        let hidden = activation_text(&receipt("running"));
        assert!(hidden.contains("Outcome: unavailable"));
        assert!(!hidden.to_ascii_lowercase().contains("running"));
        assert!(hidden.contains("attempt.started"));
    }
}

//! Hard-wrapped composer rows and caret share the same cell-width calculation.
use ratatui::text::Span;

pub struct ComposerLayout {
    pub rows: Vec<String>,
    pub positions: Vec<(usize, usize)>,
}

pub fn layout(text: &str, width: usize) -> ComposerLayout {
    let width = width.max(1);
    let mut rows = vec![String::new()];
    let mut positions = Vec::new();
    let mut col = 0;
    let mut wrapped = false;
    for c in text.chars() {
        let w = Span::raw(c.to_string()).width();
        if c != '\n' && col + w > width {
            rows.push(String::new());
            col = 0;
        }
        positions.push((col, rows.len() - 1));
        if c == '\n' {
            if !wrapped {
                rows.push(String::new());
            }
            col = 0;
            wrapped = false;
        } else {
            wrapped = false;
            if let Some(row) = rows.last_mut() {
                row.push(c);
            }
            col += w;
            if col >= width {
                wrapped = true;
                rows.push(String::new());
                col = 0;
            }
        }
    }
    positions.push((col, rows.len() - 1));
    ComposerLayout { rows, positions }
}

pub fn move_vertical(text: &str, cursor: usize, width: usize, down: bool) -> usize {
    let layout = layout(text, width);
    let (col, row) = layout.positions[cursor.min(layout.positions.len() - 1)];
    let target = if down {
        row.saturating_add(1)
    } else {
        row.saturating_sub(1)
    };
    layout
        .positions
        .iter()
        .enumerate()
        .filter(|(_, (_, r))| *r == target)
        .min_by_key(|(_, (c, _))| c.abs_diff(col))
        .map(|(i, _)| i)
        .unwrap_or(cursor)
}

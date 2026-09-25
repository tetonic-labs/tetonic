//! Streaming-friendly Markdown presentation. The stored transcript stays untouched.
//! Unsupported or incomplete syntax remains readable as literal text.

use super::response_theme::{ACCENT, CODE, CODE_BG, EDGE, HEADING, MUTED, TEXT};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

pub(super) fn render(source: &str, width: u16) -> Vec<Line<'static>> {
    let input: Vec<_> = source.split('\n').collect();
    let mut output = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut i = 0;
    while i < input.len() {
        let raw = input[i].trim_end_matches('\r');
        let trimmed = raw.trim_start();
        i += 1;
        if let Some((marker, count)) = fence {
            let closing = trimmed.chars().take_while(|c| *c == marker).count();
            if closing >= count && trimmed[closing..].trim().is_empty() {
                output.push(Line::from(Span::styled("  └─", Style::default().fg(EDGE))));
                fence = None;
            } else {
                output.push(Line::from(vec![
                    Span::styled("  │ ", Style::default().fg(EDGE)),
                    Span::styled(
                        raw.replace('\t', "    "),
                        Style::default().fg(CODE).bg(CODE_BG),
                    ),
                ]));
            }
            continue;
        }
        if let Some(marker) = trimmed.chars().next().filter(|c| *c == '`' || *c == '~') {
            let count = trimmed.chars().take_while(|c| *c == marker).count();
            if count >= 3 {
                fence = Some((marker, count));
                let language = trimmed[count..].trim();
                output.push(Line::from(Span::styled(
                    format!(
                        "  ┌─ {}",
                        if language.is_empty() {
                            "code"
                        } else {
                            language
                        }
                    ),
                    Style::default().fg(MUTED),
                )));
                continue;
            }
        }
        if raw.contains('|') && i < input.len() && table_separator(input[i]) {
            let headers = cells(raw);
            if cells(input[i]).len() == headers.len() {
                i += 1;
                let mut rows = vec![headers];
                while i < input.len() && input[i].contains('|') && !input[i].trim().is_empty() {
                    rows.push(cells(input[i]));
                    i += 1;
                }
                output.extend(table(rows, width));
                continue;
            }
        }
        let heading = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&heading) && trimmed[heading..].starts_with(' ') {
            output.push(Line::from(inline(
                trimmed[heading..].trim(),
                Style::default().fg(HEADING).bold(),
                0,
            )));
        } else if matches!(trimmed.trim_end(), "---" | "***" | "___") {
            output.push(Line::from(Span::styled(
                "─".repeat(usize::from(width.min(48))),
                Style::default().fg(EDGE),
            )));
        } else if let Some(quote) = trimmed.strip_prefix("> ") {
            let mut spans = vec![Span::styled("│ ", Style::default().fg(HEADING))];
            spans.extend(inline(quote, Style::default().fg(MUTED).italic(), 0));
            output.push(Line::from(spans));
        } else if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            let indent = raw.len() - trimmed.len();
            let mut spans = vec![Span::styled(
                format!("{}• ", " ".repeat(indent)),
                Style::default().fg(ACCENT),
            )];
            spans.extend(inline(item, Style::default().fg(TEXT), 0));
            output.push(Line::from(spans));
        } else {
            output.push(Line::from(inline(raw, Style::default().fg(TEXT), 0)));
        }
    }
    output
}

/// Parse paired inline markup only. In-flight delimiters remain visible until closed.
fn inline(text: &str, style: Style, depth: usize) -> Vec<Span<'static>> {
    if depth > 8 {
        return vec![Span::styled(text.to_owned(), style)];
    }
    let mut spans = Vec::new();
    let mut plain = String::new();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        if let Some(escaped) = rest
            .strip_prefix('\\')
            .and_then(|s| s.chars().next())
            .filter(|c| "\\`*_[]".contains(*c))
        {
            plain.push(escaped);
            i += 1 + escaped.len_utf8();
            continue;
        }
        let code_ticks = rest.chars().take_while(|c| *c == '`').count();
        if code_ticks > 0 {
            let delimiter = "`".repeat(code_ticks);
            if let Some(end) = rest[code_ticks..].find(&delimiter) {
                flush(&mut spans, &mut plain, style);
                spans.push(Span::styled(
                    rest[code_ticks..code_ticks + end].to_owned(),
                    Style::default().fg(CODE).bg(CODE_BG),
                ));
                i += code_ticks * 2 + end;
                continue;
            }
        }
        let mut matched = false;
        for (delimiter, decorated) in [
            ("**", style.bold()),
            ("__", style.bold()),
            ("*", style.italic()),
            ("_", style.italic()),
            ("~~", style.crossed_out()),
        ] {
            // Underscores in identifiers and paths are not emphasis.
            if delimiter.contains('_')
                && i > 0
                && text[..i]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric())
            {
                continue;
            }
            if let Some(body) = rest.strip_prefix(delimiter) {
                if let Some(end) = body.find(delimiter).filter(|end| *end > 0) {
                    let inside = &body[..end];
                    if inside.starts_with(char::is_whitespace)
                        || inside.ends_with(char::is_whitespace)
                    {
                        continue;
                    }
                    flush(&mut spans, &mut plain, style);
                    spans.extend(inline(inside, decorated, depth + 1));
                    i += delimiter.len() * 2 + end;
                    matched = true;
                    break;
                }
            }
        }
        if matched {
            continue;
        }
        if let Some(label) = rest.strip_prefix('[') {
            if let Some(label_end) = label.find("](") {
                let target = &label[label_end + 2..];
                if let Some(target_end) = target.find(')') {
                    flush(&mut spans, &mut plain, style);
                    spans.extend(inline(
                        &label[..label_end],
                        style.fg(HEADING).underlined(),
                        depth + 1,
                    ));
                    spans.push(Span::styled(
                        format!(" ({})", &target[..target_end]),
                        Style::default().fg(MUTED),
                    ));
                    i += 1 + label_end + 2 + target_end + 1;
                    continue;
                }
            }
        }
        if let Some(c) = rest.chars().next() {
            plain.push(c);
            i += c.len_utf8();
        }
    }
    flush(&mut spans, &mut plain, style);
    spans
}

fn flush(spans: &mut Vec<Span<'static>>, plain: &mut String, style: Style) {
    if !plain.is_empty() {
        spans.push(Span::styled(std::mem::take(plain), style));
    }
}

fn cells(line: &str) -> Vec<String> {
    let line = line.trim();
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = if line.ends_with('|') && !line.ends_with("\\|") {
        &line[..line.len() - 1]
    } else {
        line
    };
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut chars = line.chars().peekable();
    let mut code_ticks = 0;
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'|') {
            cell.push('|');
            chars.next();
        } else if c == '`' {
            let mut count = 1;
            while chars.peek() == Some(&'`') {
                chars.next();
                count += 1;
            }
            if code_ticks == 0 {
                code_ticks = count;
            } else if code_ticks == count {
                code_ticks = 0;
            }
            cell.push_str(&"`".repeat(count));
        } else if c == '|' && code_ticks == 0 {
            cells.push(cell.trim().to_owned());
            cell.clear();
        } else {
            cell.push(c);
        }
    }
    cells.push(cell.trim().to_owned());
    cells
}

fn table_separator(line: &str) -> bool {
    let cells = cells(line);
    !cells.is_empty()
        && cells.iter().all(|s| {
            let s = s.trim_matches(':');
            s.len() >= 3 && s.chars().all(|c| c == '-')
        })
}

fn table(rows: Vec<Vec<String>>, width: u16) -> Vec<Line<'static>> {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let styled: Vec<Vec<Line<'static>>> = rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            row.iter()
                .map(|cell| {
                    Line::from(inline(
                        cell,
                        if i == 0 {
                            Style::default().fg(HEADING).bold()
                        } else {
                            Style::default().fg(TEXT)
                        },
                        0,
                    ))
                })
                .collect()
        })
        .collect();
    let widths: Vec<usize> = (0..columns)
        .map(|column| {
            styled
                .iter()
                .filter_map(|row| row.get(column))
                .map(Line::width)
                .max()
                .unwrap_or(0)
        })
        .collect();
    let mut output = Vec::new();
    if styled.len() == 1 {
        // Keep the header visible while the first data row is still streaming.
        for cell in &styled[0] {
            output.push(cell.clone());
        }
        return output;
    }
    if widths.iter().sum::<usize>() + columns.saturating_sub(1) * 3 > width as usize {
        // Narrow terminals read each row as labeled fields instead of a broken grid.
        for row in styled.iter().skip(1) {
            for (column, cell) in row.iter().enumerate() {
                let mut spans = styled[0]
                    .get(column)
                    .map(|s| s.spans.clone())
                    .unwrap_or_else(|| vec![Span::raw(format!("Column {}", column + 1))]);
                spans.push(Span::raw(": "));
                spans.extend(cell.spans.clone());
                output.push(Line::from(spans));
            }
            output.push(Line::from(""));
        }
        return output;
    }
    for (i, row) in styled.iter().enumerate() {
        let mut spans = Vec::new();
        for (column, width) in widths.iter().enumerate() {
            if column > 0 {
                spans.push(Span::styled(" │ ", Style::default().fg(EDGE)));
            }
            let cell = row.get(column).cloned().unwrap_or_default();
            let padding = width.saturating_sub(cell.width());
            spans.extend(cell.spans);
            spans.push(Span::raw(" ".repeat(padding)));
        }
        output.push(Line::from(spans));
        if i == 0 {
            output.push(Line::from(Span::styled(
                widths
                    .iter()
                    .map(|w| "─".repeat(*w))
                    .collect::<Vec<_>>()
                    .join("─┼─"),
                Style::default().fg(EDGE),
            )));
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;

    fn text(lines: &[Line<'_>]) -> String {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn formats_prose_without_modifying_code_or_identifiers() {
        let lines = render("## Heading\n**bold** and *italic* and `my_code`\n- item\nmy_code_name\n```rust\n    let s = \"**literal**\";\n```", 80);
        let rendered = text(&lines);
        assert!(rendered.contains("bold and italic and my_code"));
        assert!(rendered.contains("• item\nmy_code_name"));
        assert!(rendered.contains("    let s = \"**literal**\";"));
        assert!(lines[1]
            .spans
            .iter()
            .any(|s| s.content == "bold" && s.style.add_modifier.contains(Modifier::BOLD)));
        assert!(lines[1]
            .spans
            .iter()
            .any(|s| s.content == "italic" && s.style.add_modifier.contains(Modifier::ITALIC)));
    }

    #[test]
    fn streaming_unicode_and_unclosed_fences_are_safe() {
        let source = "## Résumé\n**Hello 界** `partial\n```rust\n  incomplete();";
        for (end, _) in source
            .char_indices()
            .chain(std::iter::once((source.len(), ' ')))
        {
            render(&source[..end], 20);
        }
        let rendered = text(&render(source, 80));
        assert!(rendered.contains("`partial"));
        assert!(rendered.contains("  incomplete();"));
        assert!(text(&render("````text\n```\n````", 80)).contains("│ ```"));
    }

    #[test]
    fn tables_adapt_to_terminal_width_and_preserve_links() {
        let source = "| File | Result |\n| --- | --- |\n| `src/main.rs` | **passed** |";
        assert!(text(&render(source, 80)).contains("src/main.rs │ passed"));
        let compact = text(&render(source, 12));
        assert!(compact.contains("File: src/main.rs"));
        assert!(compact.contains("Result: passed"));
        assert_eq!(
            cells(r"| `a|b` | escaped \| pipe |"),
            vec!["`a|b`", "escaped | pipe"]
        );
        assert!(text(&render("| Long header |\n| --- |", 5)).contains("Long header"));
        assert_eq!(
            text(&render("[Docs](https://example.com)", 80)),
            "Docs (https://example.com)"
        );
    }
}

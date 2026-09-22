//! Resilient Fuzzy Patch Auto-Alignment (OPT-601).
//!
//! Provides multi-tier patch resolution:
//! Tier 1: Exact verbatim substring match.
//! Tier 2: Line-ending (\r\n vs \n) and trailing whitespace invariance.
//! Tier 3: Indentation-invariant sequence matching with base indentation preservation.
//! Tier 4: High-confidence similarity matching (>=95% character similarity).
//! Fallback: Rich diagnostic error with nearest candidate line numbers and snippet hints.

/// Outcome of a fuzzy patch application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchOutcome {
    pub updated_text: String,
    pub note: Option<String>,
}

/// Applies an edit to `text` replacing `old` with `new` using multi-tier resilient matching.
pub fn apply_fuzzy_edit(text: &str, old: &str, new: &str) -> Result<PatchOutcome, String> {
    if old.is_empty() {
        return Err("old_string cannot be empty".to_string());
    }

    // -------------------------------------------------------------------------
    // Tier 1: Exact verbatim match
    // -------------------------------------------------------------------------
    let exact_count = text.matches(old).count();
    if exact_count == 1 {
        return Ok(PatchOutcome {
            updated_text: text.replacen(old, new, 1),
            note: None,
        });
    }
    if exact_count > 1 {
        return Err(format!(
            "ambiguous edit: found {exact_count} exact matches for old_string. Provide more surrounding context."
        ));
    }

    // Build line descriptors with byte offsets for text.
    let text_lines = collect_line_spans(text);
    let old_lines: Vec<&str> = old.lines().collect();

    if old_lines.is_empty() {
        return Err("old_string contains no lines".to_string());
    }

    let is_crlf_file = text.contains("\r\n");

    // -------------------------------------------------------------------------
    // Tier 2: Line-ending & trailing-whitespace invariant matching
    // -------------------------------------------------------------------------
    let mut tier2_matches = Vec::new();
    let window_len = old_lines.len();

    if text_lines.len() >= window_len {
        for i in 0..=(text_lines.len() - window_len) {
            let mut matches = true;
            for (k, old_l) in old_lines.iter().enumerate() {
                let text_l = text_lines[i + k].content;
                if text_l.trim_end() != old_l.trim_end() {
                    matches = false;
                    break;
                }
            }
            if matches {
                tier2_matches.push(i);
            }
        }
    }

    if tier2_matches.len() == 1 {
        let match_idx = tier2_matches[0];
        let start_byte = text_lines[match_idx].start_byte;
        let end_byte = text_lines[match_idx + window_len - 1].end_byte;
        let text_had_newline = text[..end_byte].ends_with('\n');

        let mut formatted_new = format_replacement_with_line_endings(new, is_crlf_file);
        formatted_new =
            ensure_matching_trailing_newline(formatted_new, text_had_newline, is_crlf_file);
        let mut updated = String::with_capacity(text.len() + formatted_new.len());
        updated.push_str(&text[..start_byte]);
        updated.push_str(&formatted_new);
        updated.push_str(&text[end_byte..]);

        return Ok(PatchOutcome {
            updated_text: updated,
            note: Some("auto-aligned line endings and trailing whitespace".to_string()),
        });
    }

    if tier2_matches.len() > 1 {
        let line_numbers: Vec<usize> = tier2_matches.iter().map(|&idx| idx + 1).collect();
        return Err(format!(
            "ambiguous edit: found {} matches with trailing whitespace variation at lines {:?}",
            tier2_matches.len(),
            line_numbers
        ));
    }

    // -------------------------------------------------------------------------
    // Tier 3: Indentation-invariant sequence matching
    // -------------------------------------------------------------------------
    let mut tier3_matches = Vec::new();
    if text_lines.len() >= window_len {
        for i in 0..=(text_lines.len() - window_len) {
            let mut matches = true;
            for (k, old_l) in old_lines.iter().enumerate() {
                let text_l = text_lines[i + k].content;
                if text_l.trim() != old_l.trim() {
                    matches = false;
                    break;
                }
            }
            if matches {
                tier3_matches.push(i);
            }
        }
    }

    if tier3_matches.len() == 1 {
        let match_idx = tier3_matches[0];
        let start_byte = text_lines[match_idx].start_byte;
        let end_byte = text_lines[match_idx + window_len - 1].end_byte;
        let text_had_newline = text[..end_byte].ends_with('\n');

        // Calculate indentation difference based on the first non-empty line
        let file_indent = leading_indent(text_lines[match_idx].content);
        let old_indent = leading_indent(old_lines[0]);

        let mut adjusted_new = adjust_indentation(new, old_indent, file_indent, is_crlf_file);
        adjusted_new =
            ensure_matching_trailing_newline(adjusted_new, text_had_newline, is_crlf_file);
        let mut updated = String::with_capacity(text.len() + adjusted_new.len());
        updated.push_str(&text[..start_byte]);
        updated.push_str(&adjusted_new);
        updated.push_str(&text[end_byte..]);

        return Ok(PatchOutcome {
            updated_text: updated,
            note: Some("auto-aligned indentation mismatch".to_string()),
        });
    }

    if tier3_matches.len() > 1 {
        let line_numbers: Vec<usize> = tier3_matches.iter().map(|&idx| idx + 1).collect();
        return Err(format!(
            "ambiguous edit: found {} matches with different indentation at lines {:?}",
            tier3_matches.len(),
            line_numbers
        ));
    }

    // -------------------------------------------------------------------------
    // Tier 4: High-confidence similarity search (>= 95% char similarity)
    // -------------------------------------------------------------------------
    if text_lines.len() >= window_len {
        let old_normalized: String = old_lines
            .iter()
            .map(|l| l.trim())
            .collect::<Vec<_>>()
            .join("\n");
        let mut best_sim = 0.0f64;
        let mut best_idx = None;
        let mut second_best_sim = 0.0f64;

        for i in 0..=(text_lines.len() - window_len) {
            let candidate: String = (0..window_len)
                .map(|k| text_lines[i + k].content.trim())
                .collect::<Vec<_>>()
                .join("\n");

            let sim = similarity_score(&old_normalized, &candidate);
            if sim > best_sim {
                second_best_sim = best_sim;
                best_sim = sim;
                best_idx = Some(i);
            } else if sim > second_best_sim {
                second_best_sim = sim;
            }
        }

        if best_sim >= 0.95 && (best_sim - second_best_sim) >= 0.10 {
            if let Some(match_idx) = best_idx {
                let start_byte = text_lines[match_idx].start_byte;
                let end_byte = text_lines[match_idx + window_len - 1].end_byte;
                let text_had_newline = text[..end_byte].ends_with('\n');

                let mut formatted_new = format_replacement_with_line_endings(new, is_crlf_file);
                formatted_new =
                    ensure_matching_trailing_newline(formatted_new, text_had_newline, is_crlf_file);
                let mut updated = String::with_capacity(text.len() + formatted_new.len());
                updated.push_str(&text[..start_byte]);
                updated.push_str(&formatted_new);
                updated.push_str(&text[end_byte..]);

                return Ok(PatchOutcome {
                    updated_text: updated,
                    note: Some(format!(
                        "auto-aligned high-confidence fuzzy match ({:.1}% match)",
                        best_sim * 100.0
                    )),
                });
            }
        }
    }

    // -------------------------------------------------------------------------
    // Diagnostic Line Hinting on Failure
    // -------------------------------------------------------------------------
    let hint = generate_diagnostic_hint(&text_lines, old_lines[0]);
    Err(format!("old_string not found. {hint}"))
}

#[derive(Debug, Clone)]
struct LineSpan<'a> {
    content: &'a str,
    start_byte: usize,
    end_byte: usize,
}

fn collect_line_spans(text: &str) -> Vec<LineSpan<'_>> {
    let mut spans = Vec::new();
    let mut curr_byte = 0;

    for line in text.split_inclusive('\n') {
        let line_len = line.len();
        // Strip line terminator for content comparison
        let content = line
            .strip_suffix("\r\n")
            .or_else(|| line.strip_suffix('\n'))
            .unwrap_or(line);
        spans.push(LineSpan {
            content,
            start_byte: curr_byte,
            end_byte: curr_byte + line_len,
        });
        curr_byte += line_len;
    }

    // Handle trailing line if string didn't end in \n
    if curr_byte < text.len() {
        let content = &text[curr_byte..];
        spans.push(LineSpan {
            content,
            start_byte: curr_byte,
            end_byte: text.len(),
        });
    }

    spans
}

fn leading_indent(line: &str) -> &str {
    let non_ws = line
        .find(|c: char| !c.is_whitespace())
        .unwrap_or(line.len());
    &line[..non_ws]
}

fn adjust_indentation(new: &str, old_indent: &str, target_indent: &str, crlf: bool) -> String {
    let old_indent_len = old_indent.len();
    let target_indent_len = target_indent.len();
    let mut out = Vec::new();

    for line in new.lines() {
        if line.trim().is_empty() {
            out.push(String::new());
            continue;
        }
        let line_indent_len = line
            .find(|c: char| !c.is_whitespace())
            .unwrap_or(line.len());
        let trimmed = line.trim_start();

        let adjusted_line = if line_indent_len >= old_indent_len && old_indent_len > 0 {
            let extra = line_indent_len - old_indent_len;
            let scaled_extra = if old_indent_len == 4 && target_indent_len == 2 {
                extra / 2
            } else if old_indent_len == 2 && target_indent_len == 4 {
                extra * 2
            } else {
                extra
            };
            let total_spaces = target_indent_len + scaled_extra;
            format!("{}{}", " ".repeat(total_spaces), trimmed)
        } else {
            format!("{target_indent}{trimmed}")
        };
        out.push(adjusted_line);
    }
    let sep = if crlf { "\r\n" } else { "\n" };
    out.join(sep)
}

fn format_replacement_with_line_endings(new: &str, crlf: bool) -> String {
    if !crlf {
        new.replace("\r\n", "\n")
    } else if !new.contains("\r\n") {
        new.replace('\n', "\r\n")
    } else {
        new.to_string()
    }
}

fn ensure_matching_trailing_newline(
    mut content: String,
    text_had_newline: bool,
    crlf: bool,
) -> String {
    let has_newline = content.ends_with('\n');
    if text_had_newline && !has_newline {
        if crlf {
            content.push_str("\r\n");
        } else {
            content.push('\n');
        }
    } else if !text_had_newline && has_newline {
        if content.ends_with("\r\n") {
            content.truncate(content.len() - 2);
        } else if content.ends_with('\n') {
            content.truncate(content.len() - 1);
        }
    }
    content
}

fn similarity_score(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    let max_len = a.len().max(b.len());
    if max_len == 0 {
        return 1.0;
    }
    let dist = levenshtein(a, b);
    1.0 - (dist as f64 / max_len as f64)
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();

    let mut prev: Vec<usize> = (0..=b_chars.len()).collect();
    let mut curr: Vec<usize> = vec![0; b_chars.len() + 1];

    for (i, &ca) in a_chars.iter().enumerate() {
        curr[0] = i + 1;
        for (j, &cb) in b_chars.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (curr[j] + 1).min(prev[j + 1] + 1).min(prev[j] + cost);
        }
        prev.copy_from_slice(&curr);
    }

    prev[b_chars.len()]
}

fn generate_diagnostic_hint(text_lines: &[LineSpan<'_>], first_old_line: &str) -> String {
    let trimmed_target = first_old_line.trim();
    if trimmed_target.is_empty() {
        return format!("Target file contains {} lines.", text_lines.len());
    }

    // Search for closest matching line
    let mut best_sim = 0.0f64;
    let mut best_line = None;

    for (i, span) in text_lines.iter().enumerate() {
        let sim = similarity_score(trimmed_target, span.content.trim());
        if sim > best_sim {
            best_sim = sim;
            best_line = Some(i);
        }
    }

    if let Some(idx) = best_line {
        if best_sim >= 0.5 {
            let start = idx.saturating_sub(1);
            let end = (idx + 2).min(text_lines.len());
            let snippet: Vec<String> = (start..end)
                .map(|i| format!("{:>4}| {}", i + 1, text_lines[i].content))
                .collect();
            return format!(
                "Nearest candidate at lines {}-{}:\n{}",
                start + 1,
                end,
                snippet.join("\n")
            );
        }
    }

    format!(
        "File has {} lines; no similar line was found.",
        text_lines.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        let text = "fn foo() {\n    let x = 1;\n}\n";
        let outcome = apply_fuzzy_edit(text, "let x = 1;", "let x = 2;").unwrap();
        assert_eq!(outcome.updated_text, "fn foo() {\n    let x = 2;\n}\n");
        assert!(outcome.note.is_none());
    }

    #[test]
    fn test_crlf_and_trailing_whitespace_auto_alignment() {
        let text = "fn foo() {\r\n    let x = 1;   \r\n}\r\n";
        let outcome = apply_fuzzy_edit(text, "    let x = 1;\n", "    let x = 2;\n").unwrap();
        assert!(outcome.updated_text.contains("let x = 2;"));
        assert!(outcome.note.is_some());
    }

    #[test]
    fn test_indentation_invariant_auto_alignment() {
        let text = "class Bar:\n  def greet(self):\n    pass\n";
        // Model provided 4 spaces instead of 2 spaces
        let old = "    def greet(self):\n        pass";
        let new = "    def greet(self):\n        return 'hello'";
        let outcome = apply_fuzzy_edit(text, old, new).unwrap();
        assert_eq!(
            outcome.updated_text,
            "class Bar:\n  def greet(self):\n    return 'hello'\n"
        );
        assert!(outcome.note.as_ref().unwrap().contains("indentation"));
    }

    #[test]
    fn test_high_confidence_fuzzy_alignment() {
        let text = "pub fn compute(val: u64) -> u64 {\n    val * 2\n}\n";
        // Minor typo in old_string
        let old = "pub fn compute(val: u64) -> u64 {\n    val * 2;\n}";
        let new = "pub fn compute(val: u64) -> u64 {\n    val * 3\n}";
        let outcome = apply_fuzzy_edit(text, old, new).unwrap();
        assert!(outcome.updated_text.contains("val * 3"));
        assert!(outcome.note.as_ref().unwrap().contains("fuzzy match"));
    }

    #[test]
    fn test_unmatched_returns_diagnostic_hint() {
        let text = "fn hello_world() {\n    println!(\"hello\");\n}\n";
        let err = apply_fuzzy_edit(text, "fn goodbye_world()", "fn bye()").unwrap_err();
        assert!(err.contains("old_string not found"));
    }
}

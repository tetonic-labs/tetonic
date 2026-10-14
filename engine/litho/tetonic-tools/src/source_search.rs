//! Bounded search observations from the same authorized source snapshot.
use crate::{GrepArgs, ToolError, ToolOutcome};
use std::collections::BTreeMap;

pub(super) struct SearchHit {
    pub path: String,
    pub line: usize,
    lines: Vec<SourceLine>,
}

struct SourceLine {
    number: usize,
    text: String,
    matched: bool,
    clipped: bool,
}

pub(super) fn validate(args: &GrepArgs) -> Result<(), ToolError> {
    if args.max_bytes.is_some_and(|n| n < 256) {
        return Err(ToolError::BadArgs("max_bytes must be at least 256".into()));
    }
    if args.context_lines.is_some_and(|n| n > 12) {
        return Err(ToolError::BadArgs(
            "context_lines must be between 0 and 12".into(),
        ));
    }
    if args.max_results.is_some_and(|n| n == 0 || n > 1000) {
        return Err(ToolError::BadArgs(
            "max_results must be between 1 and 1000".into(),
        ));
    }
    Ok(())
}

/// Match and surrounding lines come from one read; never reopen a changed file
/// to append context to earlier hits. Traversal/authorization remain in Tools.
pub(super) fn collect(
    path: &str,
    text: &str,
    pattern: &regex::Regex,
    args: &GrepArgs,
) -> Vec<SearchHit> {
    let lines: Vec<_> = text.lines().collect();
    let context = args.context_lines.unwrap_or(0);
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| pattern.is_match(line))
        // One extra hit is enough to report a result-count omission.
        .take(
            args.max_results
                .unwrap_or(crate::DEFAULT_GREP_RESULTS)
                .saturating_add(1),
        )
        .map(|(i, _)| SearchHit {
            path: path.into(),
            line: i + 1,
            lines: (i.saturating_sub(context)..(i + context + 1).min(lines.len()))
                .map(|j| {
                    let line = lines[j].trim_end();
                    let (text, clipped) = excerpt(line, pattern, 4096);
                    SourceLine {
                        number: j + 1,
                        text,
                        matched: pattern.is_match(line),
                        clipped,
                    }
                })
                .collect(),
        })
        .collect()
}

pub(super) fn format_hits(
    hits: &[SearchHit],
    pattern: &regex::Regex,
    args: &GrepArgs,
    scan_stopped: bool,
) -> Result<ToolOutcome, ToolError> {
    validate(args)?;
    let bytes = args.max_bytes.unwrap_or(2048).min(4096);
    let limit = args.max_results.unwrap_or(crate::DEFAULT_GREP_RESULTS);
    const NOTE: &str = "\n[Search limited: excerpts/context clipped, more matches omitted or files unsearched. Only displayed text is evidence. Use read_file at listed lines; narrow the query for more.]";
    let mut rows = BTreeMap::new();
    let mut used = 0;
    let mut clipped = hits.len() > limit || scan_stopped;
    for hit in hits.iter().take(limit) {
        // Secure the match first, then its nearest context. A tiny byte budget
        // must not be spent on leading context without showing the actual hit.
        let mut lines: Vec<_> = hit.lines.iter().collect();
        lines.sort_by_key(|line| (line.number.abs_diff(hit.line), line.number));
        for line in lines {
            let key = (hit.path.as_str(), line.number);
            if rows.contains_key(&key) {
                continue;
            }
            let marker = if line.matched { ':' } else { '-' };
            let prefix = format!("{}:{}{} ", hit.path, line.number, marker);
            let available = bytes.saturating_sub(used + prefix.len() + 1 + NOTE.len());
            if available < 16 {
                clipped = true;
                continue;
            }
            let line_limit = if args.context_lines.unwrap_or(0) == 0 {
                available.min(266)
            } else {
                available
            };
            let (excerpt, partial) = excerpt(&line.text, pattern, line_limit);
            let entry = format!("{prefix}{excerpt}");
            used += entry.len() + 1;
            rows.insert(key, (entry, line.matched));
            clipped |= partial || line.clipped;
        }
    }
    let shown = rows.values().filter(|(_, matched)| *matched).count();
    let mut body = rows
        .into_values()
        .map(|(text, _)| text)
        .collect::<Vec<_>>()
        .join("\n");
    if clipped {
        body.push_str(NOTE);
    }
    let mut result = ToolOutcome::ok(format!("{shown} search match(es) shown"), body);
    result.content_complete = !clipped;
    Ok(result)
}

fn excerpt(line: &str, pattern: &regex::Regex, bytes: usize) -> (String, bool) {
    if line.len() <= bytes {
        return (line.into(), false);
    }
    let location = pattern.find(line).map_or(0, |m| m.start());
    let body_bytes = bytes.saturating_sub(10);
    let mut start = location.saturating_sub(64.min(body_bytes / 3));
    while !line.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (start + body_bytes).min(line.len());
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    (
        format!(
            "{}{}{}",
            if start > 0 { "[...]" } else { "" },
            &line[start..end],
            if end < line.len() { "[...]" } else { "" }
        ),
        true,
    )
}

#[cfg(test)]
#[path = "source_search_tests.rs"]
mod tests;

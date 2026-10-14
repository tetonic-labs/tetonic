//! Bounded search observations with locations for targeted source reads.
use crate::{GrepArgs, ToolError, ToolOutcome};

pub(super) fn format_hits(
    hits: &[(String, usize, String)],
    pattern: &regex::Regex,
    args: &GrepArgs,
) -> Result<ToolOutcome, ToolError> {
    let bytes = args.max_bytes.unwrap_or(2048);
    if bytes < 256 {
        return Err(ToolError::BadArgs("max_bytes must be at least 256".into()));
    }
    let bytes = bytes.min(4096);
    let limit = args.max_results.unwrap_or(crate::DEFAULT_GREP_RESULTS);
    const NOTE: &str = "\n[Search excerpts clipped or more matches omitted. Only displayed text is evidence. Use read_file at the listed lines; narrow the pattern/path for other matches.]";
    let mut body = String::new();
    let mut clipped = hits.len() > limit;
    let mut shown = 0;
    for (path, line_number, line) in hits.iter().take(limit) {
        let prefix = format!("{path}:{line_number}: ");
        let available = bytes.saturating_sub(body.len() + prefix.len() + 1 + NOTE.len());
        if available < 16 {
            clipped = true;
            break;
        }
        let (excerpt, partial) = excerpt(line, pattern, available.min(266));
        let entry = format!("{prefix}{excerpt}");
        if shown > 0 {
            body.push('\n');
        }
        body.push_str(&entry);
        shown += 1;
        clipped |= partial;
    }
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
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn large_unicode_matches_are_excerpts_with_actionable_locations() {
        let hits = (1..=80)
            .map(|n| {
                (
                    "sources/doc.md".into(),
                    n,
                    format!("{}needle{}", "資料🙂".repeat(200), "long text ".repeat(100)),
                )
            })
            .collect::<Vec<_>>();
        let args = serde_json::from_value(json!({"pattern":"needle","max_bytes":100000})).unwrap();
        let result = format_hits(&hits, &regex::Regex::new("needle").unwrap(), &args).unwrap();
        assert!(result.content.len() <= 4096);
        assert!(result.content.contains("sources/doc.md:1:"));
        assert!(result.content.contains("needle"));
        assert!(result.content.contains("Use read_file"));
        assert!(!result.content_complete);
        let small = serde_json::from_value(json!({"pattern":"needle","max_bytes":256})).unwrap();
        let result = format_hits(&hits, &regex::Regex::new("needle").unwrap(), &small).unwrap();
        assert!(result.content.len() <= 256);
        assert!(result.content.contains("sources/doc.md:1:"));
        assert!(result.content.contains("needle"));
    }

    #[test]
    fn small_results_remain_exact_and_missing_matches_are_not_invented() {
        let args = serde_json::from_value(json!({"pattern":"foo"})).unwrap();
        let pattern = regex::Regex::new("foo").unwrap();
        let result = format_hits(&[("a".into(), 2, "foo".into())], &pattern, &args).unwrap();
        assert_eq!(result.content, "a:2: foo");
        assert!(result.content_complete);
        assert_eq!(format_hits(&[], &pattern, &args).unwrap().content, "");
        let args = serde_json::from_value(json!({"pattern":"foo","max_bytes":0})).unwrap();
        assert!(format_hits(&[], &pattern, &args).is_err());
    }
}

//! Bounded observations of the already-authorized live or staged source.
use crate::{ReadFileArgs, ToolError, ToolOutcome};
use sha2::{Digest, Sha256};

const DEFAULT_BYTES: usize = 2048;
const MAX_BYTES: usize = 4096;

pub(super) fn page(text: &str, args: &ReadFileArgs) -> Result<ToolOutcome, ToolError> {
    let requested = args.max_bytes.unwrap_or(DEFAULT_BYTES);
    if requested < 4 {
        return Err(ToolError::BadArgs("max_bytes must be at least 4".into()));
    }
    let limit = requested.min(MAX_BYTES);
    let lines = args.start_line.is_some() || args.end_line.is_some();
    if lines && args.offset_bytes.is_some() {
        return Err(ToolError::BadArgs(
            "Use a line range or offset_bytes, not both".into(),
        ));
    }
    let digest = format!("{:x}", Sha256::digest(text.as_bytes()));
    if args
        .expected_sha256
        .as_ref()
        .is_some_and(|expected| expected != &digest)
    {
        return Err(ToolError::Other("Source changed since the previous read. Read the relevant range again without expected_sha256; do not combine the old and new versions.".into()));
    }
    let (start, requested_end) = if lines {
        let first = args.start_line.unwrap_or(1);
        let last = args.end_line.unwrap_or(usize::MAX);
        if first == 0 || last < first {
            return Err(ToolError::BadArgs(
                "Line numbers are one-based; end_line must be at least start_line".into(),
            ));
        }
        let mut offset = 0;
        let mut start = text.len();
        let mut end = text.len();
        for (i, line) in text.split_inclusive('\n').enumerate() {
            if i + 1 == first {
                start = offset;
            }
            offset += line.len();
            if i + 1 == last {
                end = offset;
                break;
            }
        }
        (start, end)
    } else {
        (args.offset_bytes.unwrap_or(0), text.len())
    };
    if start > text.len() || !text.is_char_boundary(start) {
        return Err(ToolError::BadArgs(
            "offset_bytes must be a UTF-8 character boundary within the file".into(),
        ));
    }
    let mut end = requested_end.min(start.saturating_add(limit));
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    // Prefer complete lines, but make progress through a single giant line too.
    if end < requested_end {
        if let Some(newline) = text[start..end].rfind('\n') {
            // Do not return only a heading/blank line when the next paragraph
            // fills most of the page. Byte cursors already support split lines.
            if newline + 1 >= limit / 2 {
                end = start + newline + 1;
            }
        }
    }
    let complete = end == requested_end;
    let content = if complete && lines {
        text[start..end]
            .lines()
            .enumerate()
            .map(|(i, line)| format!("{:>6}| {line}", args.start_line.unwrap_or(1) + i))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        text[start..end].to_owned()
    };
    let numbered_overflow = content.len() > limit;
    let content = if numbered_overflow {
        text[start..end].to_owned()
    } else {
        content
    };
    let mut outcome = ToolOutcome::ok(
        format!("read {} ({} bytes)", args.path, text.len()),
        content,
    );
    outcome.content_complete = complete;
    if !complete || args.offset_bytes.is_some() || numbered_overflow {
        let first_line = text[..start].bytes().filter(|b| *b == b'\n').count() + 1;
        let continuation = if complete {
            "none (requested range complete)".into()
        } else {
            serde_json::json!({"path":args.path,"offset_bytes":end,"max_bytes":limit,"expected_sha256":digest}).to_string()
        };
        outcome.summary = format!(
            "read page {} bytes {start}..{end}/{} ({}source)",
            args.path,
            text.len(),
            if complete {
                "complete requested "
            } else {
                "partial "
            }
        );
        outcome.content = format!("[SOURCE PAGE: {}]\nBytes {start}..{end} of {}; starts on line {first_line}. SHA-256: {digest}.\nNext read_file: {continuation}\nOnly the displayed bytes were read; omitted content is not evidence.{}\n--- source ---\n{}\n--- end page ---", serde_json::to_string(&args.path).unwrap_or_default(), text.len(), if lines && !complete { " Continuation uses file byte coordinates; stop when the requested line range is satisfied." } else { "" }, outcome.content);
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Tools, Workspace};
    use serde_json::{json, Value};

    fn next(outcome: &ToolOutcome) -> Value {
        serde_json::from_str(
            outcome
                .content
                .lines()
                .find_map(|line| line.strip_prefix("Next read_file: "))
                .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn pages_reconstruct_utf8_without_gaps_and_reject_stale_versions() {
        let dir = tempfile::tempdir().unwrap();
        let tools = Tools::new(Workspace::new(dir.path()).unwrap(), false);
        // Giant Unicode lines must remain pageable; newline-only cursors cannot do this.
        let source = "資料🙂abc".repeat(1200);
        std::fs::write(dir.path().join("large.txt"), &source).unwrap();
        let mut args = json!({"path":"large.txt"});
        let mut rebuilt = String::new();
        let mut pages = 0;
        loop {
            let result = tools.execute("read_file", &args);
            assert!(result.ok, "{result:?}");
            assert!(result.content.len() < DEFAULT_BYTES + 1000);
            let body = result
                .content
                .split_once("--- source ---\n")
                .unwrap()
                .1
                .strip_suffix("\n--- end page ---")
                .unwrap();
            rebuilt.push_str(body);
            pages += 1;
            if result.content_complete {
                break;
            }
            args = next(&result);
        }
        assert!(pages > 1);
        assert_eq!(rebuilt, source);
        std::fs::write(dir.path().join("large.txt"), "new version").unwrap();
        let stale = tools.execute("read_file", &args);
        assert!(!stale.ok);
        assert!(stale.content.contains("Source changed"));
    }

    #[test]
    fn a_short_heading_does_not_waste_the_following_page() {
        let text = format!("Heading\n{}", "paragraph ".repeat(200));
        let first = page(
            &text,
            &serde_json::from_value(json!({"path":"doc", "max_bytes":512})).unwrap(),
        )
        .unwrap();
        assert_eq!(next(&first)["offset_bytes"], 512);
        let second = page(&text, &serde_json::from_value(next(&first)).unwrap()).unwrap();
        assert_eq!(next(&second)["offset_bytes"], 1024);
        assert!(second.content.contains("starts on line 2"));
    }

    #[test]
    fn line_pages_small_reads_and_invalid_ranges_are_explicit() {
        let large = page(
            &"x".repeat(50000),
            &serde_json::from_value(json!({"path":"x","max_bytes":50000})).unwrap(),
        )
        .unwrap();
        assert!(!large.content_complete);
        assert!(large.content.len() < MAX_BYTES + 1000);
        assert_eq!(next(&large)["offset_bytes"], MAX_BYTES);
        let text = "a\nb\nc\n";
        let read = |value| page(text, &serde_json::from_value(value).unwrap());
        assert_eq!(read(json!({"path":"x"})).unwrap().content, text);
        assert_eq!(
            read(json!({"path":"x","start_line":2,"end_line":2}))
                .unwrap()
                .content,
            "     2| b"
        );
        let partial = read(json!({"path":"x","start_line":1,"end_line":3,"max_bytes":4})).unwrap();
        assert!(!partial.content_complete);
        assert_eq!(next(&partial)["offset_bytes"], 4);
        for bad in [
            json!({"path":"x","max_bytes":0}),
            json!({"path":"x","offset_bytes":99}),
            json!({"path":"x","start_line":0}),
            json!({"path":"x","start_line":3,"end_line":2}),
            json!({"path":"x","start_line":1,"offset_bytes":0}),
        ] {
            assert!(read(bad).is_err());
        }
        assert!(page(
            "資料",
            &serde_json::from_value(json!({"path":"x","offset_bytes":1})).unwrap()
        )
        .is_err());
        let blank = page(
            &"\n".repeat(2000),
            &serde_json::from_value(json!({"path":"x","start_line":1})).unwrap(),
        )
        .unwrap();
        assert!(
            blank.content.len() < 3000,
            "line numbers must not amplify an observation"
        );
    }

    #[test]
    fn staged_pages_are_private_and_continuations_recheck_workspace_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        let tools = Tools::new(Workspace::new(dir.path()).unwrap(), false);
        let other = Tools::new(Workspace::new(dir.path()).unwrap(), false);
        let source = "evidence\n".repeat(1000);
        assert!(
            tools
                .execute("write_file", &json!({"path":"draft.txt","content":source}))
                .ok
        );
        let first = tools.execute("read_file", &json!({"path":"draft.txt"}));
        assert!(first.ok && !first.content_complete);
        let mut args = next(&first);
        assert!(tools.execute("read_file", &args).ok);
        assert!(!other.execute("read_file", &args).ok);
        for path in ["../outside.txt", ".ssh/id_rsa", ".lokai/staging/private"] {
            args["path"] = json!(path);
            assert!(!tools.execute("read_file", &args).ok);
        }
        tools.mutation.abort_staged_if_any().unwrap();
        assert!(!dir.path().join("draft.txt").exists());
    }
}

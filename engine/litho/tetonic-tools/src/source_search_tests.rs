use super::*;
use serde_json::json;

fn search(text: &str, args: serde_json::Value) -> ToolOutcome {
    let args: GrepArgs = serde_json::from_value(args).unwrap();
    let pattern = regex::Regex::new(&args.pattern).unwrap();
    format_hits(
        &collect("doc.md", text, &pattern, &args),
        &pattern,
        &args,
        false,
    )
    .unwrap()
}

#[test]
fn context_reads_instructions_in_one_observation_and_merges_overlaps() {
    let text = "# Setup\nInstall Rust\n## Build\ncargo build\n## Launch\ntetonic ui\nOpen the printed URL\nUnrelated\n";
    let result = search(
        text,
        json!({"pattern":"^## (Build|Launch)$", "context_lines":1}),
    );
    assert_eq!(result.content, "doc.md:2- Install Rust\ndoc.md:3: ## Build\ndoc.md:4- cargo build\ndoc.md:5: ## Launch\ndoc.md:6- tetonic ui");
    assert!(result.content_complete);
    assert_eq!(result.summary, "2 search match(es) shown");
    // The command adjacent to a heading is available without another read.
    assert!(!search(text, json!({"pattern":"^## Build$"}))
        .content
        .contains("cargo build"));
}

#[test]
fn large_unicode_matches_are_excerpts_with_actionable_locations() {
    let text = (0..80)
        .map(|_| {
            format!(
                "{}needle{}\n",
                "資料🙂".repeat(200),
                "long text ".repeat(100)
            )
        })
        .collect::<String>();
    for bytes in [256, 512, 2048, 4096, 100000] {
        for context in [0, 1, 12] {
            let result = search(
                &text,
                json!({"pattern":"needle", "max_bytes":bytes, "context_lines":context}),
            );
            assert!(result.content.len() <= bytes.min(4096));
            assert!(result.content.contains("doc.md:1:"));
            assert!(result.content.contains("needle"));
            assert!(result.content.contains("Use read_file"));
            assert!(!result.content_complete);
        }
    }
}

#[test]
fn limited_context_keeps_the_match_instead_of_only_leading_lines() {
    let text = format!(
        "{}\nneedle\n{}",
        "before ".repeat(500),
        "after ".repeat(500)
    );
    let result = search(
        &text,
        json!({"pattern":"needle", "context_lines":12, "max_bytes":256}),
    );
    assert!(result.content.len() <= 256);
    assert!(result.content.contains("doc.md:2: needle"));
    assert!(!result.content_complete);
    // A path that itself cannot fit is never cut into a misleading location.
    let args = serde_json::from_value(json!({"pattern":"needle", "max_bytes":256})).unwrap();
    let pattern = regex::Regex::new("needle").unwrap();
    let result = format_hits(
        &collect(&"long/".repeat(100), "needle", &pattern, &args),
        &pattern,
        &args,
        false,
    )
    .unwrap();
    assert!(result.content.len() <= 256);
    assert_eq!(result.summary, "0 search match(es) shown");
    assert!(!result.content_complete);
}

#[test]
fn small_results_and_zero_context_remain_exact() {
    for context in [None, Some(0)] {
        let result = search(
            "bar\nfoo\n",
            json!({"pattern":"foo", "context_lines":context}),
        );
        assert_eq!(result.content, "doc.md:2: foo");
        assert!(result.content_complete);
    }
    let empty = search("bar", json!({"pattern":"foo", "context_lines":12}));
    assert_eq!(empty.content, "");
    assert!(empty.content_complete);
    let first = search(
        "foo\na\nb\nfoo",
        json!({"pattern":"foo", "context_lines":1, "max_results":1}),
    );
    assert!(first.content.contains("doc.md:1: foo"));
    assert!(!first.content.contains("doc.md:4:"));
    assert!(!first.content_complete);
}

#[test]
fn context_bounds_and_result_bounds_reject_invalid_requests() {
    for args in [
        json!({"pattern":"x", "max_bytes":255}),
        json!({"pattern":"x", "context_lines":13}),
        json!({"pattern":"x", "max_results":0}),
        json!({"pattern":"x", "max_results":1001}),
    ] {
        assert!(validate(&serde_json::from_value(args).unwrap()).is_err());
    }
}

#[test]
fn context_search_respects_grants_private_files_and_live_file_boundary() {
    use crate::{Tools, Workspace};
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("visible.md"),
        "heading\nneedle\nvisible detail",
    )
    .unwrap();
    for path in ["store.db", ".ssh/id_rsa", ".tetonic/hidden.txt"] {
        let file = dir.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, "needle\nPRIVATE_CANARY").unwrap();
    }
    let tools = Tools::new(Workspace::new(dir.path()).unwrap(), false)
        .protect_store_file(dir.path().join("store.db"));
    assert!(
        tools
            .execute(
                "write_file",
                &json!({"path":"visible.md", "content":"needle\nSTAGED_CANARY"})
            )
            .ok
    );
    assert!(
        tools
            .execute(
                "write_file",
                &json!({"path":"new.md", "content":"needle\nSTAGED_NEW"})
            )
            .ok
    );
    let args = json!({"pattern":"needle", "context_lines":1});
    let result = tools.execute("grep", &args);
    assert!(result.ok);
    assert!(result.content.contains("visible detail"));
    assert!(!result.content.contains("PRIVATE_CANARY"));
    assert!(!result.content.contains("STAGED_"));
    assert!(
        !tools
            .execute(
                "grep",
                &json!({"pattern":"needle", "path":"../outside", "context_lines":1})
            )
            .ok
    );
    // Exact forbidden file selection cannot bypass traversal protections.
    for path in ["store.db", ".ssh/id_rsa", ".tetonic/hidden.txt"] {
        let result = tools.execute(
            "grep",
            &json!({"pattern":"needle", "path":path, "context_lines":1}),
        );
        assert!(!result.content.contains("PRIVATE_CANARY"));
    }
    let tools = tools.with_allowed_tools(std::collections::HashSet::from(["read_file".into()]));
    assert!(
        !tools.execute("grep", &args).ok,
        "file read permission is not search permission"
    );
}

#[test]
fn search_retains_independent_locations_across_files() {
    let args = serde_json::from_value(json!({"pattern":"needle", "context_lines":1})).unwrap();
    let pattern = regex::Regex::new("needle").unwrap();
    let mut hits = collect("a.md", "needle\nfirst", &pattern, &args);
    hits.extend(collect("b.md", "needle\nsecond", &pattern, &args));
    let result = format_hits(&hits, &pattern, &args, false).unwrap();
    assert_eq!(
        result.content,
        "a.md:1: needle\na.md:2- first\nb.md:1: needle\nb.md:2- second"
    );
    // Formatting uses captured bytes, never a later filesystem read.
    assert!(result.content_complete);
}

#[test]
fn stopped_scan_is_incomplete_even_when_returned_matches_fit_the_limit() {
    let args = serde_json::from_value(json!({"pattern":"needle", "max_results":1})).unwrap();
    let pattern = regex::Regex::new("needle").unwrap();
    let hits = collect("a.md", "needle", &pattern, &args);
    let result = format_hits(&hits, &pattern, &args, true).unwrap();
    assert!(!result.content_complete);
    assert!(result.content.starts_with("a.md:1: needle\n"));
    assert!(result.content.contains("files unsearched"));
}

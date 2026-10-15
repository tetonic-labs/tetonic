use super::*;
use serde_json::json;
use tetonic_domain::ToolHost;

#[test]
fn batch_reads_ranges_pending_edits_and_reports_each_failure_without_claiming_completeness() {
    let dir = tempfile::tempdir().unwrap();
    let tools = Tools::new(Workspace::new(dir.path()).unwrap(), false);
    std::fs::write(dir.path().join("first.txt"), "first\nsecond\nthird\n").unwrap();
    assert!(
        tools
            .execute(
                "write_file",
                &json!({"path":"draft.txt","content":"private draft"})
            )
            .ok
    );
    let read = tools.execute(
        "read_files",
        &json!({"files":[{"path":"first.txt","start_line":2,"end_line":2},{"path":"draft.txt"}]}),
    );
    assert!(read.ok && read.content_complete, "{read:?}");
    assert!(read.content.contains("     2| second"));
    assert!(read.content.contains("private draft"));
    assert!(!dir.path().join("draft.txt").exists());
    for path in [
        "../outside",
        ".ssh/id_rsa",
        ".lokai/staging/private",
        "missing.txt",
    ] {
        let result = tools.execute(
            "read_files",
            &json!({"files":[{"path":"first.txt"},{"path":path}]}),
        );
        assert!(!result.ok && !result.content_complete, "{result:?}");
        assert!(result.content.contains("first\nsecond"));
        assert!(result.content.contains("failed"));
    }
    let restricted = tools.with_allowed_tools(["read_files".into()].into_iter().collect());
    assert!(
        restricted
            .execute("read_files", &json!({"files":[{"path":"first.txt"}]}))
            .ok
    );
    assert!(
        !restricted
            .execute("read_file", &json!({"path":"first.txt"}))
            .ok
    );
    assert!(!restricted.execute("inspect_changes", &json!({})).ok);
}

#[test]
fn batch_shares_source_budget_and_rejects_invalid_or_stale_reads() {
    let dir = tempfile::tempdir().unwrap();
    let tools = Tools::new(Workspace::new(dir.path()).unwrap(), false);
    std::fs::write(dir.path().join("large.txt"), "資料🙂".repeat(1000)).unwrap();
    let args = json!({"files":[{"path":"large.txt"},{"path":"large.txt"}],"max_bytes":100});
    let result = tools.execute("read_files", &args);
    assert!(result.ok && !result.content_complete, "{result:?}");
    let source_bytes: usize = result
        .content
        .split("--- source ---\n")
        .skip(1)
        .map(|s| s.split("\n--- end page ---").next().unwrap().len())
        .sum();
    assert!(source_bytes > 80 && source_bytes <= 100, "{source_bytes}");
    assert_eq!(result.content.matches("Next read_file:").count(), 2);
    for args in [
        json!({"files":[]}),
        json!({"files":vec![json!({"path":"a"});5]}),
        json!({"files":[{"path":"large.txt","expected_sha256":"stale"}]}),
        json!({"files":[{"path":"large.txt"}],"max_bytes":0}),
    ] {
        assert!(!tools.execute("read_files", &args).ok);
    }
    let cap = tools.limit_observation("read_files", &args, 64).unwrap();
    assert_eq!(cap["max_bytes"], 64);
    assert_eq!(cap["files"], args["files"]);
    let scope = tetonic_domain::work_scope::WorkScope::default();
    scope.cancel();
    assert!(
        !tools
            .execute_authorized_cancellable(
                "read_files",
                &args,
                None,
                Some(&scope.cancellation_signal())
            )
            .ok
    );
}

#[test]
fn inspection_is_private_paged_and_invalidated_by_changes() {
    let dir = tempfile::tempdir().unwrap();
    let tools = Tools::new(Workspace::new(dir.path()).unwrap(), false);
    let observer = Tools::new(Workspace::new(dir.path()).unwrap(), false);
    assert!(tools
        .execute("inspect_changes", &json!({}))
        .content
        .contains("No pending"));
    assert!(tools.staged_transaction_preview().unwrap().is_none());
    std::fs::write(dir.path().join("a.txt"), "original\n").unwrap();
    assert!(
        tools
            .execute("write_file", &json!({"path":"a.txt","content":"new\n"}))
            .ok
    );
    let full = tools.execute("inspect_changes", &json!({}));
    assert!(full.ok && full.content_complete, "{full:?}");
    assert!(full.content.contains("-original\n+new\n"), "{full:?}");
    assert!(full.content.contains("not checked"));
    assert!(observer
        .execute("inspect_changes", &json!({}))
        .content
        .contains("No pending"));
    let mut args = json!({"max_bytes":40});
    let mut text = String::new();
    loop {
        let page = tools.execute("inspect_changes", &args);
        assert!(page.ok, "{page:?}");
        text.push_str(
            page.content
                .split("--- report ---\n")
                .nth(1)
                .unwrap()
                .split("\n--- end page ---")
                .next()
                .unwrap(),
        );
        if page.content_complete {
            break;
        }
        args = serde_json::from_str(
            page.content
                .lines()
                .find_map(|s| s.strip_prefix("Next inspect_changes: "))
                .unwrap(),
        )
        .unwrap();
    }
    assert!(full.content.contains(&text));
    assert!(
        tools
            .execute("write_file", &json!({"path":"a.txt","content":"changed\n"}))
            .ok
    );
    assert!(!tools.execute("inspect_changes", &args).ok);
    std::fs::write(dir.path().join("a.txt"), "peer change").unwrap();
    let conflict = tools.execute("inspect_changes", &json!({}));
    assert!(conflict.content.contains("live file changed"));
    assert!(!conflict.content.contains("-peer change"));
}

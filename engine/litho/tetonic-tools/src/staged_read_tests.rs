use super::*;
use serde_json::json;

#[test]
fn read_own_staged_file_without_publishing_it_to_other_agents() {
    let dir = tempfile::tempdir().unwrap();
    let tools = Tools::new(Workspace::new(dir.path()).unwrap(), false);
    let observer = Tools::new(Workspace::new(dir.path()).unwrap(), false);
    let written = tools.execute(
        "write_file",
        &json!({"path":"result.md","content":"first\nsecond\n"}),
    );
    assert!(written.ok, "{written:?}");
    let read = tools.execute(
        "read_file",
        &json!({"path":"./result.md","start_line":2,"end_line":2}),
    );
    assert!(read.ok, "{read:?}");
    assert_eq!(read.content, "     2| second");
    assert!(!dir.path().join("result.md").exists());
    assert!(
        !observer
            .execute("read_file", &json!({"path":"result.md"}))
            .ok
    );
    tools.mutation.abort_staged_if_any().unwrap();
    assert!(!tools.execute("read_file", &json!({"path":"result.md"})).ok);
}

#[test]
fn staged_overwrite_is_readable_but_abort_restores_the_live_view() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("result.md"), "original").unwrap();
    let tools = Tools::new(Workspace::new(dir.path()).unwrap(), false);
    for content in ["revision one", "revision two"] {
        assert!(
            tools
                .execute("write_file", &json!({"path":"result.md","content":content}))
                .ok
        );
        let read = tools.execute("read_file", &json!({"path":"result.md"}));
        assert!(read.ok, "{read:?}");
        assert_eq!(read.content, content);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("result.md")).unwrap(),
            "original"
        );
    }
    for path in ["../outside.md", ".lokai/staging/secret", ".ssh/id_rsa"] {
        assert!(!tools.execute("read_file", &json!({"path":path})).ok);
    }
    tools.mutation.abort_staged_if_any().unwrap();
    assert_eq!(
        tools
            .execute("read_file", &json!({"path":"result.md"}))
            .content,
        "original"
    );
}

#[test]
fn edits_compose_with_own_drafts_and_remain_private_until_commit() {
    for existed in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        if existed {
            std::fs::write(dir.path().join("result.md"), "original").unwrap();
        }
        let tools = Tools::new(Workspace::new(dir.path()).unwrap(), false);
        let observer = Tools::new(Workspace::new(dir.path()).unwrap(), false);
        assert!(
            tools
                .execute(
                    "write_file",
                    &json!({"path":"result.md","content":"first\nsecond\n"})
                )
                .ok
        );
        for (old, new) in [("first", "repaired"), ("second", "checked")] {
            let args = json!({"path":"./result.md", "old_string":old, "new_string":new});
            assert!(
                !observer.execute("edit_file", &args).ok,
                "another agent cannot edit this draft"
            );
            let edited = tools.execute("edit_file", &args);
            assert!(edited.ok, "{edited:?}");
        }
        assert_eq!(
            tools
                .execute("read_file", &json!({"path":"result.md"}))
                .content,
            "repaired\nchecked\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("result.md")).ok(),
            existed.then(|| "original".into())
        );
        tools.mutation.commit_staged_if_any().unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("result.md")).unwrap(),
            "repaired\nchecked\n"
        );
    }
}

#[test]
fn repeated_write_reports_the_previous_pending_contents() {
    let dir = tempfile::tempdir().unwrap();
    let tools = Tools::new(Workspace::new(dir.path()).unwrap(), false);
    assert!(
        tools
            .execute("write_file", &json!({"path":"result.md","content":"draft"}))
            .ok
    );
    let rewritten = tools.execute(
        "write_file",
        &json!({"path":"result.md","content":"revised"}),
    );
    assert!(rewritten.ok, "{rewritten:?}");
    assert_eq!(rewritten.change.unwrap().before.as_deref(), Some("draft"));
    assert!(!dir.path().join("result.md").exists());
}

#[test]
fn small_non_python_edits_do_not_require_copying_the_whole_workspace() {
    let dir = tempfile::tempdir().unwrap();
    let tools = Tools::new(Workspace::new(dir.path()).unwrap(), false);
    // Too large for a verification overlay, but unrelated to this small edit.
    std::fs::File::create(dir.path().join("large.dat"))
        .unwrap()
        .set_len(257 * 1024 * 1024)
        .unwrap();
    std::fs::write(dir.path().join("result.md"), "original").unwrap();
    let written = tools.execute("write_file", &json!({"path":"result.md","content":"draft"}));
    assert!(written.ok, "{written:?}");
    let edited = tools.execute(
        "edit_file",
        &json!({"path":"result.md","old_string":"draft","new_string":"ready"}),
    );
    assert!(edited.ok, "{edited:?}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("result.md")).unwrap(),
        "original"
    );
    tools.mutation.abort_staged_if_any().unwrap();
}

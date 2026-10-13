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

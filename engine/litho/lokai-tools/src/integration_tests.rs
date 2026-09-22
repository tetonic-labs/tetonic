use std::path::PathBuf;

use super::*;
use crate::workspace::refresh_mutating_path;
use serde_json::json;

#[test]
fn tools_new_defaults_to_sandboxed() {
    let dir = std::env::temp_dir().join(format!("lokai-tools-r62-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let tools = Tools::new(Workspace::new(&dir).unwrap(), false);
    assert_eq!(
        tools.enforcement_level(),
        EnforcementLevel::Sandboxed,
        "R6-2: production Tools::new must use OS sandbox"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

fn tmp_ws(tag: &str) -> (Tools, PathBuf) {
    let dir = std::env::temp_dir().join(format!("lokai-tools-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let tools = Tools::new(Workspace::new(&dir).unwrap(), false);
    (tools, dir)
}

#[test]
fn grep_is_parallel_safe_and_deterministically_ordered() {
    let (tools, dir) = tmp_ws("grep");
    // foo appears on lines 1 & 3 of a.txt and line 2 of b.txt.
    std::fs::write(dir.join("a.txt"), "foo\nbar\nfoo\n").unwrap();
    std::fs::write(dir.join("b.txt"), "bar\nfoo\n").unwrap();
    std::fs::write(dir.join("c.txt"), "nothing here\n").unwrap();

    let out = tools.execute("grep", &json!({ "pattern": "foo" }));
    dbg!(&out);
    assert!(out.ok);
    let lines: Vec<&str> = out.content.lines().collect();
    // Parallel walk, but output must be sorted by (path, line) every run.
    assert_eq!(lines, vec!["a.txt:1: foo", "a.txt:3: foo", "b.txt:2: foo"]);

    // Limit is respected.
    let limited = tools.execute("grep", &json!({ "pattern": "foo", "max_results": 1 }));
    assert_eq!(limited.content.lines().count(), 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn glob_matches_and_sorts() {
    let (tools, dir) = tmp_ws("glob");
    std::fs::write(dir.join("z.rs"), "").unwrap();
    std::fs::write(dir.join("a.rs"), "").unwrap();
    std::fs::write(dir.join("m.py"), "").unwrap();

    let out = tools.execute("glob", &json!({ "pattern": "*.rs" }));
    dbg!(&out);
    assert!(out.ok);
    let lines: Vec<&str> = out.content.lines().collect();
    assert_eq!(lines, vec!["a.rs", "z.rs"]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn resolve_rejects_symlink_escape() {
    let dir = std::env::temp_dir().join(format!("lokai-tools-symlink-{}", std::process::id()));
    let outside = std::env::temp_dir().join(format!("lokai-tools-outside-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&outside);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), "secret").unwrap();

    let link = dir.join("escape");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    #[cfg(windows)]
    {
        if let Err(e) = std::os::windows::fs::symlink_dir(&outside, &link) {
            eprintln!("skipping resolve_rejects_symlink_escape (need symlink privilege): {e}");
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::remove_dir_all(&outside);
            return;
        }
    }

    let ws = Workspace::new(&dir).unwrap();
    assert!(matches!(
        ws.resolve("escape/secret.txt"),
        Err(ToolError::OutsideWorkspace(_))
    ));

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&outside);
}

#[test]
#[ignore = "superseded by transaction manager verify step"]
fn syntax_error_reverts_when_flag_set() {
    let prev = std::env::var("LOKAI_REVERT_ON_SYNTAX_ERROR").ok();
    std::env::set_var("LOKAI_REVERT_ON_SYNTAX_ERROR", "1");
    let (tools, dir) = tmp_ws("syntax-revert");
    std::fs::write(dir.join("broken.py"), "def ok():\n    return 1\n").unwrap();

    let out = tools.execute(
        "edit_file",
        &json!({
            "path": "broken.py",
            "old_string": "def ok():",
            "new_string": "def ok(\n"
        }),
    );
    dbg!(&out);
    assert!(!out.ok);
    let content = std::fs::read_to_string(dir.join("broken.py")).unwrap();
    assert!(
        content.contains("def ok():"),
        "expected revert, got: {content}"
    );

    match prev {
        Some(v) => std::env::set_var("LOKAI_REVERT_ON_SYNTAX_ERROR", v),
        None => std::env::remove_var("LOKAI_REVERT_ON_SYNTAX_ERROR"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn refresh_mutating_path_rejects_symlink_parent_race() {
    let dir = std::env::temp_dir().join(format!("lokai-tools-write-race-{}", std::process::id()));
    let outside =
        std::env::temp_dir().join(format!("lokai-tools-write-outside-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&outside);
    std::fs::create_dir_all(dir.join("subdir")).unwrap();
    std::fs::create_dir_all(&outside).unwrap();

    let ws = Workspace::new(&dir).unwrap();
    let target = ws.resolve("subdir/new.txt").unwrap();

    let link = dir.join("subdir");
    #[cfg(unix)]
    {
        std::fs::remove_dir(&link).unwrap();
        std::os::unix::fs::symlink(&outside, &link).unwrap();
    }
    #[cfg(windows)]
    {
        let _ = std::fs::remove_dir_all(&link);
        if std::os::windows::fs::symlink_dir(&outside, &link).is_err() {
            eprintln!("skipping refresh_mutating_path_rejects_symlink_parent_race (need symlink privilege)");
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::remove_dir_all(&outside);
            return;
        }
    }

    assert!(matches!(
        refresh_mutating_path(&ws, &target),
        Err(ToolError::OutsideWorkspace(_))
    ));

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&outside);
}

#[test]
fn empty_old_string_rejected() {
    let (tools, dir) = tmp_ws("empty-old");
    std::fs::write(dir.join("t.txt"), "hello").unwrap();
    let ws = tools.workspace().clone();
    let err = tools
        .mutation_service()
        .edit_file(
            &ws,
            None,
            json!({
                "path": "t.txt",
                "old_string": "",
                "new_string": "x"
            }),
        )
        .unwrap_err();
    assert!(matches!(err, ToolError::BadArgs(_)));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn coerce_args_rejects_invalid_json_string() {
    let err = types::coerce_args(&serde_json::Value::String("not-json".into())).unwrap_err();
    assert!(matches!(err, ToolError::BadArgs(_)));
}

#[test]
fn finish_dispatches_via_execute() {
    let (tools, dir) = tmp_ws("finish");
    let out = tools.execute("finish", &json!({ "summary": "done testing" }));
    dbg!(&out);
    assert!(out.ok);
    assert!(out.content.contains("done testing"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn execute_authorized_routes_mutation_with_capability() {
    use lokai_domain::{ActionId, ActionKind, DataClass, IssuedCapability, ProposedAction};

    let (tools, dir) = tmp_ws("auth-mut");
    use lokai_domain::{AgentId, SessionId};
    let wv = lokai_transaction::version::capture_workspace_version(&dir, &[]).unwrap();
    let action = ProposedAction {
        action_id: ActionId::new("mut_auth"),
        session_id: SessionId::new("s"),
        run_id: None,
        task_id: None,
        attempt_id: None,
        agent_id: Some(AgentId::new("a0")),
        workspace_version: Some(wv),
        data_class: DataClass::RepositorySource,
        kind: ActionKind::WriteFile,
        parameters: lokai_domain::execution::CanonicalActionParameters {
            digest: "digest".into(),
            executable_identity: Some("write_file".into()),
            resolved_path: None,
            arguments: vec![],
            shell_identity: None,
            shell_mode: None,
            script_bytes: None,
            working_directory: None,
            env_vars: None,
            stdin_source_classification: None,
            filesystem_access_scope: None,
            network_policy: None,
            resource_limits: None,
            process_class: None,
            sandbox_profile: None,
            expected_output_limits: None,
            schema_version: 1,
            tool_arguments: Some(json!({ "path": "out.txt", "content": "via sink" })),
        },
        requested_capabilities: Default::default(),
        trace_context: Default::default(),
    };
    let authorized = lokai_domain::AuthorizedAction {
        capability: IssuedCapability {
            capability_id: lokai_domain::CapabilityId::new("cap_auth"),
            session_id: action.session_id.clone(),
            run_id: None,
            task_id: None,
            attempt_id: None,
            agent_id: action.agent_id.clone(),
            action_kind: ActionKind::WriteFile,
            canonical_parameter_digest: action.parameters.digest.clone(),
            workspace_version: action.workspace_version.clone(),
            data_classification: action.data_class,
            issuance_timestamp: 0,
            expiration: 0,
            max_use_count: 1,
            current_use_count: 0,
            issuing_policy_version: "v1".into(),
            approval_record_id: None,
            revoked: false,
        },
        action,
    };
    let out = tools.execute_authorized(
        "write_file",
        &json!({ "path": "out.txt", "content": "via sink" }),
        Some(&authorized),
    );
    dbg!(&out);
    assert!(out.ok);
    let commit_out = tools.commit_staged_if_any().unwrap();
    assert!(commit_out.is_some());
    assert_eq!(
        std::fs::read_to_string(dir.join("out.txt")).unwrap(),
        "via sink"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn read_without_capability_is_denied_when_consumer_bound() {
    use std::sync::Arc;

    let dir = std::env::temp_dir().join(format!("lokai-tools-r61-read-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), "hello\n").unwrap();

    struct DenyAll;
    impl lokai_domain::CapabilityConsumer for DenyAll {
        fn authorize(
            &self,
            _: &lokai_domain::AuthorizedAction,
        ) -> Result<(), lokai_domain::CapabilityError> {
            Err(lokai_domain::CapabilityError::ScopeMismatch)
        }
    }
    let gated =
        Tools::new(Workspace::new(&dir).unwrap(), true).with_capability_consumer(Arc::new(DenyAll));
    let denied = gated.execute("read_file", &json!({ "path": "a.txt" }));
    assert!(!denied.ok, "R6-1: read without issued capability must deny");
    assert!(
        denied.content.contains("capability required"),
        "got: {}",
        denied.content
    );
    let shell = gated.execute("run_shell", &json!({ "command": "echo hi" }));
    assert!(
        !shell.ok && shell.content.contains("capability required"),
        "R6-1: allow_shell without issue must deny; got: {}",
        shell.content
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// R10: verify default uses staged overlay; failed verify does not commit.
#[test]
fn staged_failing_verify_leaves_workspace_uncommitted() {
    let dir = std::env::temp_dir().join(format!(
        "lokai-tools-r10-fail-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let tools = Tools::new(Workspace::new(&dir).unwrap(), false)
        .with_enforcement_level(EnforcementLevel::Constrained);
    std::fs::write(dir.join("f.txt"), "live-ok\n").unwrap();
    // Script expects live content; on overlay it sees staged-bad and exits 1.
    std::fs::write(
        dir.join("check_verify.py"),
        "import pathlib,sys\n\
         t=pathlib.Path('f.txt').read_text()\n\
         sys.exit(0 if t=='live-ok\\n' else 1)\n",
    )
    .unwrap();
    let staged = tools.execute(
        "write_file",
        &json!({ "path": "f.txt", "content": "staged-bad\n" }),
    );
    assert!(staged.ok, "stage write: {}", staged.content);
    assert_eq!(
        std::fs::read_to_string(dir.join("f.txt")).unwrap(),
        "live-ok\n"
    );

    let overlay = tools
        .verification_overlay_if_staged()
        .unwrap()
        .expect("open stage must materialize verify overlay");
    assert_eq!(
        std::fs::read_to_string(overlay.join("f.txt")).unwrap(),
        "staged-bad\n",
        "verify view must show staged bytes"
    );

    let (ok, out) = tools.run_command("python check_verify.py");
    assert!(
        !ok,
        "verify must fail on staged-bad (missing python also fails the run); out={out}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("f.txt")).unwrap(),
        "live-ok\n",
        "failed verify must leave live tree uncommitted"
    );
    assert!(
        tools.commit_staged().is_err(),
        "failed verify must block commit"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// R10: successful verify-on-stage still allows finish/commit.
#[test]
fn staged_passing_verify_allows_commit() {
    let dir = std::env::temp_dir().join(format!(
        "lokai-tools-r10-pass-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let tools = Tools::new(Workspace::new(&dir).unwrap(), false)
        .with_enforcement_level(EnforcementLevel::Constrained);
    std::fs::write(dir.join("f.txt"), "live-ok\n").unwrap();
    std::fs::write(
        dir.join("check_verify.py"),
        "import pathlib,sys\n\
         t=pathlib.Path('f.txt').read_text()\n\
         sys.exit(0 if t=='staged-good\\n' else 1)\n",
    )
    .unwrap();
    let staged = tools.execute(
        "write_file",
        &json!({ "path": "f.txt", "content": "staged-good\n" }),
    );
    assert!(staged.ok, "stage write: {}", staged.content);

    let python_available = std::process::Command::new("python")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if python_available {
        let (ok, out) = tools.run_command("python check_verify.py");
        assert!(ok, "verify must pass on staged-good; out={out}");
    } else {
        let _ = tools
            .verification_overlay_if_staged()
            .unwrap()
            .expect("overlay");
        tools
            .finish_verification_run(
                "python check_verify.py",
                true,
                "python unavailable",
                Some(0),
            )
            .unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(dir.join("f.txt")).unwrap(),
        "live-ok\n",
        "live tree stays until commit"
    );
    let committed = tools.commit_staged().expect("passing verify allows commit");
    assert!(committed.artifact.commit_succeeded);
    assert_eq!(
        std::fs::read_to_string(dir.join("f.txt")).unwrap(),
        "staged-good\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn edit_file_fuzzy_auto_aligns_whitespace_and_crlf() {
    let (tools, dir) = tmp_ws("fuzzy-edit");
    let file = dir.join("code.py");
    std::fs::write(
        &file,
        "class Service:\r\n  def run(self):   \r\n    pass\r\n",
    )
    .unwrap();

    // Model provides 4 spaces and LF line endings
    let outcome = tools.execute(
        "edit_file",
        &json!({
            "path": "code.py",
            "old_string": "    def run(self):\n        pass",
            "new_string": "    def run(self):\n        return 42"
        }),
    );
    assert!(
        outcome.ok,
        "edit_file should succeed via fuzzy alignment: {}",
        outcome.content
    );
    let committed = tools.commit_staged().unwrap();
    assert!(committed.artifact.commit_succeeded);

    let content = std::fs::read_to_string(&file).unwrap();
    assert!(content.contains("return 42"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn read_file_typo_returns_fuzzy_path_suggestion() {
    let (tools, dir) = tmp_ws("fuzzy-read");
    let file = dir.join("configuration.rs");
    std::fs::write(&file, "pub struct Config;\n").unwrap();

    // Model makes a typo: "configuraton.rs"
    let outcome = tools.execute("read_file", &json!({ "path": "configuraton.rs" }));
    assert!(!outcome.ok);
    assert!(outcome
        .content
        .contains("path 'configuraton.rs' does not exist. Did you mean 'configuration.rs'?"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn read_file_refuses_parentdir_escape() {
    let (tools, dir) = tmp_ws("read-escape");
    let out = tools.execute("read_file", &json!({ "path": "../secret.txt" }));
    assert!(!out.ok);
    assert!(
        !out.content.contains("super secret"),
        "escaped bytes must not appear: {}",
        out.content
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn write_file_none_version_is_denied() {
    use lokai_domain::{
        ActionId, ActionKind, AgentId, DataClass, IssuedCapability, ProposedAction, SessionId,
    };

    let (tools, dir) = tmp_ws("none-ver");
    let action = ProposedAction {
        action_id: ActionId::new("mut_none"),
        session_id: SessionId::new("s"),
        run_id: None,
        task_id: None,
        attempt_id: None,
        agent_id: Some(AgentId::new("a0")),
        workspace_version: None,
        data_class: DataClass::RepositorySource,
        kind: ActionKind::WriteFile,
        parameters: lokai_domain::execution::CanonicalActionParameters {
            digest: "digest".into(),
            executable_identity: Some("write_file".into()),
            resolved_path: Some("out.txt".into()),
            arguments: vec![],
            shell_identity: None,
            shell_mode: None,
            script_bytes: None,
            working_directory: Some(dir.display().to_string()),
            env_vars: None,
            stdin_source_classification: None,
            filesystem_access_scope: None,
            network_policy: None,
            resource_limits: None,
            process_class: None,
            sandbox_profile: None,
            expected_output_limits: None,
            schema_version: 1,
            tool_arguments: Some(json!({ "path": "out.txt", "content": "nope" })),
        },
        requested_capabilities: Default::default(),
        trace_context: Default::default(),
    };
    let authorized = lokai_domain::AuthorizedAction {
        capability: IssuedCapability {
            capability_id: lokai_domain::CapabilityId::new("cap_none"),
            session_id: action.session_id.clone(),
            run_id: None,
            task_id: None,
            attempt_id: None,
            agent_id: action.agent_id.clone(),
            action_kind: ActionKind::WriteFile,
            canonical_parameter_digest: action.parameters.digest.clone(),
            workspace_version: None,
            data_classification: action.data_class,
            issuance_timestamp: 0,
            expiration: u64::MAX,
            max_use_count: 1,
            current_use_count: 0,
            issuing_policy_version: "v1".into(),
            approval_record_id: None,
            revoked: false,
        },
        action,
    };
    let out = tools.execute_authorized(
        "write_file",
        &json!({ "path": "out.txt", "content": "nope" }),
        Some(&authorized),
    );
    assert!(!out.ok);
    assert!(!dir.join("out.txt").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

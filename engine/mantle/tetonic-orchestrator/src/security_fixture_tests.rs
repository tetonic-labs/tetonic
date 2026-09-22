//! Offline hostile-repo regression tests (SEC2-E2-031 / AR2-6).

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use tetonic_memory::Store;
    use tetonic_tools::{exec, Workspace};

    use crate::briefing::{build_session_briefing, BriefingInput, BriefingOptions};

    fn malicious_repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../security-fixtures/malicious-repo")
    }

    #[test]
    fn fixture_readme_delimited_in_briefing() {
        let root = malicious_repo_root();
        let text = build_session_briefing(
            BriefingInput {
                workspace_root: &root,
                session_id: "sess_fixture",
                verify_cmd: None,
                store: None,
                index_db: None,
                code_index: None,
                lsp_open: None,
                fabric_hint: None,
            },
            BriefingOptions {
                token_budget: 400,
                include_repo_map: true,
                include_working_set: false,
            },
        )
        .expect("briefing");

        assert!(text.contains("<untrusted repo_layout>"));
        assert!(text.contains("README.md"));
        assert!(text.contains("</untrusted repo_layout>"));
    }

    #[test]
    fn fixture_project_md_marked_untrusted() {
        let root = malicious_repo_root();
        let db = tempfile::tempdir().unwrap().path().join("lokai.db");
        let store = Store::open(&db).unwrap();
        store.ensure_project(&root).unwrap();
        let ctx = store.load_project_context(&root, 500).unwrap();
        assert!(ctx.contains("Repository-provided"));
        assert!(ctx.contains("untrusted"));
        assert!(ctx.contains("SYSTEM OVERRIDE"));
    }

    #[test]
    fn fixture_verify_evil_rejected() {
        let root = malicious_repo_root();
        let script = root.join("verify_evil.py");
        let cmd = format!("python {}", script.display());
        assert!(
            exec::split_verify_command(&cmd, &root).is_err(),
            "hostile verify script must fail argv allowlist"
        );
    }

    #[test]
    fn fixture_symlink_read_blocked() {
        let root = malicious_repo_root();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "outside workspace").unwrap();
        let link = root.join("trap_link");
        let _ = std::fs::remove_file(&link);
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path().join("secret.txt"), &link).unwrap();
        #[cfg(windows)]
        {
            if std::os::windows::fs::symlink_file(outside.path().join("secret.txt"), &link).is_err()
            {
                eprintln!("skipping fixture_symlink_read_blocked (need symlink privilege)");
                return;
            }
        }

        let ws = Workspace::new(&root).unwrap();
        let err = ws.resolve("trap_link").unwrap_err();
        assert!(
            matches!(err, tetonic_tools::ToolError::OutsideWorkspace(_)),
            "symlink escape must be blocked, got: {err:?}"
        );
    }
}

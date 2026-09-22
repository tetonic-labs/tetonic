//! Optional git worktree per session (D2). Enable with `LOKAI_USE_WORKTREE=1`.

use std::path::{Path, PathBuf};

use crate::process_executor::{coding_executor, EnforcementLevel};

/// Whether session-scoped git worktrees are enabled.
pub fn session_worktree_enabled() -> bool {
    std::env::var("LOKAI_USE_WORKTREE")
        .ok()
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

/// Create (or reuse) a git worktree for `session_id` under `.lokai/worktrees/`.
/// Returns the main workspace root when git is unavailable or worktrees are disabled.
pub fn ensure_session_worktree(main_root: &Path, session_id: &str) -> Result<PathBuf, String> {
    if !session_worktree_enabled() {
        return Ok(main_root.to_path_buf());
    }
    if !main_root.join(".git").exists() {
        return Err("worktree isolation requires a git repository".into());
    }
    let wt_root = main_root.join(".lokai").join("worktrees").join(session_id);
    if wt_root.exists() {
        return std::fs::canonicalize(&wt_root).map_err(|e| e.to_string());
    }
    let parent = wt_root
        .parent()
        .ok_or_else(|| "worktree path has no parent directory".to_string())?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let executor = coding_executor(main_root, EnforcementLevel::Sandboxed);
    let status = executor
        .run_git_status(&[
            "worktree",
            "add",
            "--detach",
            wt_root.to_str().unwrap_or_default(),
            "HEAD",
        ])
        .map_err(|e| format!("git worktree add failed: {e}"))?;
    if !status {
        let branch = format!("lokai/{}", &session_id[..session_id.len().min(12)]);
        let status2 = executor
            .run_git_status(&[
                "worktree",
                "add",
                "-b",
                &branch,
                wt_root.to_str().unwrap_or_default(),
                "HEAD",
            ])
            .map_err(|e| format!("git worktree add failed: {e}"))?;
        if !status2 {
            return Err("git worktree add returned non-zero".into());
        }
    }
    std::fs::canonicalize(&wt_root).map_err(|e| e.to_string())
}

/// Remove a session worktree (best-effort).
pub fn remove_session_worktree(main_root: &Path, session_id: &str) {
    if !session_worktree_enabled() {
        return;
    }
    let wt_root = main_root.join(".lokai").join("worktrees").join(session_id);
    if !wt_root.exists() {
        return;
    }
    let executor = coding_executor(main_root, EnforcementLevel::Sandboxed);
    let _ = executor.run_git_status(&[
        "worktree",
        "remove",
        "--force",
        wt_root.to_str().unwrap_or_default(),
    ]);
    let _ = std::fs::remove_dir_all(&wt_root);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static WT_ENV: Mutex<()> = Mutex::new(());

    #[test]
    fn disabled_returns_main_root() {
        let _lock = WT_ENV.lock().unwrap();
        std::env::remove_var("LOKAI_USE_WORKTREE");
        let root = std::env::temp_dir().join(format!("lokai-wt-off-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let out = ensure_session_worktree(&root, "sess_x").unwrap();
        assert_eq!(out, root);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn enabled_requires_git() {
        let _lock = WT_ENV.lock().unwrap();
        std::env::set_var("LOKAI_USE_WORKTREE", "1");
        struct ClearEnv;
        impl Drop for ClearEnv {
            fn drop(&mut self) {
                std::env::remove_var("LOKAI_USE_WORKTREE");
            }
        }
        let _clear = ClearEnv;
        let root = std::env::temp_dir().join(format!("lokai-wt-on-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        assert!(ensure_session_worktree(&root, "sess_y").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}

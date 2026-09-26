//! Workspace verification command auto-discovery (D8).
//!
//! Hosts call [`resolve_verify_cmd`] at session start to pick a verify-before-finish
//! command. Explicit operator overrides win; otherwise we detect pytest/cargo/bench
//! conventions when confidence is high.

use std::path::Path;

/// How sure we are that the detected command is the right one for this workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VerifyConfidence {
    Low,
    High,
}

/// A candidate verification command plus metadata for logging / audit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyDetect {
    pub command: String,
    /// Human-readable label, e.g. `"pytest"`, `"cargo test"`, `"_lokai_verify.py"`.
    pub source: &'static str,
    pub confidence: VerifyConfidence,
}

/// Resolve the verify command for a session.
///
/// - `explicit` = `Some("none"|"off")` → disabled.
/// - `explicit` = `Some("auto")` → run detection; use result even if low confidence
///   is not returned (only high/medium detections are returned today).
/// - `explicit` = `Some(cmd)` → use `cmd` verbatim.
/// - `explicit` = `None` → auto-detect only when confidence is [`VerifyConfidence::High`].
pub fn resolve_verify_cmd(explicit: Option<&str>, workspace: &Path) -> Option<String> {
    match explicit.map(str::trim).filter(|s| !s.is_empty()) {
        Some("none" | "off" | "false" | "0") => None,
        Some("auto") => detect_verify_command(workspace).map(|d| d.command),
        Some(cmd) => Some(cmd.to_string()),
        None => detect_verify_command(workspace)
            .filter(|d| d.confidence >= VerifyConfidence::High)
            .map(|d| d.command),
    }
}

/// Scan `workspace` for a verification command. Returns the highest-confidence match.
pub fn detect_verify_command(workspace: &Path) -> Option<VerifyDetect> {
    let root = workspace;

    // 1. Explicit project verification script/config (High confidence)
    if root.join("_lokai_verify.py").is_file() {
        return Some(VerifyDetect {
            command: python_cmd("_lokai_verify.py"),
            source: "_lokai_verify.py",
            confidence: VerifyConfidence::High,
        });
    }

    if let Some(cmd) = verify_file_command(root) {
        return Some(VerifyDetect {
            command: cmd,
            source: ".lokai/verify",
            confidence: VerifyConfidence::High,
        });
    }

    // 2. Standard ecosystem conventions (Advisory / Low confidence: opt-in via --verify auto)
    if let Some(cmd) = package_json_test_command(root) {
        return Some(VerifyDetect {
            command: cmd,
            source: "package.json",
            confidence: VerifyConfidence::Low,
        });
    }

    if root.join("go.mod").is_file() {
        return Some(VerifyDetect {
            command: "go test ./...".to_string(),
            source: "go.mod",
            confidence: VerifyConfidence::Low,
        });
    }

    if root.join("Cargo.toml").is_file() {
        return Some(VerifyDetect {
            command: "cargo test --quiet".to_string(),
            source: "Cargo.toml",
            confidence: VerifyConfidence::Low,
        });
    }

    if root.join("pytest.ini").is_file() || pyproject_has_pytest(root) {
        return Some(VerifyDetect {
            command: "python -m pytest -q".to_string(),
            source: "pytest",
            confidence: VerifyConfidence::Low,
        });
    }

    if python_test_tree(root) {
        return Some(VerifyDetect {
            command: "python -m pytest -q".to_string(),
            source: "tests/",
            confidence: VerifyConfidence::Low,
        });
    }

    if root.join("CMakeLists.txt").is_file() {
        return Some(VerifyDetect {
            command: "ctest".to_string(),
            source: "CMakeLists.txt",
            confidence: VerifyConfidence::Low,
        });
    }

    if let Ok(text) = read_workspace_text(&root.join("Makefile")) {
        if text.contains("test:") || text.contains("check:") {
            return Some(VerifyDetect {
                command: "make test".to_string(),
                source: "Makefile",
                confidence: VerifyConfidence::Low,
            });
        }
    }

    if root.join("pom.xml").is_file() {
        return Some(VerifyDetect {
            command: "mvn test".to_string(),
            source: "pom.xml",
            confidence: VerifyConfidence::Low,
        });
    }

    if root.join("build.gradle").is_file() || root.join("build.gradle.kts").is_file() {
        return Some(VerifyDetect {
            command: "gradle test".to_string(),
            source: "build.gradle",
            confidence: VerifyConfidence::Low,
        });
    }

    None
}

fn read_workspace_text(path: &Path) -> Result<String, crate::types::ToolError> {
    crate::workspace::read_to_string_nofollow(path)
}

fn verify_file_command(root: &Path) -> Option<String> {
    if let Ok(text) = read_workspace_text(&root.join(".lokai/verify.json")) {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(cmd) = val.get("command").and_then(|v| v.as_str()) {
                if !cmd.trim().is_empty() {
                    return Some(cmd.trim().to_string());
                }
            }
        }
    }
    if let Ok(text) = read_workspace_text(&root.join(".lokai/verify.toml")) {
        for line in text.lines() {
            let line = line.trim();
            if let Some(cmd) = line.strip_prefix("command") {
                let clean = cmd
                    .trim()
                    .trim_start_matches('=')
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'');
                if !clean.is_empty() {
                    return Some(clean.to_string());
                }
            }
        }
    }
    None
}

fn package_json_test_command(root: &Path) -> Option<String> {
    let pkg_path = root.join("package.json");
    let text = read_workspace_text(&pkg_path).ok()?;
    let val: serde_json::Value = serde_json::from_str(&text).ok()?;
    let test_script = val.get("scripts")?.get("test")?.as_str()?;
    if test_script.trim().is_empty() || test_script.contains("no test specified") {
        return None;
    }
    if root.join("bun.lockb").is_file() || root.join("bun.lock").is_file() {
        Some("bun test".to_string())
    } else if root.join("pnpm-lock.yaml").is_file() {
        Some("pnpm test".to_string())
    } else if root.join("yarn.lock").is_file() {
        Some("yarn test".to_string())
    } else {
        Some("npm test".to_string())
    }
}

fn python_cmd(script: &str) -> String {
    if cfg!(windows) {
        format!("python {script}")
    } else {
        format!("python3 {script}")
    }
}

fn pyproject_has_pytest(root: &Path) -> bool {
    let path = root.join("pyproject.toml");
    let Ok(text) = read_workspace_text(&path) else {
        return false;
    };
    text.contains("[tool.pytest") || text.contains("pytest")
}

fn python_test_tree(root: &Path) -> bool {
    for dir_name in ["tests", "test"] {
        let dir = root.join(dir_name);
        if dir.is_dir() && dir_has_python_tests(&dir) {
            return true;
        }
    }
    false
}

fn dir_has_python_tests(dir: &Path) -> bool {
    let Ok(read) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_file() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with("test_") && name.ends_with(".py") {
                return true;
            }
            if name.ends_with("_test.py") {
                return true;
            }
        }
    }
    false
}

/// Run `python -m py_compile` on a workspace `.py` file. Returns `Ok(())` or a
/// short syntax error message (IndentationError, SyntaxError, etc.).
pub fn check_python_syntax(workspace: &Path, rel_path: &str) -> Result<(), String> {
    if !rel_path.ends_with(".py") {
        return Ok(());
    }
    if rel_path.contains("..") || rel_path.contains(';') || rel_path.contains('|') {
        return Err("invalid python path for syntax check".into());
    }
    let abs = workspace.join(rel_path);
    if !abs.is_file() {
        return Ok(());
    }
    crate::process_executor::run_py_compile(workspace, rel_path)
}

/// Pull high-signal diagnostic lines from verify output for model feedback (VER-103).
pub fn summarize_verify_failure(output: &str) -> Option<String> {
    let mut key_lines: Vec<&str> = Vec::new();
    for line in output.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if (t.starts_with("error[E")
            || t.starts_with("error:")
            || t.starts_with("FAIL ")
            || t.starts_with("--- FAIL:")
            || t.starts_with("FAILED ")
            || t.contains("AssertionError")
            || t.starts_with("assert ")
            || t.contains("SyntaxError:")
            || t.contains("TypeError:")
            || t.contains("re.error:"))
            && !key_lines.contains(&t)
        {
            key_lines.push(t);
            if key_lines.len() >= 4 {
                break;
            }
        }
    }

    if !key_lines.is_empty() {
        return Some(key_lines.join("\n"));
    }

    // Fallback: search backwards for any line containing error/fail
    for line in output.lines().rev() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.contains("Error") || t.contains("error") || t.contains("FAIL") {
            return Some(t.to_string());
        }
    }

    output
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
}

/// Format a numbered snapshot of the changed file for injection after edit/write.
/// Prefers a window around the first changed line for large files.
pub fn format_post_edit_snapshot(change: &crate::FileChange) -> String {
    let Some(after) = change.after.as_deref() else {
        return String::new();
    };
    let lines: Vec<&str> = after.lines().collect();
    if lines.is_empty() {
        return format!("\n[post-edit snapshot of {}]\n(file is empty)", change.path);
    }

    let (start, end) = snapshot_window(&change.before, after, lines.len());
    let body: String = lines[start..end]
        .iter()
        .enumerate()
        .map(|(i, line)| format!("{:>6}| {line}", start + i + 1))
        .collect::<Vec<_>>()
        .join("\n");

    let range_note = if start == 0 && end == lines.len() {
        String::new()
    } else {
        format!(" (lines {}–{})", start + 1, end)
    };

    format!(
        "\n[post-edit snapshot of {}{}]\n{body}",
        change.path, range_note
    )
}

fn snapshot_window(before: &Option<String>, after: &str, line_count: usize) -> (usize, usize) {
    const MAX_LINES: usize = 80;
    const CONTEXT: usize = 12;

    if line_count <= MAX_LINES {
        return (0, line_count);
    }

    let focus = first_changed_line(before.as_deref(), after).unwrap_or(0);
    let start = focus.saturating_sub(CONTEXT);
    let end = (focus + CONTEXT + 1).min(line_count).max(start + 1);
    let end = end.min(line_count);
    let start = end.saturating_sub(MAX_LINES).max(start);
    (start, end)
}

fn first_changed_line(before: Option<&str>, after: &str) -> Option<usize> {
    let before_lines: Vec<&str> = before.unwrap_or("").lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();
    let max = before_lines.len().max(after_lines.len());
    for i in 0..max {
        let b = before_lines.get(i).copied().unwrap_or("");
        let a = after_lines.get(i).copied().unwrap_or("");
        if b != a {
            return Some(i);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChangeKind, FileChange};
    use std::fs;
    use std::path::PathBuf;

    fn tmp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lokai-verify-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sqlite_makefile_is_not_a_verify_command() {
        let dir = tmp_dir("sqlite-make");
        let mut bytes = b"SQLite format 3\0".to_vec();
        bytes.extend_from_slice(b"test:\nPRIVATECANARY\n");
        fs::write(dir.join("Makefile"), bytes).unwrap();
        let detected = detect_verify_command(&dir);
        let rendered = format!("{detected:?}");
        assert!(
            !rendered.contains("PRIVATECANARY"),
            "verify detection loaded the database: {rendered}"
        );
        assert!(detected.is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn detects_lokai_verify_script() {
        let dir = tmp_dir("lokai");
        fs::write(dir.join("_lokai_verify.py"), "print('ok')\n").unwrap();
        let d = detect_verify_command(&dir).unwrap();
        assert_eq!(d.source, "_lokai_verify.py");
        assert_eq!(d.confidence, VerifyConfidence::High);
        assert!(d.command.contains("_lokai_verify.py"));
        assert!(resolve_verify_cmd(None, &dir).is_some());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn detects_lokai_verify_toml_high_confidence() {
        let dir = tmp_dir("lokai-toml");
        fs::create_dir_all(dir.join(".lokai")).unwrap();
        fs::write(
            dir.join(".lokai/verify.toml"),
            "command = \"npm run test:fast\"\n",
        )
        .unwrap();
        let d = detect_verify_command(&dir).unwrap();
        assert_eq!(d.source, ".lokai/verify");
        assert_eq!(d.command, "npm run test:fast");
        assert_eq!(d.confidence, VerifyConfidence::High);
        assert_eq!(
            resolve_verify_cmd(None, &dir).as_deref(),
            Some("npm run test:fast")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn detects_cargo() {
        let dir = tmp_dir("cargo");
        fs::write(dir.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        let d = detect_verify_command(&dir).unwrap();
        assert_eq!(d.source, "Cargo.toml");
        assert_eq!(d.confidence, VerifyConfidence::Low);
        // By default without explicit flags, advisory low confidence is NOT run implicitly
        assert!(resolve_verify_cmd(None, &dir).is_none());
        assert_eq!(
            resolve_verify_cmd(Some("auto"), &dir).as_deref(),
            Some("cargo test --quiet")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn detects_package_json() {
        let dir = tmp_dir("pkg-json");
        fs::write(
            dir.join("package.json"),
            "{\"scripts\": {\"test\": \"vitest\"}}\n",
        )
        .unwrap();
        let d = detect_verify_command(&dir).unwrap();
        assert_eq!(d.source, "package.json");
        assert_eq!(d.command, "npm test");
        assert_eq!(d.confidence, VerifyConfidence::Low);

        fs::write(dir.join("bun.lockb"), "").unwrap();
        let d_bun = detect_verify_command(&dir).unwrap();
        assert_eq!(d_bun.command, "bun test");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn detects_go_mod() {
        let dir = tmp_dir("go-mod");
        fs::write(dir.join("go.mod"), "module example.com/m\n").unwrap();
        let d = detect_verify_command(&dir).unwrap();
        assert_eq!(d.source, "go.mod");
        assert_eq!(d.command, "go test ./...");
        assert_eq!(d.confidence, VerifyConfidence::Low);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn detects_pytest_tests_dir() {
        let dir = tmp_dir("pytest");
        fs::create_dir_all(dir.join("tests")).unwrap();
        fs::write(dir.join("tests/test_foo.py"), "def test_x(): pass\n").unwrap();
        let d = detect_verify_command(&dir).unwrap();
        assert_eq!(d.source, "tests/");
        assert_eq!(d.confidence, VerifyConfidence::Low);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_none_only_high_confidence() {
        let dir = tmp_dir("empty");
        assert!(resolve_verify_cmd(None, &dir).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_explicit_overrides() {
        let dir = tmp_dir("override");
        assert_eq!(
            resolve_verify_cmd(Some("make check"), &dir).as_deref(),
            Some("make check")
        );
        assert!(resolve_verify_cmd(Some("none"), &dir).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn post_edit_snapshot_shows_changed_region() {
        let change = FileChange {
            path: "a.py".into(),
            kind: ChangeKind::Edit,
            before: Some("line1\nline2\nline3\n".into()),
            after: Some("line1\nCHANGED\nline3\n".into()),
        };
        let snap = format_post_edit_snapshot(&change);
        assert!(snap.contains("CHANGED"));
        assert!(snap.contains("post-edit snapshot"));
    }

    #[test]
    fn py_compile_rejects_shell_metachar_in_path() {
        let dir = std::env::temp_dir().join(format!("py-bad-path-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(check_python_syntax(&dir, "bad;rm.py").is_err());
    }

    #[test]
    fn py_compile_catches_bad_python() {
        let dir = tmp_dir("syntax");
        fs::write(dir.join("bad.py"), "def f(\n").unwrap();
        let err = check_python_syntax(&dir, "bad.py").unwrap_err();
        assert!(err.contains("SyntaxError") || err.contains("bad.py"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn py_compile_ok_on_valid_python() {
        let dir = tmp_dir("okpy");
        fs::write(dir.join("ok.py"), "def f():\n    return 1\n").unwrap();
        assert!(check_python_syntax(&dir, "ok.py").is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn summarize_verify_failure_extracts_error_line() {
        let out = "stdout:\nGRADE: FAIL\nstderr:\nre.error: bad range\n";
        let line = summarize_verify_failure(out).unwrap();
        assert!(line.contains("re.error"));
    }
}

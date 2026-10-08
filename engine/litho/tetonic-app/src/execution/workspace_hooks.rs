//! Host filesystem/capability hooks; no work scheduling or lifecycle ownership.
use std::path::Path;
use std::sync::Arc;
use tetonic_context::workspace::ContextFsHooks;
use tetonic_core::{CaptureWorkspaceVersion, PostEditSnapshot, ResolveUnderRoot};
use tetonic_tools::{EnforcementLevel, Workspace};

pub(crate) fn composition_capability_hooks(
) -> (PostEditSnapshot, ResolveUnderRoot, CaptureWorkspaceVersion) {
    (
        Arc::new(tetonic_tools::format_post_edit_snapshot),
        Arc::new(|root, rel| {
            let abs = tetonic_transaction::fs_ops::resolve_under_root(root, rel).map_err(|_| ())?;
            std::fs::metadata(abs).map(|m| m.len()).map_err(|_| ())
        }),
        Arc::new(|root, paths| {
            tetonic_transaction::version::capture_workspace_version(root, paths)
                .map_err(|e| e.to_string())
        }),
    )
}

fn git_args_leave_workspace(args: &[&str]) -> bool {
    args.iter().any(|arg| {
        let arg = arg.replace('\\', "/");
        arg == ".." || arg.starts_with("../") || arg.contains("/../")
    })
}

fn reserved_markers(reserved: &[std::path::PathBuf], root: &Path) -> Vec<String> {
    let mut markers = Vec::new();
    for path in reserved {
        if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
            let name = name.to_ascii_lowercase();
            if !name.is_empty() {
                markers.push(name);
            }
        }
        if let Ok(rel) = path.strip_prefix(root) {
            let rel = rel
                .to_string_lossy()
                .replace('\\', "/")
                .to_ascii_lowercase();
            if !rel.is_empty() {
                markers.push(rel);
            }
        }
    }
    markers
}

fn line_mentions_reserved(line: &str, markers: &[String]) -> bool {
    let lower = line.replace('\\', "/").to_ascii_lowercase();
    markers.iter().any(|marker| lower.contains(marker))
}

fn git_line_names_sqlite(line: &str, root: &Path) -> bool {
    let mut candidates = Vec::new();
    if let Some(rest) = line.trim().strip_prefix("diff --git ") {
        candidates.extend(rest.split_whitespace().map(str::to_string));
    } else {
        let raw = line.trim_end();
        if raw.len() > 3 {
            candidates.push(raw[3..].trim().trim_matches('"').to_string());
        }
    }
    candidates.into_iter().any(|token| {
        let rel = token
            .strip_prefix("a/")
            .or_else(|| token.strip_prefix("b/"))
            .unwrap_or(token.as_str());
        if rel.is_empty() || rel.contains("..") || rel.contains('\0') {
            return false;
        }
        tetonic_context::workspace::path_is_sqlite_store_family(&root.join(rel))
    })
}

/// Drop git diff sections and status lines for the protected store or any live
/// SQLite database. A text diff of that file would otherwise enter the model prompt.
pub(crate) fn without_reserved_git_output(
    output: &str,
    reserved: &[std::path::PathBuf],
    root: &Path,
) -> String {
    let markers = reserved_markers(reserved, root);
    let hidden =
        |line: &str| line_mentions_reserved(line, &markers) || git_line_names_sqlite(line, root);
    if !output.contains("diff --git") {
        return output
            .lines()
            .filter(|line| !hidden(line))
            .collect::<Vec<_>>()
            .join("\n");
    }
    let mut kept = String::new();
    let mut section = String::new();
    let mut drop_section = false;
    let mut in_section = false;
    for line in output.lines() {
        if line.starts_with("diff --git") {
            if in_section && !drop_section {
                kept.push_str(&section);
            }
            in_section = true;
            drop_section = hidden(line);
            section = String::new();
            if !drop_section {
                section.push_str(line);
                section.push('\n');
            }
        } else if !in_section || !drop_section {
            if in_section {
                section.push_str(line);
                section.push('\n');
            } else if !hidden(line) {
                kept.push_str(line);
                kept.push('\n');
            }
        }
    }
    if in_section && !drop_section {
        kept.push_str(&section);
    }
    kept
}

/// Production jailed-read hooks for scoped context compilers: refuse the
/// control database and its sidecars, and filter them from git output.
pub fn composition_fs_hooks(reserved: Vec<std::path::PathBuf>) -> ContextFsHooks {
    let git_reserved = reserved.clone();
    ContextFsHooks {
        skip_symlink: Arc::new(tetonic_transaction::fs_ops::is_symlink_or_reparse),
        jailed_read: Arc::new(move |root, rel| {
            let ws = Workspace::new(root).map_err(|e| e.to_string())?;
            let path = ws.resolve(rel).map_err(|e| e.to_string())?;
            if tetonic_tools::path_is_reserved(&reserved, &path) {
                return Err("file is outside this execution grant".into());
            }
            tetonic_tools::read_to_string_nofollow(&path).map_err(|e| e.to_string())
        }),
        run_git: Arc::new(move |root, args| {
            if git_args_leave_workspace(args) {
                return Err("git command is outside this execution grant".into());
            }
            let pe = tetonic_tools::coding_executor(root, EnforcementLevel::Sandboxed);
            let r = pe.run_git(args.iter().map(|s| (*s).to_string()))?;
            Ok(without_reserved_git_output(&r.output, &git_reserved, root))
        }),
    }
}

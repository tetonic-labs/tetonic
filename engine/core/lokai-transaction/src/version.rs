//! Capture `WorkspaceVersion` for Git and non-Git workspaces.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use lokai_domain::{
    CommitHash, ContentDigest, RepositoryId, WorkspacePath, WorkspaceVersion,
    WorkspaceVersionScheme,
};

use crate::error::TransactionError;
use crate::fs_ops::{digest_bytes, digest_file, is_excluded_artifact, rel_path};

pub fn capture_workspace_version(
    root: &Path,
    relevant_paths: &[WorkspacePath],
) -> Result<WorkspaceVersion, TransactionError> {
    let root = std::fs::canonicalize(root)
        .map_err(|e| TransactionError::InvalidWorkspace(e.to_string()))?;
    let repo_id = repository_id(&root)?;
    if is_git_repo(&root) {
        capture_git(&root, repo_id, relevant_paths)
    } else {
        capture_manifest(&root, repo_id, relevant_paths)
    }
}

fn is_git_repo(root: &Path) -> bool {
    root.join(".git").exists() || git_rev_parse(root).is_ok()
}

fn git_rev_parse(root: &Path) -> Result<String, TransactionError> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|e| TransactionError::Git(e.to_string()))?;
    if !out.status.success() {
        return Err(TransactionError::Git(
            String::from_utf8_lossy(&out.stderr).into(),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn repository_id(root: &Path) -> Result<RepositoryId, TransactionError> {
    if is_git_repo(root) {
        let git_dir = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "--git-common-dir"])
            .output()
            .map_err(|e| TransactionError::Git(e.to_string()))?;
        if git_dir.status.success() {
            let dir = String::from_utf8_lossy(&git_dir.stdout).trim().to_string();
            let abs = if Path::new(&dir).is_absolute() {
                PathBuf::from(&dir)
            } else {
                root.join(&dir)
            };
            if let Ok(c) = std::fs::canonicalize(&abs) {
                return Ok(RepositoryId::new(format!(
                    "git:{}",
                    digest_bytes(c.to_string_lossy().as_bytes()).0
                )));
            }
        }
    }
    let canon = std::fs::canonicalize(root)
        .map_err(|e| TransactionError::InvalidWorkspace(e.to_string()))?;
    Ok(RepositoryId::new(format!(
        "manifest:{}",
        digest_bytes(canon.to_string_lossy().as_bytes()).0
    )))
}

fn git_status_porcelain(root: &Path) -> Result<String, TransactionError> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "status",
            "--porcelain",
            "-uall",
            "--",
            ".",
            ":(exclude).lokai",
        ])
        .output()
        .map_err(|e| TransactionError::Git(e.to_string()))?;
    if !out.status.success() {
        return Err(TransactionError::Git(
            String::from_utf8_lossy(&out.stderr).into(),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn git_ls_files_index(root: &Path) -> Result<String, TransactionError> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-s", "--", ".", ":(exclude).lokai"])
        .output()
        .map_err(|e| TransactionError::Git(e.to_string()))?;
    if !out.status.success() {
        return Err(TransactionError::Git(
            String::from_utf8_lossy(&out.stderr).into(),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn git_submodule_status(root: &Path) -> Result<String, TransactionError> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["submodule", "status", "--recursive"])
        .output()
        .map_err(|e| TransactionError::Git(e.to_string()))?;
    if !out.status.success() {
        return Ok(String::new());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn digest_to_generation(d: &ContentDigest) -> u64 {
    u64::from_str_radix(
        d.0.trim_start_matches("sha256:").get(..16).unwrap_or("0"),
        16,
    )
    .unwrap_or(0)
}

fn capture_git(
    root: &Path,
    repository_id: RepositoryId,
    relevant_paths: &[WorkspacePath],
) -> Result<WorkspaceVersion, TransactionError> {
    let head = git_rev_parse(root)?;
    let status = git_status_porcelain(root)?;
    let index = git_ls_files_index(root)?;
    let submodules = git_submodule_status(root)?;
    let dirty_state_digest = digest_bytes(status.as_bytes());
    let tracked_payload = format!("{head}\n{index}\n{submodules}");
    let tracked_state_digest = digest_bytes(tracked_payload.as_bytes());
    let index_generation = digest_to_generation(&tracked_state_digest);
    let mut relevant_path_digests = BTreeMap::new();
    for p in relevant_paths {
        let abs = root.join(p.0.replace('/', std::path::MAIN_SEPARATOR_STR));
        if abs.is_file() {
            if let Ok(d) = digest_file(&abs) {
                relevant_path_digests.insert(p.clone(), d);
            }
        }
    }
    Ok(WorkspaceVersion {
        repository_id,
        version_scheme: WorkspaceVersionScheme::Git,
        git_head: Some(CommitHash(head)),
        dirty_state_digest,
        tracked_state_digest,
        relevant_path_digests,
        index_generation: Some(index_generation),
    })
}

fn capture_manifest(
    root: &Path,
    repository_id: RepositoryId,
    relevant_paths: &[WorkspacePath],
) -> Result<WorkspaceVersion, TransactionError> {
    let mut manifest = BTreeMap::new();
    walk_manifest(root, root, &mut manifest)?;
    let manifest_json =
        serde_json::to_string(&manifest).map_err(|e| TransactionError::Other(e.to_string()))?;
    let dirty_state_digest = digest_bytes(manifest_json.as_bytes());
    let tracked_state_digest = dirty_state_digest.clone();
    // Content-derived, never wall clock: capability binding compares whole
    // `WorkspaceVersion` values, so a time-based generation denies any mutation whose
    // capability was issued in the previous second even on an untouched workspace.
    let index_generation = digest_to_generation(&tracked_state_digest);
    let mut relevant_path_digests = BTreeMap::new();
    for p in relevant_paths {
        if let Some(d) = manifest.get(&p.0) {
            relevant_path_digests.insert(p.clone(), ContentDigest::new(d.clone()));
        } else {
            let abs = root.join(p.0.replace('/', std::path::MAIN_SEPARATOR_STR));
            if abs.is_file() {
                if let Ok(d) = digest_file(&abs) {
                    relevant_path_digests.insert(p.clone(), d);
                }
            }
        }
    }
    Ok(WorkspaceVersion {
        repository_id,
        version_scheme: WorkspaceVersionScheme::Manifest,
        git_head: None,
        dirty_state_digest,
        tracked_state_digest,
        relevant_path_digests,
        index_generation: Some(index_generation),
    })
}

fn walk_manifest(
    root: &Path,
    dir: &Path,
    out: &mut BTreeMap<String, String>,
) -> Result<(), TransactionError> {
    if dir
        .file_name()
        .is_some_and(|n| n == ".git" || n == ".lokai")
    {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir).map_err(|e| TransactionError::Io(e.to_string()))? {
        let entry = entry.map_err(|e| TransactionError::Io(e.to_string()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|e| TransactionError::Io(e.to_string()))?;
        if file_type.is_dir()
            && !file_type.is_symlink()
            && !crate::fs_ops::is_symlink_or_reparse(&path)
        {
            walk_manifest(root, &path, out)?;
        } else if file_type.is_symlink() || crate::fs_ops::is_symlink_or_reparse(&path) {
            let rel = path
                .strip_prefix(root)
                .map_err(|_| TransactionError::OutsideWorkspace(path.display().to_string()))?;
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            if is_excluded_artifact(&rel_str) {
                continue;
            }
            let target_str = std::fs::read_link(&path)
                .map(|t| t.to_string_lossy().to_string())
                .unwrap_or_default();
            let d = digest_bytes(target_str.as_bytes());
            out.insert(rel_str, d.0);
        } else if file_type.is_file() {
            let rel = rel_path(root, &path)?;
            if is_excluded_artifact(&rel.0) {
                continue;
            }
            let d = digest_file(&path)?;
            out.insert(rel.0, d.0);
        }
    }
    Ok(())
}

pub fn path_state(
    root: &Path,
    rel: &WorkspacePath,
) -> Result<(bool, Option<ContentDigest>, Option<u32>), TransactionError> {
    let abs = resolve_safe(root, rel)?;
    let meta = std::fs::symlink_metadata(&abs);
    let Ok(meta) = meta else {
        return Ok((false, None, None));
    };
    let mode = crate::fs_ops::file_mode(&abs);
    if meta.is_symlink() || crate::fs_ops::is_symlink_or_reparse(&abs) {
        let target_str = std::fs::read_link(&abs)
            .map(|t| t.to_string_lossy().to_string())
            .unwrap_or_default();
        let d = digest_bytes(target_str.as_bytes());
        return Ok((true, Some(d), mode));
    }
    if meta.is_file() {
        let d = digest_file(&abs)?;
        return Ok((true, Some(d), mode));
    }
    Ok((true, None, mode))
}

pub fn resolve_safe(root: &Path, rel: &WorkspacePath) -> Result<PathBuf, TransactionError> {
    crate::fs_ops::resolve_under_root(root, &rel.0)
}

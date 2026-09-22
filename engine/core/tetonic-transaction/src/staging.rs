//! Staging directory and operation tracking.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use tetonic_domain::{ContentDigest, StagedOperation, StagedOperationKind, WorkspacePath};

use crate::error::TransactionError;
use crate::fs_ops::{
    digest_bytes, digest_file, is_excluded_artifact, rel_path, resolve_under_root,
};

#[derive(Debug, Default, Clone)]
pub struct ReadWriteSets {
    pub reads: BTreeSet<WorkspacePath>,
    pub writes: BTreeMap<WorkspacePath, StagedOperation>,
    /// Content digests captured when paths enter the read set.
    pub read_baselines: BTreeMap<WorkspacePath, ContentDigest>,
}

#[derive(Clone)]
pub struct StagingArea {
    pub dir: PathBuf,
    pub content_dir: PathBuf,
}

impl StagingArea {
    fn safe_component(s: &str) -> String {
        s.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    }

    /// Stage under `.lokai/staging/{txn}` so crash recovery can find and discard orphans (R6-3).
    pub fn create(root: &Path, _repo_id: &str, txn_id: &str) -> Result<Self, TransactionError> {
        let base = root
            .join(".lokai")
            .join("staging")
            .join(Self::safe_component(txn_id));
        std::fs::create_dir_all(&base).map_err(|e| TransactionError::Io(e.to_string()))?;
        let content_dir = base.join("content");
        std::fs::create_dir_all(&content_dir).map_err(|e| TransactionError::Io(e.to_string()))?;
        Ok(Self {
            dir: base,
            content_dir,
        })
    }

    pub fn store_content(
        &self,
        rel: &WorkspacePath,
        data: &[u8],
    ) -> Result<PathBuf, TransactionError> {
        let safe = rel.0.replace('/', "_");
        let path = self.content_dir.join(safe);
        std::fs::write(&path, data).map_err(|e| TransactionError::Io(e.to_string()))?;
        Ok(path)
    }

    pub fn stage_create(
        &self,
        root: &Path,
        sets: &mut ReadWriteSets,
        rel: &WorkspacePath,
        content: &[u8],
        base_digest: Option<ContentDigest>,
    ) -> Result<(), TransactionError> {
        if is_excluded_artifact(&rel.0) {
            return Err(TransactionError::Other(format!(
                "refusing to stage build artifact: {}",
                rel.0
            )));
        }
        let content_path = self.store_content(rel, content)?;
        let new_digest = digest_bytes(content);
        sets.writes.insert(
            rel.clone(),
            StagedOperation {
                kind: StagedOperationKind::Create,
                path: rel.clone(),
                destination: None,
                base_digest,
                new_digest: Some(new_digest),
                new_content_path: Some(content_path.display().to_string()),
                base_mode: None,
                new_mode: None,
            },
        );
        sets.reads.insert(rel.clone());
        let _ = root;
        Ok(())
    }

    pub fn stage_replace(
        &self,
        root: &Path,
        sets: &mut ReadWriteSets,
        rel: &WorkspacePath,
        content: &[u8],
        base_digest: Option<ContentDigest>,
    ) -> Result<(), TransactionError> {
        if is_excluded_artifact(&rel.0) {
            return Err(TransactionError::Other(format!(
                "refusing to stage build artifact: {}",
                rel.0
            )));
        }
        let abs = resolve_under_root(root, &rel.0)?;
        let base = if base_digest.is_none() {
            let meta = std::fs::symlink_metadata(&abs);
            if let Ok(meta) = meta {
                if meta.is_symlink() || crate::fs_ops::is_symlink_or_reparse(&abs) {
                    let target_str = std::fs::read_link(&abs)
                        .map(|t| t.to_string_lossy().to_string())
                        .unwrap_or_default();
                    Some(digest_bytes(target_str.as_bytes()))
                } else if meta.is_file() {
                    Some(digest_file(&abs)?)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            base_digest
        };
        let content_path = self.store_content(rel, content)?;
        let new_digest = digest_bytes(content);
        let kind = if std::fs::symlink_metadata(&abs).is_ok() {
            StagedOperationKind::Replace
        } else {
            StagedOperationKind::Create
        };
        sets.writes.insert(
            rel.clone(),
            StagedOperation {
                kind,
                path: rel.clone(),
                destination: None,
                base_digest: base,
                new_digest: Some(new_digest),
                new_content_path: Some(content_path.display().to_string()),
                base_mode: None,
                new_mode: None,
            },
        );
        sets.reads.insert(rel.clone());
        Ok(())
    }

    pub fn stage_edit(
        &self,
        root: &Path,
        sets: &mut ReadWriteSets,
        rel: &WorkspacePath,
        old: &str,
        new: &str,
    ) -> Result<(String, String), TransactionError> {
        let abs = resolve_under_root(root, &rel.0)?;
        if crate::fs_ops::is_symlink_or_reparse(&abs) || !abs.is_file() {
            return Err(TransactionError::Other(format!(
                "file not found: {}",
                rel.0
            )));
        }
        let text = crate::fs_ops::read_to_string_nofollow(&abs)?;
        let outcome = crate::fuzzy_patch::apply_fuzzy_edit(&text, old, new)
            .map_err(TransactionError::Other)?;
        let base_digest = digest_file(&abs)?;
        let updated = outcome.updated_text;
        self.stage_replace(root, sets, rel, updated.as_bytes(), Some(base_digest))?;
        Ok((text, updated))
    }

    pub fn stage_delete(
        &self,
        root: &Path,
        sets: &mut ReadWriteSets,
        rel: &WorkspacePath,
    ) -> Result<(), TransactionError> {
        let abs = resolve_under_root(root, &rel.0)?;
        let base = if abs.exists() {
            Some(digest_file(&abs).unwrap_or_else(|_| digest_bytes(b"")))
        } else {
            None
        };
        sets.writes.insert(
            rel.clone(),
            StagedOperation {
                kind: StagedOperationKind::Delete,
                path: rel.clone(),
                destination: None,
                base_digest: base,
                new_digest: None,
                new_content_path: None,
                base_mode: None,
                new_mode: None,
            },
        );
        sets.reads.insert(rel.clone());
        Ok(())
    }

    pub fn stage_rename(
        &self,
        root: &Path,
        sets: &mut ReadWriteSets,
        from: &WorkspacePath,
        to: &WorkspacePath,
    ) -> Result<(), TransactionError> {
        let abs = resolve_under_root(root, &from.0)?;
        let base = digest_file(&abs)?;
        sets.writes.insert(
            from.clone(),
            StagedOperation {
                kind: StagedOperationKind::Rename,
                path: from.clone(),
                destination: Some(to.clone()),
                base_digest: Some(base),
                new_digest: None,
                new_content_path: None,
                base_mode: None,
                new_mode: None,
            },
        );
        sets.reads.insert(from.clone());
        sets.reads.insert(to.clone());
        Ok(())
    }

    pub fn stage_mode_change(
        &self,
        root: &Path,
        sets: &mut ReadWriteSets,
        rel: &WorkspacePath,
        new_mode: u32,
    ) -> Result<(), TransactionError> {
        let abs = resolve_under_root(root, &rel.0)?;
        if !abs.is_file() {
            return Err(TransactionError::Other(format!(
                "file not found: {}",
                rel.0
            )));
        }
        let base = digest_file(&abs)?;
        let base_mode = crate::fs_ops::file_mode(&abs);
        sets.writes.insert(
            rel.clone(),
            StagedOperation {
                kind: StagedOperationKind::ModeChange,
                path: rel.clone(),
                destination: None,
                base_digest: Some(base),
                new_digest: None,
                new_content_path: None,
                base_mode,
                new_mode: Some(new_mode),
            },
        );
        sets.reads.insert(rel.clone());
        Ok(())
    }

    pub fn record_read(&self, root: &Path, sets: &mut ReadWriteSets, rel: &WorkspacePath) {
        if !sets.reads.contains(rel) {
            if let Ok(abs) = resolve_under_root(root, &rel.0) {
                if abs.is_file() {
                    if let Ok(d) = digest_file(&abs) {
                        sets.read_baselines.insert(rel.clone(), d);
                    }
                }
            }
        }
        sets.reads.insert(rel.clone());
    }

    pub fn operations_in_order(sets: &ReadWriteSets) -> Vec<StagedOperation> {
        sets.writes.values().cloned().collect()
    }

    pub fn materialize_overlay(
        root: &Path,
        staging: &StagingArea,
        sets: &ReadWriteSets,
    ) -> Result<PathBuf, TransactionError> {
        let overlay = staging.dir.join("verify_overlay");
        if overlay.exists() {
            std::fs::remove_dir_all(&overlay).map_err(|e| TransactionError::Io(e.to_string()))?;
        }
        copy_tree(root, &overlay, root)?;
        for op in sets.writes.values() {
            let dest = overlay.join(op.path.0.replace('/', std::path::MAIN_SEPARATOR_STR));
            match op.kind {
                StagedOperationKind::Delete | StagedOperationKind::Rename => {
                    if dest.exists() {
                        if dest.is_dir() {
                            std::fs::remove_dir_all(&dest)
                                .map_err(|e| TransactionError::Io(e.to_string()))?;
                        } else {
                            std::fs::remove_file(&dest)
                                .map_err(|e| TransactionError::Io(e.to_string()))?;
                        }
                    }
                }
                _ => {
                    if let Some(cp) = &op.new_content_path {
                        if let Some(parent) = dest.parent() {
                            std::fs::create_dir_all(parent)
                                .map_err(|e| TransactionError::Io(e.to_string()))?;
                        }
                        std::fs::copy(cp, &dest)
                            .map_err(|e| TransactionError::Io(e.to_string()))?;
                    }
                }
            }
            if op.kind == StagedOperationKind::Rename {
                if let Some(to) = &op.destination {
                    let to_dest = overlay.join(to.0.replace('/', std::path::MAIN_SEPARATOR_STR));
                    if let Some(parent) = to_dest.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| TransactionError::Io(e.to_string()))?;
                    }
                    if dest.exists() {
                        std::fs::rename(&dest, &to_dest)
                            .map_err(|e| TransactionError::Io(e.to_string()))?;
                    }
                }
            }
        }
        Ok(overlay)
    }
}

fn copy_tree(src_root: &Path, dst_root: &Path, from: &Path) -> Result<(), TransactionError> {
    if from.is_file() {
        if let Some(parent) = dst_root.parent() {
            std::fs::create_dir_all(parent).map_err(|e| TransactionError::Io(e.to_string()))?;
        }
        std::fs::copy(from, dst_root).map_err(|e| TransactionError::Io(e.to_string()))?;
        return Ok(());
    }
    std::fs::create_dir_all(dst_root).map_err(|e| TransactionError::Io(e.to_string()))?;
    for entry in std::fs::read_dir(from).map_err(|e| TransactionError::Io(e.to_string()))? {
        let entry = entry.map_err(|e| TransactionError::Io(e.to_string()))?;
        let name = entry.file_name();
        if name == ".git" || name == ".lokai" {
            continue;
        }
        let rel = rel_path(src_root, &entry.path())?;
        if is_excluded_artifact(&rel.0) {
            continue;
        }
        let target = dst_root.join(name);
        if entry.path().is_dir() {
            copy_tree(src_root, &target, &entry.path())?;
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| TransactionError::Io(e.to_string()))?;
            }
            std::fs::copy(entry.path(), &target)
                .map_err(|e| TransactionError::Io(e.to_string()))?;
        }
    }
    Ok(())
}

pub fn patch_digest(ops: &[StagedOperation]) -> ContentDigest {
    let json = serde_json::to_string(ops).unwrap_or_default();
    digest_bytes(json.as_bytes())
}

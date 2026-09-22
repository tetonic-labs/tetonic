//! Apply staged operations with journal-backed recovery.

use std::path::Path;

use lokai_domain::{
    ActualState, ConflictKind, ContentDigest, ExpectedState, StagedOperationKind, TransactionState,
    WorkspaceConflict, WorkspacePath,
};

use crate::error::TransactionError;
use crate::fs_ops::{digest_file, resolve_under_root};
use crate::journal::{CommitJournal, JournalOperation, RenameDestinationBackup};
use crate::staging::ReadWriteSets;
use crate::version::{path_state, resolve_safe};

pub fn build_journal(
    journal: &mut CommitJournal,
    root: &Path,
    backup_dir: &Path,
    sets: &ReadWriteSets,
) -> Result<(), TransactionError> {
    std::fs::create_dir_all(backup_dir).map_err(|e| TransactionError::Io(e.to_string()))?;
    journal.operations.clear();
    for (i, op) in crate::staging::StagingArea::operations_in_order(sets)
        .into_iter()
        .enumerate()
    {
        let backup_path = if matches!(
            op.kind,
            StagedOperationKind::Delete
                | StagedOperationKind::Rename
                | StagedOperationKind::Replace
                | StagedOperationKind::Edit
                | StagedOperationKind::ModeChange
        ) {
            let abs = resolve_under_root(root, &op.path.0)?;
            if abs.exists() && abs.is_file() {
                let bp = backup_dir.join(format!("{:03}_{}", i, op.path.0.replace('/', "_")));
                if let Some(parent) = bp.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| TransactionError::Io(e.to_string()))?;
                }
                std::fs::copy(&abs, &bp).map_err(|e| TransactionError::Io(e.to_string()))?;
                Some(bp.display().to_string())
            } else {
                None
            }
        } else {
            None
        };
        let rename_destination = if matches!(op.kind, StagedOperationKind::Rename) {
            let to = op
                .destination
                .as_ref()
                .ok_or_else(|| TransactionError::Other("rename missing destination".into()))?;
            let dest = resolve_safe(root, to)?;
            if dest.exists() {
                if !dest.is_file() || dest.is_symlink() {
                    return Err(TransactionError::Other(
                        "rename destination must be a regular file".into(),
                    ));
                }
                let bp = backup_dir.join(format!("{i:03}_destination"));
                std::fs::copy(&dest, &bp).map_err(|e| TransactionError::Io(e.to_string()))?;
                Some(RenameDestinationBackup::File(bp.display().to_string()))
            } else {
                Some(RenameDestinationBackup::Absent)
            }
        } else {
            None
        };
        // Copy staged content into the backup tree so commit journals survive staging cleanup (R6-3).
        let new_content_path = if let Some(ref cp) = op.new_content_path {
            if Path::new(cp).exists() {
                let dest = backup_dir.join(format!("{:03}_new_{}", i, op.path.0.replace('/', "_")));
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| TransactionError::Io(e.to_string()))?;
                }
                std::fs::copy(cp, &dest).map_err(|e| TransactionError::Io(e.to_string()))?;
                dest.display().to_string()
            } else {
                cp.clone()
            }
        } else {
            String::new()
        };
        journal.operations.push(JournalOperation {
            index: i as u32,
            operation: op.clone(),
            backup_path,
            rename_destination,
            expected_digest: op.base_digest.clone(),
            new_content_path,
            started: Some(false),
            completed: false,
            rolled_back: false,
        });
    }
    for entry in std::fs::read_dir(backup_dir).map_err(|e| TransactionError::Io(e.to_string()))? {
        let path = entry
            .map_err(|e| TransactionError::Io(e.to_string()))?
            .path();
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .and_then(|f| f.sync_all())
            .map_err(|e| TransactionError::Io(e.to_string()))?;
    }
    crate::publication::sync_directory_chain(backup_dir, root)
        .map_err(|e| TransactionError::Io(e.to_string()))?;
    journal.state = TransactionState::Committing;
    Ok(())
}

pub fn apply_journal_operation(
    root: &Path,
    journal: &mut CommitJournal,
    index: u32,
    lokai_dir: &Path,
) -> Result<(), TransactionError> {
    let entry = journal
        .operations
        .iter_mut()
        .find(|op| op.index == index)
        .ok_or_else(|| TransactionError::Other("missing journal operation".into()))?;
    if entry.completed {
        return Ok(());
    }
    revalidate_precondition(root, entry)?;
    entry.started = Some(true);
    journal.persist(lokai_dir)?;
    apply_one(root, journal, index)?;
    journal.mark_completed(index)?;
    journal.persist(lokai_dir)?;
    Ok(())
}

pub fn apply_journal(
    root: &Path,
    journal: &mut CommitJournal,
    lokai_dir: &Path,
) -> Result<(), TransactionError> {
    for op in journal
        .operations
        .iter()
        .map(|o| o.index)
        .collect::<Vec<_>>()
    {
        apply_journal_operation(root, journal, op, lokai_dir)?;
    }
    journal.state = TransactionState::Committed;
    journal.persist(lokai_dir)?;
    Ok(())
}

fn apply_one(root: &Path, journal: &mut CommitJournal, index: u32) -> Result<(), TransactionError> {
    let entry = journal
        .operations
        .iter()
        .find(|o| o.index == index)
        .cloned()
        .ok_or_else(|| TransactionError::Other("missing op".into()))?;
    revalidate_precondition(root, &entry)?;
    let abs = resolve_safe(root, &entry.operation.path)?;
    match entry.operation.kind {
        StagedOperationKind::Create | StagedOperationKind::Replace | StagedOperationKind::Edit => {
            crate::publication::replace(root, &abs, Path::new(&entry.new_content_path))
                .map_err(|e| TransactionError::Io(e.to_string()))?;
        }
        StagedOperationKind::Delete => {
            crate::publication::remove(root, &abs)
                .map_err(|e| TransactionError::Io(e.to_string()))?;
        }
        StagedOperationKind::Rename => {
            let to = entry
                .operation
                .destination
                .as_ref()
                .ok_or_else(|| TransactionError::Other("rename missing destination".into()))?;
            let dest = resolve_safe(root, to)?;
            crate::publication::rename(root, &abs, &dest)
                .map_err(|e| TransactionError::Io(e.to_string()))?;
        }
        StagedOperationKind::ModeChange => {
            let mode = entry
                .operation
                .new_mode
                .ok_or_else(|| TransactionError::Other("mode change missing target mode".into()))?;
            crate::publication::change_mode(&abs, mode)
                .map_err(|e| TransactionError::Io(e.to_string()))?;
        }
    }
    Ok(())
}

fn revalidate_precondition(root: &Path, entry: &JournalOperation) -> Result<(), TransactionError> {
    let (exists, digest, mode) = path_state(root, &entry.operation.path)?;
    if let Some(expected) = &entry.expected_digest {
        if !exists {
            return Err(TransactionError::Conflict(vec![WorkspaceConflict {
                path: entry.operation.path.clone(),
                expected: ExpectedState {
                    digest: Some(expected.clone()),
                    exists: true,
                    mode: None,
                },
                actual: ActualState {
                    digest: None,
                    exists: false,
                    mode: None,
                },
                conflict_kind: ConflictKind::FileRemoved,
            }]));
        }
        if digest.as_ref() != Some(expected) {
            return Err(TransactionError::Conflict(vec![WorkspaceConflict {
                path: entry.operation.path.clone(),
                expected: ExpectedState {
                    digest: Some(expected.clone()),
                    exists: true,
                    mode,
                },
                actual: ActualState {
                    digest,
                    exists: true,
                    mode,
                },
                conflict_kind: ConflictKind::ContentChanged,
            }]));
        }
    } else if matches!(entry.operation.kind, StagedOperationKind::Create) && exists {
        return Err(TransactionError::Conflict(vec![WorkspaceConflict {
            path: entry.operation.path.clone(),
            expected: ExpectedState {
                digest: None,
                exists: false,
                mode: None,
            },
            actual: ActualState {
                digest,
                exists: true,
                mode,
            },
            conflict_kind: ConflictKind::NewFileDestinationOccupied,
        }]));
    }
    let abs = resolve_safe(root, &entry.operation.path)?;
    if abs.is_symlink() {
        let canon = std::fs::canonicalize(&abs).map_err(|e| TransactionError::Io(e.to_string()))?;
        if !canon.starts_with(root) {
            return Err(TransactionError::Conflict(vec![WorkspaceConflict {
                path: entry.operation.path.clone(),
                expected: ExpectedState {
                    digest: entry.expected_digest.clone(),
                    exists: true,
                    mode,
                },
                actual: ActualState {
                    digest: digest_file(&abs).ok(),
                    exists: true,
                    mode,
                },
                conflict_kind: ConflictKind::SymlinkRetargeted,
            }]));
        }
    }
    Ok(())
}

/// Resolve a durable intent against the actual filesystem before rollback.
pub(crate) fn reconcile_progress(
    root: &Path,
    journal: &mut CommitJournal,
) -> Result<(), TransactionError> {
    for entry in &mut journal.operations {
        if entry.completed || entry.rolled_back || entry.started == Some(false) {
            continue;
        }
        if entry.started.is_none() {
            return Err(TransactionError::RecoveryRequired(
                "legacy journal has unknown mutation progress".into(),
            ));
        }
        let (exists, digest, mode) = path_state(root, &entry.operation.path)?;
        let applied = match entry.operation.kind {
            StagedOperationKind::Create
            | StagedOperationKind::Replace
            | StagedOperationKind::Edit => {
                exists
                    && entry.operation.new_digest.is_some()
                    && digest == entry.operation.new_digest
            }
            StagedOperationKind::Delete => !exists,
            StagedOperationKind::Rename => {
                if let Some(dest) = &entry.operation.destination {
                    let (present, dest_digest, _) = path_state(root, dest)?;
                    !exists
                        && present
                        && entry.expected_digest.is_some()
                        && dest_digest == entry.expected_digest
                } else {
                    false
                }
            }
            StagedOperationKind::ModeChange => exists && mode == entry.operation.new_mode,
        };
        if applied {
            entry.completed = true;
        } else if revalidate_precondition(root, entry).is_err() {
            return Err(TransactionError::RecoveryRequired(
                "filesystem differs from both journal states".into(),
            ));
        }
    }
    Ok(())
}

pub fn rollback_journal(
    root: &Path,
    journal: &mut CommitJournal,
    lokai_dir: &Path,
) -> Result<(), TransactionError> {
    reconcile_progress(root, journal)?;
    let mut indices: Vec<u32> = journal
        .operations
        .iter()
        .filter(|o| o.completed && !o.rolled_back)
        .map(|o| o.index)
        .collect();
    indices.sort_by(|a, b| b.cmp(a));
    for idx in indices {
        let op = journal
            .operations
            .iter()
            .find(|o| o.index == idx)
            .cloned()
            .ok_or_else(|| TransactionError::Other("journal index missing".into()))?;
        rollback_one(root, &op)?;
        journal.mark_rolled_back(idx)?;
        journal.persist(lokai_dir)?;
    }
    Ok(())
}

fn rollback_one(root: &Path, entry: &JournalOperation) -> Result<(), TransactionError> {
    let abs = resolve_safe(root, &entry.operation.path)?;
    if matches!(entry.operation.kind, StagedOperationKind::Rename) {
        let destination = entry
            .operation
            .destination
            .as_ref()
            .ok_or_else(|| TransactionError::Other("rename missing destination".into()))?;
        let dest = resolve_safe(root, destination)?;
        let state = entry.rename_destination.as_ref().ok_or_else(|| {
            TransactionError::Other(
                "legacy rename journal lacks destination backup; manual recovery required".into(),
            )
        })?;
        let source_backup = entry
            .backup_path
            .as_ref()
            .filter(|p| Path::new(p).is_file())
            .ok_or_else(|| {
                TransactionError::Other(
                    "rename source backup missing; manual recovery required".into(),
                )
            })?;
        if let RenameDestinationBackup::File(backup) = state {
            if !Path::new(backup).is_file() {
                return Err(TransactionError::Other(
                    "rename destination backup missing; manual recovery required".into(),
                ));
            }
        }
        crate::publication::restore(root, &abs, Path::new(source_backup))
            .map_err(|e| TransactionError::Io(e.to_string()))?;
        match state {
            RenameDestinationBackup::Absent => crate::publication::remove(root, &dest)
                .map_err(|e| TransactionError::Io(e.to_string()))?,
            RenameDestinationBackup::File(backup) => {
                crate::publication::restore(root, &dest, Path::new(backup))
                    .map_err(|e| TransactionError::Io(e.to_string()))?;
            }
        }
        return Ok(());
    }
    if let Some(backup) = &entry.backup_path {
        crate::publication::restore(root, &abs, Path::new(backup))
            .map_err(|e| TransactionError::Io(e.to_string()))?;
    } else if matches!(entry.operation.kind, StagedOperationKind::Create) {
        crate::publication::remove(root, &abs).map_err(|e| TransactionError::Io(e.to_string()))?;
    } else {
        return Err(TransactionError::RecoveryRequired(
            "original file backup missing; manual recovery required".into(),
        ));
    }
    Ok(())
}

pub fn detect_conflicts(
    root: &Path,
    base: &lokai_domain::WorkspaceVersion,
    sets: &ReadWriteSets,
) -> Result<(), TransactionError> {
    let relevant: Vec<_> = sets
        .writes
        .keys()
        .chain(sets.reads.iter())
        .cloned()
        .collect();
    let current = crate::version::capture_workspace_version(root, &relevant)?;
    if current.repository_id != base.repository_id {
        return Err(TransactionError::Conflict(vec![WorkspaceConflict {
            path: WorkspacePath::new("."),
            expected: ExpectedState {
                digest: None,
                exists: true,
                mode: None,
            },
            actual: ActualState {
                digest: None,
                exists: true,
                mode: None,
            },
            conflict_kind: ConflictKind::WorkspaceIdentityChanged,
        }]));
    }
    if base.version_scheme == lokai_domain::WorkspaceVersionScheme::Git
        && current.git_head != base.git_head
    {
        return Err(TransactionError::Conflict(vec![WorkspaceConflict {
            path: WorkspacePath::new(".git/HEAD"),
            expected: ExpectedState {
                digest: base
                    .git_head
                    .as_ref()
                    .map(|h| ContentDigest::new(h.0.clone())),
                exists: true,
                mode: None,
            },
            actual: ActualState {
                digest: current
                    .git_head
                    .as_ref()
                    .map(|h| ContentDigest::new(h.0.clone())),
                exists: true,
                mode: None,
            },
            conflict_kind: ConflictKind::HeadChanged,
        }]));
    }
    let mut conflicts = Vec::new();
    for op in sets.writes.values() {
        let (exists, digest, mode) = path_state(root, &op.path)?;
        if let Some(base_d) = op
            .base_digest
            .as_ref()
            .or_else(|| base.relevant_path_digests.get(&op.path))
        {
            if exists {
                if let Some(d) = &digest {
                    if d != base_d {
                        conflicts.push(WorkspaceConflict {
                            path: op.path.clone(),
                            expected: ExpectedState {
                                digest: Some(base_d.clone()),
                                exists: true,
                                mode: None,
                            },
                            actual: ActualState {
                                digest: Some(d.clone()),
                                exists: true,
                                mode,
                            },
                            conflict_kind: ConflictKind::ContentChanged,
                        });
                    }
                }
            }
        }
        if matches!(op.kind, StagedOperationKind::Create) && exists {
            conflicts.push(WorkspaceConflict {
                path: op.path.clone(),
                expected: ExpectedState {
                    digest: None,
                    exists: false,
                    mode: None,
                },
                actual: ActualState {
                    digest: digest.clone(),
                    exists: true,
                    mode,
                },
                conflict_kind: ConflictKind::NewFileDestinationOccupied,
            });
        }
        if let Some(expected) = &op.base_digest {
            if exists {
                if digest.as_ref() != Some(expected) {
                    conflicts.push(WorkspaceConflict {
                        path: op.path.clone(),
                        expected: ExpectedState {
                            digest: Some(expected.clone()),
                            exists: true,
                            mode,
                        },
                        actual: ActualState {
                            digest: digest.clone(),
                            exists: true,
                            mode,
                        },
                        conflict_kind: ConflictKind::ContentChanged,
                    });
                }
            } else {
                conflicts.push(WorkspaceConflict {
                    path: op.path.clone(),
                    expected: ExpectedState {
                        digest: Some(expected.clone()),
                        exists: true,
                        mode: None,
                    },
                    actual: ActualState {
                        digest: None,
                        exists: false,
                        mode: None,
                    },
                    conflict_kind: ConflictKind::DeleteTargetChanged,
                });
            }
        }
        if let Some(dest) = &op.destination {
            if matches!(op.kind, StagedOperationKind::Rename) {
                let (dest_exists, dest_digest, dest_mode) = path_state(root, dest)?;
                if dest_exists {
                    conflicts.push(WorkspaceConflict {
                        path: dest.clone(),
                        expected: ExpectedState {
                            digest: None,
                            exists: false,
                            mode: None,
                        },
                        actual: ActualState {
                            digest: dest_digest,
                            exists: true,
                            mode: dest_mode,
                        },
                        conflict_kind: ConflictKind::RenameDestinationOccupied,
                    });
                }
            }
        }
    }
    for read_path in &sets.reads {
        if sets.writes.contains_key(read_path) {
            continue;
        }
        let Some(base_d) = sets.read_baselines.get(read_path) else {
            continue;
        };
        let (exists, digest, mode) = path_state(root, read_path)?;
        if !exists {
            conflicts.push(WorkspaceConflict {
                path: read_path.clone(),
                expected: ExpectedState {
                    digest: Some(base_d.clone()),
                    exists: true,
                    mode: None,
                },
                actual: ActualState {
                    digest: None,
                    exists: false,
                    mode: None,
                },
                conflict_kind: ConflictKind::ContentChanged,
            });
        } else if digest.as_ref() != Some(base_d) {
            conflicts.push(WorkspaceConflict {
                path: read_path.clone(),
                expected: ExpectedState {
                    digest: Some(base_d.clone()),
                    exists: true,
                    mode: None,
                },
                actual: ActualState {
                    digest: digest.clone(),
                    exists: true,
                    mode,
                },
                conflict_kind: ConflictKind::ContentChanged,
            });
        }
    }
    if conflicts.is_empty() {
        Ok(())
    } else {
        Err(TransactionError::Conflict(conflicts))
    }
}

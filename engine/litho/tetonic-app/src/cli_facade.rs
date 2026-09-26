//! Facade methods on Application for CLI offline, index, memory, and capacity operations.
//!
//! Keeps portals (tetonic-cli) completely decoupled from underlying storage,
//! capabilities, and compute crates.

use std::path::{Path, PathBuf};

use crate::errors::AppError;
use crate::Application;

// ---------------------------------------------------------------------------
// Time-travel / Checkpoints types and methods
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct CheckpointInfo {
    pub id: String,
    pub mark: i64,
    pub label: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct CheckpointsReport {
    pub current_head: i64,
    pub redo_target: Option<i64>,
    pub checkpoints: Vec<CheckpointInfo>,
}

#[derive(Debug, Clone)]
pub struct RestoreFileChange {
    pub verb: &'static str,
    pub path: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RestoreSummary {
    pub from_mark: i64,
    pub to_mark: i64,
    pub applied: usize,
    pub skipped: usize,
    pub dry_run: bool,
    pub reason: String,
    pub changes: Vec<RestoreFileChange>,
}

// ---------------------------------------------------------------------------
// Project Memory types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ProjectStatusInfo {
    pub id: String,
    pub name: String,
    pub root: String,
    pub digest_chars: usize,
    pub note_count: usize,
    pub last_active_at: String,
}

// ---------------------------------------------------------------------------
// History types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SessionSummaryInfo {
    pub id: String,
    pub status: String,
    pub messages: i64,
    pub tool_calls: i64,
    pub file_changes: i64,
    pub started_at: String,
    pub workspace_root: String,
}

// (Code Index types moved to cli_index.rs)

// ---------------------------------------------------------------------------
// Application implementation
// ---------------------------------------------------------------------------

impl Application {
    // --- Store & Path Access ---

    pub fn store(&self) -> Option<&tetonic_memory::SharedStore> {
        self.turn.store.as_ref()
    }

    // --- Time Travel / Checkpoints ---

    pub fn create_checkpoint(&self, ws_root: &str, label: &str) -> Result<(String, i64), AppError> {
        let store = self
            .store()
            .ok_or_else(|| AppError::PersistenceFailed("no store available".into()))?;
        let ws = ws_root.to_string();
        let lbl = label.to_string();
        store
            .write_sync(move |db| {
                db.create_checkpoint(&ws, &lbl, "manual", None)
                    .map_err(AppError::hide_store_failure)
            })
            .map_err(AppError::hide_store_failure)?
    }

    pub fn list_checkpoints(&self, ws_root: &str) -> Result<CheckpointsReport, AppError> {
        let store = self
            .store()
            .ok_or_else(|| AppError::PersistenceFailed("no store available".into()))?;
        let ws = ws_root.to_string();
        store
            .read_sync(move |db| {
                let current_head = db
                    .current_head(&ws)
                    .map_err(AppError::hide_store_failure)?;
                let redo_target = db
                    .head_state(&ws)
                    .map_err(AppError::hide_store_failure)?
                    .and_then(|(_, r)| r);
                let rows = db
                    .list_checkpoints(&ws)
                    .map_err(AppError::hide_store_failure)?;
                let checkpoints = rows
                    .into_iter()
                    .map(|c| CheckpointInfo {
                        id: c.id,
                        mark: c.mark,
                        label: c.label,
                        created_at: c.created_at,
                    })
                    .collect();
                Ok(CheckpointsReport {
                    current_head,
                    redo_target,
                    checkpoints,
                })
            })
            .map_err(AppError::hide_store_failure)?
    }

    pub fn restore_checkpoint(
        &self,
        ws_root: &str,
        reference: &str,
        dry_run: bool,
    ) -> Result<RestoreSummary, AppError> {
        let store = self
            .store()
            .ok_or_else(|| AppError::PersistenceFailed("no store available".into()))?;
        let ref_str = reference.to_string();
        let ws = ws_root.to_string();
        let ck = store
            .read_sync(move |db| {
                db.find_checkpoint(&ws, &ref_str)
                    .map_err(AppError::hide_store_failure)
            })
            .map_err(AppError::hide_store_failure)??
            .ok_or_else(|| {
                AppError::InvalidRequest(format!("no checkpoint matching '{reference}'"))
            })?;
        self.restore_to(ws_root, ck.mark, "restore", dry_run)
    }

    pub fn undo(&self, ws_root: &str, dry_run: bool) -> Result<RestoreSummary, AppError> {
        let store = self
            .store()
            .ok_or_else(|| AppError::PersistenceFailed("no store available".into()))?;
        let ws = ws_root.to_string();
        let (cur, target) = store
            .read_sync(move |db| {
                let cur = db
                    .current_head(&ws)
                    .map_err(AppError::hide_store_failure)?;
                if cur == 0 {
                    return Err(AppError::InvalidRequest(
                        "nothing to undo (no recorded changes)".into(),
                    ));
                }
                let target = db
                    .previous_boundary(&ws, cur)
                    .map_err(AppError::hide_store_failure)?;
                Ok((cur, target))
            })
            .map_err(AppError::hide_store_failure)??;
        if target == cur {
            return Err(AppError::InvalidRequest(
                "already at previous boundary".into(),
            ));
        }
        self.restore_to(ws_root, target, "undo", dry_run)
    }

    pub fn redo(&self, ws_root: &str, dry_run: bool) -> Result<RestoreSummary, AppError> {
        let store = self
            .store()
            .ok_or_else(|| AppError::PersistenceFailed("no store available".into()))?;
        let ws = ws_root.to_string();
        let (cur, redo) = store
            .read_sync(move |db| {
                let cur = db
                    .current_head(&ws)
                    .map_err(AppError::hide_store_failure)?;
                let redo = db
                    .head_state(&ws)
                    .map_err(AppError::hide_store_failure)?
                    .and_then(|(_, r)| r);
                Ok::<_, AppError>((cur, redo))
            })
            .map_err(AppError::hide_store_failure)??;
        let target = match redo {
            Some(t) if t > cur => t,
            Some(_) => {
                return Err(AppError::InvalidRequest(
                    "already at or past redo target".into(),
                ))
            }
            None => return Err(AppError::InvalidRequest("no redo target recorded".into())),
        };
        self.restore_to(ws_root, target, "redo", dry_run)
    }

    pub fn restore_to(
        &self,
        ws_root: &str,
        target: i64,
        reason: &str,
        dry_run: bool,
    ) -> Result<RestoreSummary, AppError> {
        let store = self
            .store()
            .ok_or_else(|| AppError::PersistenceFailed("no store available".into()))?;
        let ws = ws_root.to_string();
        let cur = store
            .read_sync({
                let ws = ws.clone();
                move |db| {
                    db.current_head(&ws)
                        .map_err(AppError::hide_store_failure)
                }
            })
            .map_err(AppError::hide_store_failure)??;

        if target == cur {
            return Ok(RestoreSummary {
                from_mark: cur,
                to_mark: target,
                applied: 0,
                skipped: 0,
                dry_run,
                reason: reason.into(),
                changes: Vec::new(),
            });
        }

        let forward = target > cur;
        let (lo, hi) = if forward {
            (cur, target)
        } else {
            (target, cur)
        };
        let mut changes = store
            .read_sync({
                let ws = ws.clone();
                move |db| {
                    db.workspace_changes_in_range(&ws, lo, hi)
                        .map_err(AppError::hide_store_failure)
                }
            })
            .map_err(AppError::hide_store_failure)??;

        if !forward {
            changes.reverse();
        }

        let mut transitions = std::collections::BTreeMap::new();
        for ch in &changes {
            let (expected, desired) = if forward {
                (ch.before.clone(), ch.after.clone())
            } else {
                (ch.after.clone(), ch.before.clone())
            };
            transitions
                .entry(ch.path.clone())
                .and_modify(|transition: &mut RestoreTransition| {
                    transition.desired = desired.clone()
                })
                .or_insert(RestoreTransition { expected, desired });
        }
        let change_records = apply_restore_transitions(Path::new(ws_root), &transitions, dry_run)?;
        let applied = change_records.len();

        if !dry_run {
            let reason_owned = reason.to_string();
            let ws_owned = ws.clone();
            store
                .write_sync(move |db| {
                    let new_redo = if forward {
                        None
                    } else {
                        let prev = db.head_state(&ws_owned)?.and_then(|(_, r)| r);
                        Some(prev.map_or(cur, |r| r.max(cur)))
                    };
                    db.set_head(&ws_owned, target, new_redo)?;
                    db.record_restore(&ws_owned, cur, target, &reason_owned, applied as i64)?;
                    Ok::<(), anyhow::Error>(())
                })
                .map_err(AppError::hide_store_failure)?
                .map_err(AppError::hide_store_failure)?;
        }

        Ok(RestoreSummary {
            from_mark: cur,
            to_mark: target,
            applied,
            skipped: 0,
            dry_run,
            reason: reason.into(),
            changes: change_records,
        })
    }

    // --- Project Memory ---

    pub fn project_status(&self, ws_root: &str) -> Result<Option<ProjectStatusInfo>, AppError> {
        let store = self
            .store()
            .ok_or_else(|| AppError::PersistenceFailed("no store available".into()))?;
        let root = PathBuf::from(ws_root);
        store
            .read_sync(move |db| {
                let st = db
                    .project_status(&root)
                    .map_err(AppError::hide_store_failure)?;
                Ok(st.map(|s| ProjectStatusInfo {
                    id: s.id,
                    name: s.name,
                    root: s.root,
                    digest_chars: s.digest_chars,
                    note_count: s.note_count,
                    last_active_at: s.last_active_at,
                }))
            })
            .map_err(AppError::hide_store_failure)?
    }

    pub fn ensure_project(&self, ws_root: &str) -> Result<String, AppError> {
        let store = self
            .store()
            .ok_or_else(|| AppError::PersistenceFailed("no store available".into()))?;
        let root = PathBuf::from(ws_root);
        store
            .write_sync(move |db| {
                db.ensure_project(&root)
                    .map_err(AppError::hide_store_failure)
            })
            .map_err(AppError::hide_store_failure)?
    }

    pub fn add_project_note(
        &self,
        ws_root: &str,
        note: &str,
        author: &str,
    ) -> Result<(), AppError> {
        let store = self
            .store()
            .ok_or_else(|| AppError::PersistenceFailed("no store available".into()))?;
        let root = PathBuf::from(ws_root);
        let n = note.to_string();
        let a = author.to_string();
        store
            .write_sync(move |db| {
                db.add_project_note(&root, &n, &a)
                    .map_err(AppError::hide_store_failure)
            })
            .map_err(AppError::hide_store_failure)?
    }

    // --- History ---

    pub fn list_recent_sessions(&self, limit: usize) -> Result<Vec<SessionSummaryInfo>, AppError> {
        let store = self
            .store()
            .ok_or_else(|| AppError::PersistenceFailed("no store available".into()))?;
        let limit_u32 = limit as u32;
        store
            .read_sync(move |db| {
                let rows = db
                    .list_recent_sessions(limit_u32)
                    .map_err(AppError::hide_store_failure)?;
                Ok(rows
                    .into_iter()
                    .map(|s| SessionSummaryInfo {
                        id: s.id,
                        status: s.status,
                        messages: s.messages,
                        tool_calls: s.tool_calls,
                        file_changes: s.file_changes,
                        started_at: s.started_at,
                        workspace_root: s.workspace_root,
                    })
                    .collect())
            })
            .map_err(AppError::hide_store_failure)?
    }

    pub fn session_transcript(
        &self,
        session_id: &str,
    ) -> Result<Vec<(i64, String, String)>, AppError> {
        let store = self
            .store()
            .ok_or_else(|| AppError::PersistenceFailed("no store available".into()))?;
        let sid = session_id.to_string();
        store
            .read_sync(move |db| {
                match db.transcript(&sid) {
                    Ok(rows) => Ok(rows),
                    Err(tetonic_memory::StoreError::ControlAccessDenied) => Ok(Vec::new()),
                    Err(_) => Err(AppError::PersistenceFailed("request failed".into())),
                }
            })
            .map_err(|_| AppError::PersistenceFailed("request failed".into()))?
    }
    // (Code Index methods moved to cli_index.rs)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct RestoreTransition {
    expected: Option<String>,
    desired: Option<String>,
}

fn apply_restore_transitions(
    root: &Path,
    transitions: &std::collections::BTreeMap<String, RestoreTransition>,
    dry_run: bool,
) -> Result<Vec<RestoreFileChange>, AppError> {
    use tetonic_transaction::{WorkspaceTransactionService, WorkspaceTxnConfig};
    let transactions = WorkspaceTransactionService::new(root, WorkspaceTxnConfig::default())
        .map_err(restore_failed)?;
    let result = (|| {
        let mut records = Vec::new();
        for (path, transition) in transitions {
            if transition.expected == transition.desired {
                continue;
            }
            transactions.with_active(|txn| {
                txn.stage_file_transition(
                    path,
                    transition.expected.as_deref(),
                    transition.desired.as_deref(),
                )
            })?;
            records.push(RestoreFileChange {
                verb: if transition.desired.is_some() {
                    "write "
                } else {
                    "delete"
                },
                path: path.clone(),
                error: None,
            });
        }
        if dry_run {
            transactions.abort_active_if_any()?;
        } else {
            transactions.commit_active_if_any(
                "lokai:restore",
                tetonic_domain::DataClass::RepositorySource,
            )?;
        }
        Ok::<_, tetonic_transaction::TransactionError>(records)
    })();
    if result.is_err() {
        // Failed commits requiring recovery remain intact; abort refuses those states.
        let _ = transactions.abort_active_if_any();
    }
    result.map_err(restore_failed)
}

fn restore_failed<E: std::fmt::Display>(error: E) -> AppError {
    AppError::hide_store_failure(error)
}

#[cfg(test)]
mod restore_tests {
    use super::*;

    fn transition(expected: Option<&str>, desired: Option<&str>) -> RestoreTransition {
        RestoreTransition {
            expected: expected.map(str::to_owned),
            desired: desired.map(str::to_owned),
        }
    }

    #[test]
    fn restore_history_only_advances_after_success_and_collapses_repeated_edits() {
        let dir = tempfile::tempdir().unwrap();
        let store = tetonic_memory::SharedStore::open(dir.path().join("audit.db"), 1).unwrap();
        let root = dir.path().display().to_string();
        let ws = root.clone();
        let tip = store
            .write_sync(move |db| {
                let session = db.start_session(&ws, "test", "mock")?;
                db.record_file_change(
                    "call1",
                    &session,
                    "file.txt",
                    "edit",
                    Some("original"),
                    Some("intermediate"),
                )?;
                db.record_file_change(
                    "call2",
                    &session,
                    "file.txt",
                    "edit",
                    Some("intermediate"),
                    Some("latest"),
                )?;
                db.current_head(&ws)
            })
            .unwrap()
            .unwrap();
        let (sink, _) = crate::events::RecordingEventSink::new();
        let app =
            Application::bootstrap_mock_with_store(dir.path(), Some(store.clone()), sink, vec![]);
        std::fs::write(dir.path().join("file.txt"), "user edit").unwrap();
        assert!(app.restore_to(&root, 0, "undo", false).is_err());
        let ws = root.clone();
        assert_eq!(
            store
                .read_sync(move |db| db.current_head(&ws))
                .unwrap()
                .unwrap(),
            tip
        );
        std::fs::write(dir.path().join("file.txt"), "latest").unwrap();
        app.restore_to(&root, 0, "undo", false).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("file.txt")).unwrap(),
            "original"
        );
        app.redo(&root, false).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("file.txt")).unwrap(),
            "latest"
        );
    }

    #[test]
    fn conflict_in_later_file_leaves_entire_restore_unapplied() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "agent a").unwrap();
        std::fs::write(dir.path().join("b.txt"), "user edit").unwrap();
        let transitions = std::collections::BTreeMap::from([
            (
                "a.txt".into(),
                transition(Some("agent a"), Some("original a")),
            ),
            (
                "b.txt".into(),
                transition(Some("agent b"), Some("original b")),
            ),
        ]);
        assert!(apply_restore_transitions(dir.path(), &transitions, false).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "agent a"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("b.txt")).unwrap(),
            "user edit"
        );
    }

    #[test]
    fn restore_preview_preserves_files_and_commit_applies_create_replace_delete() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("replace.txt"), "new").unwrap();
        std::fs::write(dir.path().join("delete.txt"), "created").unwrap();
        let transitions = std::collections::BTreeMap::from([
            ("create.txt".into(), transition(None, Some("restored"))),
            ("replace.txt".into(), transition(Some("new"), Some("old"))),
            ("delete.txt".into(), transition(Some("created"), None)),
        ]);
        assert_eq!(
            apply_restore_transitions(dir.path(), &transitions, true)
                .unwrap()
                .len(),
            3
        );
        assert!(!dir.path().join("create.txt").exists());
        assert!(dir.path().join("delete.txt").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("replace.txt")).unwrap(),
            "new"
        );
        assert_eq!(
            apply_restore_transitions(dir.path(), &transitions, false)
                .unwrap()
                .len(),
            3
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("create.txt")).unwrap(),
            "restored"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("replace.txt")).unwrap(),
            "old"
        );
        assert!(!dir.path().join("delete.txt").exists());
    }

    #[test]
    fn restore_does_not_overwrite_an_unrecorded_file_even_in_preview() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("new.txt"), "user file").unwrap();
        let transitions = std::collections::BTreeMap::from([(
            "new.txt".into(),
            transition(None, Some("restored")),
        )]);
        assert!(apply_restore_transitions(dir.path(), &transitions, true).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("new.txt")).unwrap(),
            "user file"
        );
    }

    #[test]
    fn restore_failure_does_not_repeat_file_bytes() {
        let err = super::restore_failed("io error: PRIVATECANARY file bytes");
        assert_eq!(err.employee_message(), "request failed");
        assert!(!err.to_string().contains("PRIVATECANARY"));
    }
}

#[cfg(test)]
mod transcript_tests {
    use tetonic_memory::ContextOwner;

    #[tokio::test]
    async fn offline_transcript_hides_private_history() {
        let dir = tempfile::tempdir().unwrap();
        let store = tetonic_memory::SharedStore::open(dir.path().join("audit.db"), 1).unwrap();
        let session = store
            .write_sync(|db| {
                db.bootstrap_control("alice", "org", "Org").unwrap();
                db.create_information_context(
                    "alice",
                    "private",
                    &ContextOwner::Private {
                        org_id: "org".into(),
                    },
                )
                .unwrap();
                let id = db.create_context_history("alice", "private").unwrap();
                db.append_context_message(
                    "alice",
                    "private",
                    &id,
                    "note-1",
                    "PRIVATECANARY transcript",
                )
                .unwrap();
                id
            })
            .unwrap();
        let app = crate::Application::bootstrap_mock_with_store(
            dir.path(),
            Some(store),
            std::sync::Arc::new(crate::events::NoopEventSink),
            vec![],
        );
        let rows = app.session_transcript(&session).unwrap();
        let rendered = format!("{rows:?}");
        assert!(rows.is_empty(), "{rendered}");
        assert!(!rendered.contains("PRIVATECANARY"));
        assert!(app.session_transcript("missing-session").unwrap().is_empty());
    }
}

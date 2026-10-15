//! Retain stopped edits as an inert artifact, never a resumable execution or
//! accepted result. The normal abort still removes the live staging area.
use super::*;

impl WorkspaceTransactionService {
    pub fn abort_preserving_draft(&self) -> Result<Option<PathBuf>, TransactionError> {
        let mut guard = self
            .active
            .lock()
            .map_err(|_| TransactionError::Other("transaction lock poisoned".into()))?;
        let Some(txn) = guard.as_mut() else {
            return Ok(None);
        };
        // Never turn unresolved commit recovery into a draft or discard its journal.
        if matches!(
            txn.state,
            TransactionState::Committing | TransactionState::RecoveryRequired
        ) {
            return Err(TransactionError::Other(
                "commit recovery must be resolved before retaining a draft".into(),
            ));
        }
        let draft = txn.retain_draft();
        // Failure to save must not leave live staged effects after a stop.
        txn.abort()?;
        *guard = None;
        draft
    }
}

impl WorkspaceTransaction {
    fn retain_draft(&self) -> Result<Option<PathBuf>, TransactionError> {
        if self.sets.writes.is_empty()
            || matches!(
                self.state,
                TransactionState::Committed | TransactionState::Aborted
            )
        {
            return Ok(None);
        }
        let mut preview = self.preview();
        // Exact contents below are authoritative. Avoid storing a second, lossy
        // full-text copy in the preview (and inflating large draft artifacts).
        preview.unified_diff.clear();
        let mut contents = Vec::new();
        for operation in &mut preview.operations {
            if let Some(path) = operation.new_content_path.take() {
                // Stage contents are already bounded by transaction size limits.
                // Preserve exact bytes (including trailing newlines), not a lossy diff.
                let bytes = std::fs::read(path).map_err(|e| TransactionError::Io(e.to_string()))?;
                if operation.new_digest.as_ref() != Some(&crate::fs_ops::digest_bytes(&bytes)) {
                    return Err(TransactionError::Other(
                        "staged content changed while retaining draft".into(),
                    ));
                }
                contents.push(serde_json::json!({"path":operation.path, "bytes":bytes}));
            }
        }
        let artifact = serde_json::json!({
            "schema_version":1, "kind":"stopped_workspace_draft", "commit_succeeded":false,
            "requires_review":true, "task_id":self.task_id, "attempt_id":self.attempt_id,
            "owner":self.owner, "data_class":DataClass::Secret,
            "preview":preview, "contents":contents,
        });
        let directory = self.lokai_dir.join("artifacts");
        crate::fs_ops::resolve_under_root(&self.root, ".lokai/artifacts")?;
        std::fs::create_dir_all(&directory).map_err(|e| TransactionError::Io(e.to_string()))?;
        let path = directory.join(format!("draft-{}.json", self.id.0));
        let temporary = path.with_extension("pending");
        for candidate in [&path, &temporary] {
            let relative = candidate
                .strip_prefix(&self.root)
                .map_err(|_| TransactionError::Other("draft outside workspace".into()))?;
            crate::fs_ops::resolve_under_root(&self.root, &relative.to_string_lossy())?;
            if crate::fs_ops::is_symlink_or_reparse(candidate) {
                return Err(TransactionError::Other("draft path is a link".into()));
            }
        }
        std::fs::write(
            &temporary,
            serde_json::to_vec(&artifact).map_err(|e| TransactionError::Other(e.to_string()))?,
        )
        .map_err(|e| TransactionError::Io(e.to_string()))?;
        std::fs::rename(&temporary, &path).map_err(|e| TransactionError::Io(e.to_string()))?;
        Ok(Some(path))
    }
}

//! Publish stopped edits as scoped, unaccepted evidence. Never restore effects.
use serde::{Deserialize, Serialize};
use std::{io::Read, sync::Arc};
use tetonic_domain::{artifact::*, AttemptId, DataClass, RunId, TaskId, TransactionPreview};

pub(crate) const MAX_DRAFT_BYTES: usize = 160 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
pub(crate) struct SavedDraft {
    pub schema_version: u32,
    pub files: Vec<SavedDraftFile>,
}
#[derive(Serialize, Deserialize)]
pub(crate) struct SavedDraftFile {
    pub path: String,
    pub operation: tetonic_domain::StagedOperationKind,
    pub byte_count: usize,
    pub content: Option<String>,
}
#[derive(Deserialize)]
struct RawDraft {
    schema_version: u32,
    kind: String,
    commit_succeeded: bool,
    requires_review: bool,
    task_id: Option<TaskId>,
    attempt_id: Option<AttemptId>,
    preview: TransactionPreview,
    contents: Vec<RawFile>,
}
#[derive(Deserialize)]
struct RawFile {
    path: tetonic_domain::WorkspacePath,
    bytes: Vec<u8>,
}

pub(crate) struct DraftRecorder {
    pub tools: Arc<tetonic_tools::Tools>,
    pub artifacts: Arc<dyn ArtifactStore>,
    pub store: tetonic_memory::SharedStore,
    pub scope: tetonic_domain::ExecutionScope,
    pub history: String,
    pub run_id: RunId,
    pub runtime: tokio::runtime::Handle,
    pub audit: Box<dyn tetonic_core::AuditSink>,
}
impl DraftRecorder {
    /// Called only on the existing blocking tool/finalization worker.
    pub fn retain(&self) {
        match self.tools.abort_preserving_draft() {
            Ok(Some(path)) => {
                let result = self.publish(&path);
                if result.is_ok() {
                    self.audit.note("Uncommitted draft saved for inspection. It is not a completed or verified result.");
                } else {
                    self.audit.note("Draft remains in local transaction storage, but its inspection copy could not be saved. No edits were published.");
                    tracing::warn!("stopped draft inspection copy unavailable");
                }
            }
            Ok(None) => {}
            Err(_) => self.audit.note("Stopped edits could not be retained. Inspect work effects before repeating actions."),
        }
    }
    fn publish(&self, path: &std::path::Path) -> Result<(), String> {
        let root = self.tools.workspace().root();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| "draft outside workspace")?;
        tetonic_transaction::fs_ops::resolve_under_root(root, &relative.to_string_lossy())
            .map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        tetonic_transaction::fs_ops::open_read_nofollow(path)
            .map_err(|e| e.to_string())?
            .take(MAX_DRAFT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > MAX_DRAFT_BYTES {
            return Err("draft exceeds inspection limit".into());
        }
        let raw: RawDraft = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let identity = (
            raw.task_id.clone().ok_or("draft task missing")?,
            raw.attempt_id.clone().ok_or("draft attempt missing")?,
        );
        let run = self.run_id.0.clone();
        let snapshot = self
            .store
            .read_sync(move |db| db.load_run_snapshot(&run))
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?
            .ok_or("draft run unavailable")?;
        let task = snapshot
            .tasks
            .get(&identity.0)
            .ok_or("draft task unavailable")?;
        let binding = task
            .binding
            .activation
            .as_ref()
            .or_else(|| task.binding.delegation.as_ref().map(|d| &d.activation));
        if task.binding.execution_scope.as_ref() != Some(&self.scope)
            || binding.is_none_or(|b| b.audit_session_id != self.history)
            || snapshot
                .attempts
                .get(&identity.1)
                .is_none_or(|a| a.task_id != identity.0)
        {
            return Err("draft execution binding mismatch".into());
        }
        if raw.schema_version != 1
            || raw.kind != "stopped_workspace_draft"
            || raw.commit_succeeded
            || !raw.requires_review
            || raw.task_id.as_ref() != Some(&identity.0)
            || raw.attempt_id.as_ref() != Some(&identity.1)
        {
            return Err("draft provenance mismatch".into());
        }
        let mut files = Vec::new();
        for op in raw.preview.operations {
            let content = raw.contents.iter().find(|f| f.path == op.path);
            if let Some(digest) = &op.new_digest {
                if content
                    .is_none_or(|f| &tetonic_transaction::fs_ops::digest_bytes(&f.bytes) != digest)
                {
                    return Err("draft bytes changed".into());
                }
            }
            files.push(SavedDraftFile {
                path: op.path.0,
                operation: op.kind,
                byte_count: content.map_or(0, |f| f.bytes.len()),
                // Text is stored as text so the existing artifact scanner sees it.
                // Binary contents remain in the original inert local draft.
                content: content.and_then(|f| String::from_utf8(f.bytes.clone()).ok()),
            });
        }
        let count = files.len();
        let payload = serde_json::to_vec(&SavedDraft {
            schema_version: 1,
            files,
        })
        .map_err(|e| e.to_string())?;
        self.runtime.block_on(async {
            let mut writer = self
                .artifacts
                .begin_write(ArtifactDeclaration {
                    kind: ArtifactKind::FileSnapshot,
                    producer_run_id: self.run_id.clone(),
                    producer_task_id: identity.0.clone(),
                    producer_attempt_id: identity.1.clone(),
                    worker_id: None,
                    workspace_version: Some(raw.preview.base_version),
                    data_class: DataClass::Secret,
                    retention_policy: RetentionPolicy::ProjectHistory,
                })
                .await
                .map_err(|e| e.to_string())?;
            if let Err(error) = writer.write_chunk(&payload).await {
                let _ = writer.abandon().await;
                return Err(error.to_string());
            }
            let meta = writer.seal().await.map_err(|e| e.to_string())?;
            let receipt = tetonic_memory::WorkspaceDraftReceipt {
                artifact_id: meta.artifact_id.0,
                digest: meta.content_digest.0,
                run_id: self.run_id.0.clone(),
                task_id: identity.0 .0,
                attempt_id: identity.1 .0,
                file_count: count,
            };
            let scope = self.scope.clone();
            let history = self.history.clone();
            self.store
                .write(move |db| {
                    db.record_workspace_draft(
                        &scope.principal_id,
                        &scope.information_context_id,
                        &history,
                        &receipt,
                    )
                })
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())
        })
    }
}

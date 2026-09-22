//! Workspace transaction service (M2-4 production API).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use lokai_domain::{
    AttemptId, CommitResult, DataClass, PatchApproval, TaskId, TransactionArtifact, TransactionId,
    TransactionPreview, TransactionState, VerificationRecord, WorkspacePath, WorkspaceVersion,
};

use crate::commit::{apply_journal, build_journal, detect_conflicts, rollback_journal};
use crate::diff::{classify_paths, unified_diff};
use crate::error::TransactionError;
use crate::journal::CommitJournal;
use crate::lock::WriterLock;
use crate::recovery::{ensure_no_unresolved_recovery, recover_at_startup};
use crate::security::{check_content_size, validate_path, SecurityLimits};
use crate::staging::{patch_digest, ReadWriteSets, StagingArea};
use crate::txn_meta::{remove_staging_dir, TransactionMeta};
use crate::verify_view::{detect_unexpected_mutations, snapshot_read_digests};
use crate::version::capture_workspace_version;

#[derive(Debug, Clone)]
pub struct WorkspaceTxnConfig {
    pub owner: String,
    pub limits: SecurityLimits,
}

impl Default for WorkspaceTxnConfig {
    fn default() -> Self {
        Self {
            owner: format!("pid:{}", std::process::id()),
            limits: SecurityLimits::default(),
        }
    }
}

pub struct WorkspaceTransactionService {
    root: PathBuf,
    lokai_dir: PathBuf,
    config: WorkspaceTxnConfig,
    active: Mutex<Option<WorkspaceTransaction>>,
}

/// Active workspace transaction (single-writer, staged mutations).
pub struct WorkspaceTransaction {
    pub id: TransactionId,
    pub state: TransactionState,
    pub base_version: WorkspaceVersion,
    staging: StagingArea,
    sets: ReadWriteSets,
    staged_bytes: u64,
    limits: SecurityLimits,
    approval: Option<PatchApproval>,
    verification: Option<VerificationRecord>,
    read_snapshot: std::collections::BTreeMap<WorkspacePath, lokai_domain::ContentDigest>,
    _lock: Option<WriterLock>,
    _lease: std::fs::File,
    lokai_dir: PathBuf,
    root: PathBuf,
    owner: String,
    pub task_id: Option<TaskId>,
    pub attempt_id: Option<AttemptId>,
}

impl WorkspaceTransactionService {
    pub fn new(
        root: impl AsRef<Path>,
        config: WorkspaceTxnConfig,
    ) -> Result<Self, TransactionError> {
        let root = std::fs::canonicalize(root.as_ref())
            .map_err(|e| TransactionError::InvalidWorkspace(e.to_string()))?;
        let lokai_dir = root.join(".lokai");
        std::fs::create_dir_all(&lokai_dir).map_err(|e| TransactionError::Io(e.to_string()))?;
        let _ = recover_at_startup(&root)?;
        ensure_no_unresolved_recovery(&lokai_dir)?;
        Ok(Self {
            root,
            lokai_dir,
            config,
            active: Mutex::new(None),
        })
    }

    pub fn with_active<F, R>(&self, f: F) -> Result<R, TransactionError>
    where
        F: FnOnce(&mut WorkspaceTransaction) -> Result<R, TransactionError>,
    {
        let mut guard = self
            .active
            .lock()
            .map_err(|_| TransactionError::Other("transaction lock poisoned".into()))?;
        if guard.is_none() {
            *guard = Some(self.begin()?);
        }
        f(guard.as_mut().unwrap())
    }

    pub fn with_active_if_any<F, R>(&self, f: F) -> Result<Option<R>, TransactionError>
    where
        F: FnOnce(&mut WorkspaceTransaction) -> Result<R, TransactionError>,
    {
        let mut guard = self
            .active
            .lock()
            .map_err(|_| TransactionError::Other("transaction lock poisoned".into()))?;
        match guard.as_mut() {
            Some(txn) => f(txn).map(Some),
            None => Ok(None),
        }
    }

    pub fn clear_active(&self) {
        if let Ok(mut guard) = self.active.lock() {
            *guard = None;
        }
    }

    pub fn verification_view_if_active(&self) -> Result<Option<PathBuf>, TransactionError> {
        let mut guard = self
            .active
            .lock()
            .map_err(|_| TransactionError::Other("transaction lock poisoned".into()))?;
        let Some(txn) = guard.as_mut() else {
            return Ok(None);
        };
        if txn.preview().operations.is_empty() {
            return Ok(None);
        }
        txn.verification_view_path().map(Some)
    }

    pub fn finish_verification_if_active(
        &self,
        record: VerificationRecord,
    ) -> Result<(), TransactionError> {
        let mut guard = self
            .active
            .lock()
            .map_err(|_| TransactionError::Other("transaction lock poisoned".into()))?;
        let Some(txn) = guard.as_mut() else {
            return Ok(());
        };
        txn.finish_verification(record, &self.config.limits)
    }

    pub fn commit_active_if_any(
        &self,
        owner: &str,
        data_class: DataClass,
    ) -> Result<Option<CommitResult>, TransactionError> {
        let mut guard = self
            .active
            .lock()
            .map_err(|_| TransactionError::Other("transaction lock poisoned".into()))?;
        let Some(txn) = guard.as_mut() else {
            return Ok(None);
        };
        if txn.preview().operations.is_empty() {
            return Ok(None);
        }
        let result = {
            let _txn_stage = lokai_telemetry::enter_stage_child("txn_commit");
            txn.commit(owner, data_class)?
        };
        *guard = None;
        Ok(Some(result))
    }

    /// Abort and discard the active staged transaction, if any (R6-3 cancel/fail path).
    pub fn abort_active_if_any(&self) -> Result<bool, TransactionError> {
        let mut guard = self
            .active
            .lock()
            .map_err(|_| TransactionError::Other("transaction lock poisoned".into()))?;
        let Some(mut txn) = guard.take() else {
            return Ok(false);
        };
        txn.abort()?;
        Ok(true)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn begin(&self) -> Result<WorkspaceTransaction, TransactionError> {
        ensure_no_unresolved_recovery(&self.lokai_dir)?;
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = TransactionId::new(format!(
            "txn_{}_{}_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
            std::process::id(),
            NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let lease = crate::lock::transaction_lease(&self.lokai_dir, &id)?;
        let base_version = capture_workspace_version(&self.root, &[])?;
        let staging = StagingArea::create(&self.root, &base_version.repository_id.0, &id.0)?;
        let txn = WorkspaceTransaction {
            id,
            state: TransactionState::Created,
            base_version,
            staging,
            sets: ReadWriteSets::default(),
            staged_bytes: 0,
            limits: self.config.limits.clone(),
            approval: None,
            verification: None,
            read_snapshot: std::collections::BTreeMap::new(),
            _lock: None,
            _lease: lease,
            lokai_dir: self.lokai_dir.clone(),
            root: self.root.clone(),
            owner: self.config.owner.clone(),
            task_id: None,
            attempt_id: None,
        };
        txn.persist_meta()?;
        Ok(txn)
    }
}

impl WorkspaceTransaction {
    fn persist_meta(&self) -> Result<(), TransactionError> {
        let meta = TransactionMeta {
            transaction_id: self.id.clone(),
            base_version: self.base_version.clone(),
            state: self.state,
            staging_dir: self.staging.dir.display().to_string(),
            owner: self.owner.clone(),
            pid: std::process::id(),
        };
        meta.persist(&self.lokai_dir)
    }

    fn transition(&mut self, to: TransactionState) -> Result<(), TransactionError> {
        let ok = matches!(
            (self.state, to),
            (TransactionState::Created, TransactionState::Staging)
                | (TransactionState::Staged, TransactionState::Staging)
                | (TransactionState::Staging, TransactionState::Staging)
                | (TransactionState::Staging, TransactionState::Staged)
                | (TransactionState::Staged, TransactionState::Verifying)
                | (TransactionState::Verifying, TransactionState::ReadyToCommit)
                | (TransactionState::Verifying, TransactionState::Rejected)
                | (TransactionState::Staged, TransactionState::Committing)
                | (
                    TransactionState::ReadyToCommit,
                    TransactionState::Committing
                )
                | (TransactionState::Committing, TransactionState::Committed)
                | (
                    TransactionState::Committing,
                    TransactionState::RecoveryRequired
                )
                | (TransactionState::Created, TransactionState::Aborted)
                | (TransactionState::Staging, TransactionState::Aborted)
                | (TransactionState::Staged, TransactionState::Aborted)
                | (TransactionState::Verifying, TransactionState::Aborted)
                | (TransactionState::ReadyToCommit, TransactionState::Aborted)
                | (TransactionState::Rejected, TransactionState::Aborted)
                | (TransactionState::Rejected, TransactionState::Staging)
                | (TransactionState::Rejected, TransactionState::Verifying)
                | (TransactionState::Staged, TransactionState::Rejected)
                | (TransactionState::ReadyToCommit, TransactionState::Conflict)
        );
        if ok {
            self.state = to;
            self.persist_meta()?;
            Ok(())
        } else {
            Err(TransactionError::InvalidTransition {
                from: self.state,
                to,
            })
        }
    }

    fn enter_staging(&mut self) -> Result<(), TransactionError> {
        match self.state {
            TransactionState::Created | TransactionState::Staged => {
                self.transition(TransactionState::Staging)
            }
            TransactionState::Staging => Ok(()),
            other => Err(TransactionError::InvalidTransition {
                from: other,
                to: TransactionState::Staging,
            }),
        }
    }

    pub fn record_read(&mut self, rel: &str) -> Result<(), TransactionError> {
        let path = validate_path(&self.root, rel)?;
        self.staging.record_read(&self.root, &mut self.sets, &path);
        Ok(())
    }

    pub fn stage_write_file(
        &mut self,
        rel: &str,
        content: &str,
        existed: bool,
    ) -> Result<(), TransactionError> {
        self.enter_staging()?;
        let path = validate_path(&self.root, rel)?;
        check_content_size(&self.limits, self.staged_bytes, content.len() as u64)?;
        self.staged_bytes += content.len() as u64;
        if existed {
            self.staging.stage_replace(
                &self.root,
                &mut self.sets,
                &path,
                content.as_bytes(),
                None,
            )?;
        } else {
            self.staging.stage_create(
                &self.root,
                &mut self.sets,
                &path,
                content.as_bytes(),
                None,
            )?;
        }
        self.approval = None;
        self.transition(TransactionState::Staged)?;
        Ok(())
    }

    pub fn stage_edit_file(
        &mut self,
        rel: &str,
        old: &str,
        new: &str,
    ) -> Result<(String, String), TransactionError> {
        self.enter_staging()?;
        let path = validate_path(&self.root, rel)?;
        let (before, after) =
            self.staging
                .stage_edit(&self.root, &mut self.sets, &path, old, new)?;
        self.staged_bytes += after.len() as u64;
        self.approval = None;
        self.transition(TransactionState::Staged)?;
        Ok((before, after))
    }

    pub fn stage_delete(&mut self, rel: &str) -> Result<(), TransactionError> {
        self.enter_staging()?;
        let path = validate_path(&self.root, rel)?;
        self.staging
            .stage_delete(&self.root, &mut self.sets, &path)?;
        self.approval = None;
        self.transition(TransactionState::Staged)?;
        Ok(())
    }

    /// Stage a complete file transition against an explicitly expected version.
    /// The caller must abort the batch if any transition is rejected.
    pub fn stage_file_transition(
        &mut self,
        rel: &str,
        expected: Option<&str>,
        desired: Option<&str>,
    ) -> Result<(), TransactionError> {
        let path = validate_path(&self.root, rel)?;
        if expected.is_none()
            && self
                .root
                .join(&path.0)
                .try_exists()
                .map_err(|e| TransactionError::Io(e.to_string()))?
        {
            return Err(TransactionError::Other(format!(
                "workspace conflict: {rel} already exists"
            )));
        }
        match desired {
            Some(content) => self.stage_write_file(rel, content, expected.is_some())?,
            None => self.stage_delete(rel)?,
        }
        let operation =
            self.sets.writes.get(&path).ok_or_else(|| {
                TransactionError::Other(format!("missing staged transition: {rel}"))
            })?;
        let expected_digest = expected.map(|text| crate::fs_ops::digest_bytes(text.as_bytes()));
        if operation.base_digest != expected_digest {
            return Err(TransactionError::Other(format!(
                "workspace conflict: {rel} no longer matches the expected content"
            )));
        }
        Ok(())
    }

    pub fn stage_rename(&mut self, from: &str, to: &str) -> Result<(), TransactionError> {
        self.enter_staging()?;
        let from_p = validate_path(&self.root, from)?;
        let to_p = validate_path(&self.root, to)?;
        self.staging
            .stage_rename(&self.root, &mut self.sets, &from_p, &to_p)?;
        self.approval = None;
        self.transition(TransactionState::Staged)?;
        Ok(())
    }

    pub fn stage_mode_change(&mut self, rel: &str, new_mode: u32) -> Result<(), TransactionError> {
        self.enter_staging()?;
        let path = validate_path(&self.root, rel)?;
        self.staging
            .stage_mode_change(&self.root, &mut self.sets, &path, new_mode)?;
        self.approval = None;
        self.transition(TransactionState::Staged)?;
        Ok(())
    }

    pub fn preview(&self) -> TransactionPreview {
        let ops = StagingArea::operations_in_order(&self.sets);
        let patch = patch_digest(&ops);
        let (created, deleted, modified) = classify_paths(&ops);
        TransactionPreview {
            transaction_id: self.id.clone(),
            base_version: self.base_version.clone(),
            patch_digest: patch,
            operations: ops.clone(),
            unified_diff: unified_diff(&self.root.display().to_string(), &ops),
            created_paths: created,
            deleted_paths: deleted,
            modified_paths: modified,
        }
    }

    pub fn rw_sets(&self) -> ReadWriteSets {
        self.sets.clone()
    }

    pub fn staging_area(&self) -> &StagingArea {
        &self.staging
    }

    pub fn stage_write_bytes(
        &mut self,
        rel: &str,
        content: &[u8],
        existed: bool,
    ) -> Result<(), TransactionError> {
        self.enter_staging()?;
        let path = validate_path(&self.root, rel)?;
        check_content_size(&self.limits, self.staged_bytes, content.len() as u64)?;
        self.staged_bytes += content.len() as u64;
        if existed {
            self.staging
                .stage_replace(&self.root, &mut self.sets, &path, content, None)?;
        } else {
            self.staging
                .stage_create(&self.root, &mut self.sets, &path, content, None)?;
        }
        self.approval = None;
        self.transition(TransactionState::Staged)?;
        Ok(())
    }

    pub fn begin_verification(&mut self) -> Result<PathBuf, TransactionError> {
        // Idempotent while Verifying so verify helpers may re-bind the overlay
        // cwd without a second Staged→Verifying transition (R10).
        if self.state != TransactionState::Verifying {
            self.transition(TransactionState::Verifying)?;
            let paths: Vec<_> = self.sets.reads.iter().cloned().collect();
            self.read_snapshot = snapshot_read_digests(&self.root, &paths)?;
        }
        StagingArea::materialize_overlay(&self.root, &self.staging, &self.sets)
    }

    /// Overlay tree for syntax checks during staging. Does not enter Verifying
    /// (CH-5): py_compile must see staged bytes without blocking commit.
    pub fn staged_overlay_path(&mut self) -> Result<PathBuf, TransactionError> {
        StagingArea::materialize_overlay(&self.root, &self.staging, &self.sets)
    }

    pub fn finish_verification(
        &mut self,
        record: VerificationRecord,
        limits: &SecurityLimits,
    ) -> Result<(), TransactionError> {
        let unexpected = detect_unexpected_mutations(
            &self.root,
            &self.staging,
            &self.sets,
            &self.read_snapshot,
        )?;
        if !unexpected.is_empty() {
            let mut rec = record;
            rec.unexpected_mutations = unexpected;
            self.verification = Some(rec);
            self.transition(TransactionState::Rejected)?;
            return Err(TransactionError::Other(
                "verification modified unexpected source files".into(),
            ));
        }
        self.verification = Some(record);
        if self
            .verification
            .as_ref()
            .map(|v| v.success)
            .unwrap_or(false)
        {
            self.transition(TransactionState::ReadyToCommit)?;
        } else {
            self.transition(TransactionState::Rejected)?;
        }
        let _ = limits;
        Ok(())
    }

    pub fn bind_approval(&mut self, approval: PatchApproval) -> Result<(), TransactionError> {
        let preview = self.preview();
        if approval.transaction_id != self.id {
            return Err(TransactionError::ApprovalInvalid(
                "transaction id mismatch".into(),
            ));
        }
        if approval.patch_digest != preview.patch_digest {
            return Err(TransactionError::ApprovalInvalid(
                "patch digest mismatch".into(),
            ));
        }
        if approval.base_version != self.base_version {
            return Err(TransactionError::ApprovalInvalid(
                "base workspace version mismatch".into(),
            ));
        }
        if approval.verification_required && !approval.verification_passed {
            return Err(TransactionError::ApprovalInvalid(
                "verification required but not passed".into(),
            ));
        }
        self.approval = Some(approval);
        Ok(())
    }

    fn ensure_commit_approval(&mut self) -> Result<(), TransactionError> {
        if self.approval.is_some() {
            return Ok(());
        }
        let preview = self.preview();
        let verification_required = self.verification.is_some();
        let verification_passed = self
            .verification
            .as_ref()
            .map(|v| v.success)
            .unwrap_or(true);
        self.bind_approval(PatchApproval {
            transaction_id: preview.transaction_id,
            patch_digest: preview.patch_digest,
            base_version: preview.base_version,
            verification_required,
            verification_passed,
        })
    }

    pub fn commit(
        &mut self,
        owner: &str,
        data_class: DataClass,
    ) -> Result<CommitResult, TransactionError> {
        detect_conflicts(&self.root, &self.base_version, &self.sets)?;
        self.ensure_commit_approval()?;
        if self.state != TransactionState::ReadyToCommit
            && self.state != TransactionState::Staged
            && self.sets.writes.is_empty()
        {
            return Err(TransactionError::Other("empty transaction".into()));
        }
        let preview = self.preview();
        let lock = WriterLock::acquire(&self.lokai_dir, &self.id, owner)?;
        self._lock = Some(lock);
        self.transition(TransactionState::Committing)?;
        let backup_dir = self.lokai_dir.join("backups").join(&self.id.0);
        let mut journal = CommitJournal {
            transaction_id: self.id.clone(),
            base_version: self.base_version.clone(),
            patch_digest: preview.patch_digest.clone(),
            state: TransactionState::Committing,
            operations: Vec::new(),
            progress: 0,
            recovery_instructions: "rollback from backups on failure".into(),
        };
        build_journal(&mut journal, &self.root, &backup_dir, &self.sets)?;
        journal.persist(&self.lokai_dir)?;
        match apply_journal(&self.root, &mut journal, &self.lokai_dir) {
            Ok(()) => {
                self.transition(TransactionState::Committed)?;
                remove_staging_dir(&self.lokai_dir, &self.id)?;
                let result_version =
                    capture_workspace_version(&self.root, &preview.modified_paths)?;
                let artifact = TransactionArtifact {
                    transaction_id: self.id.clone(),
                    base_workspace_version: self.base_version.clone(),
                    result_workspace_version: result_version.clone(),
                    patch_artifact_id: lokai_domain::ids::ArtifactId::new(format!(
                        "patch-{}",
                        preview.patch_digest.0
                    )),
                    task_id: self.task_id.clone(),
                    attempt_id: self.attempt_id.clone(),
                    data_class,
                    verification_artifact_id: self.verification.as_ref().map(|v| {
                        lokai_domain::ids::ArtifactId::new(format!("verify-{}", v.output_digest.0))
                    }),
                    commit_succeeded: true,
                };
                self.persist_artifact(&artifact)?;
                Ok(CommitResult {
                    transaction_id: self.id.clone(),
                    base_version: self.base_version.clone(),
                    result_version,
                    patch_digest: preview.patch_digest,
                    artifact,
                })
            }
            Err(e) => {
                let _ = rollback_journal(&self.root, &mut journal, &self.lokai_dir);
                journal.state = TransactionState::RecoveryRequired;
                journal.persist(&self.lokai_dir)?;
                self.transition(TransactionState::RecoveryRequired)?;
                Err(e)
            }
        }
    }

    pub fn abort(&mut self) -> Result<(), TransactionError> {
        match self.state {
            TransactionState::Committed | TransactionState::Aborted => {
                remove_staging_dir(&self.lokai_dir, &self.id)?;
                return Ok(());
            }
            TransactionState::Committing | TransactionState::RecoveryRequired => {
                return Err(TransactionError::Other(
                    "cannot abort while commit recovery is required; use resume_recovery".into(),
                ));
            }
            _ => {}
        }
        self.transition(TransactionState::Aborted)?;
        remove_staging_dir(&self.lokai_dir, &self.id)?;
        Ok(())
    }

    fn persist_artifact(&self, artifact: &TransactionArtifact) -> Result<(), TransactionError> {
        let path = self
            .lokai_dir
            .join("artifacts")
            .join(format!("{}.json", self.id.0));
        std::fs::create_dir_all(path.parent().unwrap())
            .map_err(|e| TransactionError::Io(e.to_string()))?;
        std::fs::write(
            &path,
            serde_json::to_string_pretty(artifact)
                .map_err(|e| TransactionError::Other(e.to_string()))?,
        )
        .map_err(|e| TransactionError::Io(e.to_string()))?;
        Ok(())
    }

    pub fn verification_view_path(&mut self) -> Result<PathBuf, TransactionError> {
        self.begin_verification()
    }
}

pub fn with_active_transaction<F, R>(
    service: &WorkspaceTransactionService,
    f: F,
) -> Result<R, TransactionError>
where
    F: FnOnce(&mut WorkspaceTransaction) -> Result<R, TransactionError>,
{
    service.with_active(f)
}

pub fn clear_active_transaction(service: &WorkspaceTransactionService) {
    service.clear_active();
}

//! Model-requested checks use the existing transaction and process owners.
//! The approval binds a patch, and the transaction stays locked while that
//! patch is checked. No command-generated files are imported into the patch.
use super::*;
use serde::{Deserialize, Serialize};
use tetonic_domain::work_scope::CancellationSignal;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StagedCheckBinding {
    pub transaction_id: TransactionId,
    pub patch_digest: tetonic_domain::ContentDigest,
}

impl StagedCheckBinding {
    pub fn working_directory(&self, root: &Path) -> Result<PathBuf, TransactionError> {
        let id = &self.transaction_id.0;
        if !id.starts_with("txn_") || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err(TransactionError::Other(
                "invalid staged check identity".into(),
            ));
        }
        Ok(root
            .join(".lokai")
            .join("staging")
            .join(id)
            .join("verify_overlay"))
    }
}

pub struct StagedCheckResult {
    pub success: bool,
    pub output: String,
}

impl WorkspaceTransactionService {
    /// Metadata only: the process and workspace copy are not started before approval.
    pub fn staged_check_binding(&self) -> Result<StagedCheckBinding, TransactionError> {
        self.with_active_if_any(|txn| {
            let preview = txn.preview();
            if preview.operations.is_empty() {
                return Err(TransactionError::Other(
                    "No pending edits to check; use the workspace view.".into(),
                ));
            }
            Ok(StagedCheckBinding {
                transaction_id: preview.transaction_id,
                patch_digest: preview.patch_digest,
            })
        })?
        .ok_or_else(|| {
            TransactionError::Other("No pending edits to check; use the workspace view.".into())
        })
    }

    /// Execute on the owned blocking worker. The lock is per agent transaction,
    /// never a global workspace lock; unrelated workers continue in parallel.
    pub fn run_staged_check(
        &self,
        binding: &StagedCheckBinding,
        command: &str,
        cancel: &CancellationSignal,
        run: impl FnOnce(&Path) -> StagedCheckResult,
    ) -> Result<StagedCheckResult, TransactionError> {
        self.with_active_if_any(|txn| {
            let result = (|| {
            let preview = txn.preview();
            if binding.transaction_id != preview.transaction_id || binding.patch_digest != preview.patch_digest {
                return Err(TransactionError::Other("Pending edits changed after approval. Request a new staged check.".into()));
            }
            check_canceled(cancel)?;
            txn.verification_required = true;
            txn.approval = None;
            txn.verification = None;
            if txn.state != TransactionState::Verifying {
                txn.transition(TransactionState::Verifying)?;
            }
            let overlay = StagingArea::materialize_overlay_cancellable(&txn.root, &txn.staging, &txn.sets, Some(cancel))?;
            if overlay != binding.working_directory(&txn.root)? {
                return Err(TransactionError::Other("staged check directory mismatch".into()));
            }
            let sources = source_digests(&overlay, cancel)?;
            // Tests can read any source in this view. Bind those inputs to commit
            // conflict detection so a peer's changed dependency cannot go unnoticed.
            for (path, digest) in &sources {
                if !txn.sets.writes.contains_key(path) {
                    txn.sets.reads.insert(path.clone());
                    txn.sets.read_baselines.entry(path.clone()).or_insert_with(|| digest.clone());
                }
            }
            txn.read_snapshot = snapshot_read_digests(&txn.root, &txn.sets.reads.iter().cloned().collect::<Vec<_>>())?;
            check_canceled(cancel)?;
            let mut result = run(&overlay);
            if cancel.is_canceled() {
                result.success = false;
                result.output.push_str("\nCheck canceled; no passing verification was recorded.");
            }
            // A formatter or test that rewrites source did not check the exact
            // proposed patch. Generated files may exist, but are never published.
            let changed: Vec<_> = sources.iter().filter(|(path, expected)| {
                !crate::fs_ops::resolve_under_root(&overlay, &path.0)
                    .and_then(|p| crate::fs_ops::digest_file(&p)).is_ok_and(|actual| &actual == *expected)
            }).map(|(path, _)| path.clone()).collect();
            if !changed.is_empty() {
                result.success = false;
                result.output.push_str("\nCheck changed or removed source files in the disposable view. Apply intended changes with file tools and check again.");
            }
            let record = VerificationRecord {
                command: command.into(), sandbox_policy: "governed staged shell".into(),
                workspace_version: txn.base_version.clone(),
                exit_status: if result.success { Some(0) } else { None },
                output_digest: crate::fs_ops::digest_string(&result.output), success: result.success,
                unexpected_mutations: changed,
            };
            txn.finish_verification(record, &self.config.limits)?;
            Ok(result)
            })();
            if result.is_err() && txn.state == TransactionState::Verifying {
                txn.transition(TransactionState::Rejected)?;
            }
            result
        })?.ok_or_else(|| TransactionError::Other("The pending edits no longer exist. Request a new check.".into()))
    }
}

fn check_canceled(cancel: &CancellationSignal) -> Result<(), TransactionError> {
    if cancel.is_canceled() {
        Err(TransactionError::Other("staged check canceled".into()))
    } else {
        Ok(())
    }
}

fn source_digests(
    root: &Path,
    cancel: &CancellationSignal,
) -> Result<
    std::collections::BTreeMap<WorkspacePath, tetonic_domain::ContentDigest>,
    TransactionError,
> {
    let mut result = std::collections::BTreeMap::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        check_canceled(cancel)?;
        for entry in std::fs::read_dir(dir).map_err(|e| TransactionError::Io(e.to_string()))? {
            check_canceled(cancel)?;
            let path = entry
                .map_err(|e| TransactionError::Io(e.to_string()))?
                .path();
            if crate::fs_ops::is_symlink_or_reparse(&path) {
                return Err(TransactionError::Other(
                    "staged check cannot follow symbolic links".into(),
                ));
            }
            if path.is_dir() {
                dirs.push(path);
            } else {
                result.insert(
                    crate::fs_ops::rel_path(root, &path)?,
                    crate::fs_ops::digest_file(&path)?,
                );
            }
        }
    }
    Ok(result)
}

//! Startup recovery for incomplete commit journals and abandoned staged transactions (R6-3).

use std::path::Path;

use tetonic_domain::TransactionState;

use crate::commit::rollback_journal;
use crate::error::TransactionError;
use crate::journal::{list_incomplete_journals, CommitJournal};
use crate::lock::WriterLock;
use crate::txn_meta::{list_incomplete_txn_meta, remove_staging_dir, TransactionMeta};

#[derive(Debug, Default)]
pub struct RecoveryReport {
    pub recovered: Vec<String>,
    pub requires_manual: Vec<String>,
    /// Abandoned staged/created transactions discarded without touching the workspace.
    pub aborted_staged: Vec<String>,
}

/// Transaction IDs with journals in `RecoveryRequired` that must be resolved first.
pub fn list_unresolved_recovery(lokai_dir: &Path) -> Result<Vec<String>, TransactionError> {
    let mut txns = Vec::new();
    for path in list_incomplete_journals(lokai_dir)? {
        let journal = CommitJournal::load(&path)?;
        if journal.state == TransactionState::RecoveryRequired {
            txns.push(journal.transaction_id.0);
        }
    }
    Ok(txns)
}

pub fn ensure_no_unresolved_recovery(lokai_dir: &Path) -> Result<(), TransactionError> {
    let pending = list_unresolved_recovery(lokai_dir)?;
    if pending.is_empty() {
        Ok(())
    } else {
        Err(TransactionError::RecoveryRequired(format!(
            "unresolved commit journals: {}",
            pending.join(", ")
        )))
    }
}

/// Discard incomplete stage metadata + staging dirs that never reached commit.
/// Workspace files are never half-applied in this path (staging is outside the tree).
pub fn recover_abandoned_stages(
    lokai_dir: &Path,
    report: &mut RecoveryReport,
) -> Result<(), TransactionError> {
    let journal_owned: std::collections::HashSet<String> = list_incomplete_journals(lokai_dir)?
        .into_iter()
        .filter_map(|p| CommitJournal::load(&p).ok())
        .map(|j| j.transaction_id.0)
        .collect();
    for path in list_incomplete_txn_meta(lokai_dir)? {
        let mut meta = TransactionMeta::load(&path)?;
        if journal_owned.contains(&meta.transaction_id.0) {
            // Incomplete commit journal owns recovery for this txn.
            continue;
        }
        let _lease = match crate::lock::transaction_lease(lokai_dir, &meta.transaction_id) {
            Ok(lease) => lease,
            Err(TransactionError::LockContention(_)) => continue,
            Err(e) => return Err(e),
        };
        let txn = meta.transaction_id.0.clone();
        remove_staging_dir(lokai_dir, &meta.transaction_id)?;
        meta.state = TransactionState::Aborted;
        meta.persist(lokai_dir)?;
        report
            .aborted_staged
            .push(format!("aborted incomplete stage {txn}"));
    }
    Ok(())
}

pub fn recover_at_startup(root: &Path) -> Result<RecoveryReport, TransactionError> {
    let lokai_dir = root.join(".lokai");
    let mut report = RecoveryReport::default();
    let _writer = WriterLock::acquire(
        &lokai_dir,
        &tetonic_domain::TransactionId::new("recovery"),
        "recovery",
    )?;
    for path in list_incomplete_journals(&lokai_dir)? {
        let mut journal = CommitJournal::load(&path)?;
        let _lease = match crate::lock::transaction_lease(&lokai_dir, &journal.transaction_id) {
            Ok(lease) => lease,
            Err(TransactionError::LockContention(_)) => continue,
            Err(e) => return Err(e),
        };
        let txn = journal.transaction_id.0.clone();
        match journal.state {
            TransactionState::Committing => {
                if rollback_journal(root, &mut journal, &lokai_dir).is_ok() {
                    journal.state = TransactionState::Aborted;
                    journal.persist(&lokai_dir)?;
                    report.recovered.push(format!("rolled back {txn}"));
                } else {
                    journal.state = TransactionState::RecoveryRequired;
                    journal.persist(&lokai_dir)?;
                    report.requires_manual.push(txn);
                }
            }
            TransactionState::RecoveryRequired => {
                report.requires_manual.push(txn);
            }
            _ => {}
        }
    }
    recover_abandoned_stages(&lokai_dir, &mut report)?;
    Ok(report)
}

pub fn resume_recovery(root: &Path, txn_id: &str, complete: bool) -> Result<(), TransactionError> {
    use crate::commit::apply_journal;
    let lokai_dir = root.join(".lokai");
    let id = tetonic_domain::TransactionId::new(txn_id);
    let _lease = crate::lock::transaction_lease(&lokai_dir, &id)?;
    let _writer = WriterLock::acquire(&lokai_dir, &id, "recovery")?;
    let path = CommitJournal::path(&lokai_dir, &id);
    let mut journal = CommitJournal::load(&path)?;
    if complete {
        crate::commit::reconcile_progress(root, &mut journal)?;
        apply_journal(root, &mut journal, &lokai_dir)?;
    } else {
        rollback_journal(root, &mut journal, &lokai_dir)?;
        journal.state = TransactionState::Aborted;
        journal.persist(&lokai_dir)?;
    }
    Ok(())
}

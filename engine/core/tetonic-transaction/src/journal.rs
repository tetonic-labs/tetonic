//! Write-ahead commit journal.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use tetonic_domain::{
    ContentDigest, StagedOperation, TransactionId, TransactionState, WorkspacePath,
    WorkspaceVersion,
};

use crate::error::TransactionError;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum RenameDestinationBackup {
    Absent,
    File(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct JournalOperation {
    pub index: u32,
    pub operation: StagedOperation,
    pub backup_path: Option<String>,
    /// None denotes an older journal with unknown destination state. Recovery
    /// must not guess whether deleting the destination would destroy user data.
    #[serde(default)]
    pub rename_destination: Option<RenameDestinationBackup>,
    pub expected_digest: Option<ContentDigest>,
    pub new_content_path: String,
    /// None is a legacy journal whose mutation boundary is unknown.
    #[serde(default)]
    pub started: Option<bool>,
    pub completed: bool,
    pub rolled_back: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommitJournal {
    pub transaction_id: TransactionId,
    pub base_version: WorkspaceVersion,
    pub patch_digest: ContentDigest,
    pub state: TransactionState,
    pub operations: Vec<JournalOperation>,
    pub progress: u32,
    pub recovery_instructions: String,
}

impl CommitJournal {
    pub fn path(lokai_dir: &Path, txn_id: &TransactionId) -> PathBuf {
        lokai_dir
            .join("journals")
            .join(format!("{}.json", txn_id.0))
    }

    pub fn load(path: &Path) -> Result<Self, TransactionError> {
        let s = std::fs::read_to_string(path).map_err(|e| TransactionError::Io(e.to_string()))?;
        serde_json::from_str(&s).map_err(|e| TransactionError::Other(e.to_string()))
    }

    pub fn persist(&self, lokai_dir: &Path) -> Result<(), TransactionError> {
        let path = Self::path(lokai_dir, &self.transaction_id);
        std::fs::create_dir_all(path.parent().unwrap())
            .map_err(|e| TransactionError::Io(e.to_string()))?;
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| TransactionError::Other(e.to_string()))?;
        {
            let mut file = File::create(&tmp).map_err(|e| TransactionError::Io(e.to_string()))?;
            file.write_all(json.as_bytes())
                .map_err(|e| TransactionError::Io(e.to_string()))?;
            file.sync_all()
                .map_err(|e| TransactionError::Io(e.to_string()))?;
        }
        std::fs::rename(&tmp, &path).map_err(|e| TransactionError::Io(e.to_string()))?;
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .and_then(|file| file.sync_all())
            .map_err(|e| TransactionError::Io(e.to_string()))?;
        crate::publication::sync_directory_chain(path.parent().unwrap(), lokai_dir)
            .map_err(|e| TransactionError::Io(e.to_string()))?;
        Ok(())
    }

    pub fn mark_completed(&mut self, index: u32) -> Result<(), TransactionError> {
        if let Some(op) = self.operations.iter_mut().find(|o| o.index == index) {
            op.completed = true;
            self.progress = index + 1;
            Ok(())
        } else {
            Err(TransactionError::Other(format!(
                "unknown journal op {index}"
            )))
        }
    }

    pub fn mark_rolled_back(&mut self, index: u32) -> Result<(), TransactionError> {
        if let Some(op) = self.operations.iter_mut().find(|o| o.index == index) {
            op.rolled_back = true;
            Ok(())
        } else {
            Err(TransactionError::Other(format!(
                "unknown journal op {index}"
            )))
        }
    }
}

pub fn list_incomplete_journals(lokai_dir: &Path) -> Result<Vec<PathBuf>, TransactionError> {
    let dir = lokai_dir.join("journals");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| TransactionError::Io(e.to_string()))? {
        let entry = entry.map_err(|e| TransactionError::Io(e.to_string()))?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            if let Ok(j) = CommitJournal::load(&path) {
                if matches!(
                    j.state,
                    TransactionState::Committing | TransactionState::RecoveryRequired
                ) {
                    out.push(path);
                }
            }
        }
    }
    Ok(out)
}

pub fn touched_paths(journal: &CommitJournal) -> Vec<WorkspacePath> {
    journal
        .operations
        .iter()
        .flat_map(|o| {
            let mut v = vec![o.operation.path.clone()];
            if let Some(d) = &o.operation.destination {
                v.push(d.clone());
            }
            v
        })
        .collect()
}

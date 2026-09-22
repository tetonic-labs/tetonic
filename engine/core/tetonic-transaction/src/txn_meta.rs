//! Durable transaction metadata under `.lokai/transactions/` (R6-3).

use std::path::{Path, PathBuf};

use tetonic_domain::{TransactionId, TransactionState, WorkspaceVersion};

use crate::error::TransactionError;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionMeta {
    pub transaction_id: TransactionId,
    pub base_version: WorkspaceVersion,
    pub state: TransactionState,
    #[serde(default)]
    pub staging_dir: String,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub pid: u32,
}

impl TransactionMeta {
    pub fn path(lokai_dir: &Path, txn_id: &TransactionId) -> PathBuf {
        lokai_dir
            .join("transactions")
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
        std::fs::write(&tmp, json).map_err(|e| TransactionError::Io(e.to_string()))?;
        std::fs::rename(&tmp, &path).map_err(|e| TransactionError::Io(e.to_string()))?;
        Ok(())
    }
}

/// Incomplete (non-terminal) transaction metadata files eligible for staged recovery.
pub fn list_incomplete_txn_meta(lokai_dir: &Path) -> Result<Vec<PathBuf>, TransactionError> {
    let dir = lokai_dir.join("transactions");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| TransactionError::Io(e.to_string()))? {
        let entry = entry.map_err(|e| TransactionError::Io(e.to_string()))?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            if let Ok(meta) = TransactionMeta::load(&path) {
                if !meta.state.is_terminal()
                    && !matches!(
                        meta.state,
                        TransactionState::Committing | TransactionState::RecoveryRequired
                    )
                {
                    out.push(path);
                }
            }
        }
    }
    Ok(out)
}

pub fn remove_staging_dir(lokai_dir: &Path, id: &TransactionId) -> Result<(), TransactionError> {
    if id.0.is_empty()
        || !id
            .0
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(TransactionError::Other("invalid transaction id".into()));
    }
    let root = lokai_dir
        .parent()
        .ok_or_else(|| TransactionError::Other("missing workspace".into()))?
        .canonicalize()
        .map_err(|e| TransactionError::Io(e.to_string()))?;
    let path = lokai_dir.join("staging").join(&id.0);
    if !path
        .try_exists()
        .map_err(|e| TransactionError::Io(e.to_string()))?
    {
        return Ok(());
    }
    let resolved = path
        .canonicalize()
        .map_err(|e| TransactionError::Io(e.to_string()))?;
    if !resolved.starts_with(root.join(".lokai").join("staging"))
        || resolved != root.join(".lokai").join("staging").join(&id.0)
    {
        return Err(TransactionError::Other(
            "staging path escapes owned directory".into(),
        ));
    }
    std::fs::remove_dir_all(&path).map_err(|e| TransactionError::Io(e.to_string()))
}

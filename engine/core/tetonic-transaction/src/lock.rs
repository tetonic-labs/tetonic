//! Per-workspace single-writer lock (cross-process).

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use fs2::FileExt;
use tetonic_domain::TransactionId;

use crate::error::TransactionError;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WriterLockRecord {
    pub pid: u32,
    pub transaction_id: TransactionId,
    pub owner: String,
    pub acquired_at: i64,
}

pub struct WriterLock {
    file: File,
}

impl WriterLock {
    pub fn acquire(
        lokai_dir: &Path,
        transaction_id: &TransactionId,
        owner: &str,
    ) -> Result<Self, TransactionError> {
        std::fs::create_dir_all(lokai_dir).map_err(|e| TransactionError::Io(e.to_string()))?;
        let path = lokai_dir.join("writer.lock");
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| TransactionError::Io(e.to_string()))?;
        file.try_lock_exclusive()
            .map_err(|e| TransactionError::LockContention(e.to_string()))?;
        let record = WriterLockRecord {
            pid: std::process::id(),
            transaction_id: transaction_id.clone(),
            owner: owner.to_string(),
            acquired_at: chrono::Utc::now().timestamp(),
        };
        file.set_len(0)
            .map_err(|e| TransactionError::Io(e.to_string()))?;
        let json = serde_json::to_string_pretty(&record)
            .map_err(|e| TransactionError::Other(e.to_string()))?;
        file.write_all(json.as_bytes())
            .map_err(|e| TransactionError::Io(e.to_string()))?;
        file.sync_all()
            .map_err(|e| TransactionError::Io(e.to_string()))?;
        Ok(Self { file })
    }

    pub fn try_read(lokai_dir: &Path) -> Result<Option<WriterLockRecord>, TransactionError> {
        let path = lokai_dir.join("writer.lock");
        if !path.exists() {
            return Ok(None);
        }
        let mut s = String::new();
        File::open(&path)
            .and_then(|mut f| f.read_to_string(&mut s))
            .map_err(|e| TransactionError::Io(e.to_string()))?;
        if s.trim().is_empty() {
            return Ok(None);
        }
        let rec: WriterLockRecord =
            serde_json::from_str(&s).map_err(|e| TransactionError::Other(e.to_string()))?;
        Ok(Some(rec))
    }

    pub fn is_abandoned(record: &WriterLockRecord) -> bool {
        #[cfg(unix)]
        {
            use std::process::Command;
            let out = Command::new("kill")
                .args(["-0", &record.pid.to_string()])
                .output();
            !matches!(out, Ok(o) if o.status.success())
        }
        #[cfg(windows)]
        {
            use std::process::Command;
            let out = Command::new("tasklist")
                .args(["/FI", &format!("PID eq {}", record.pid)])
                .output();
            match out {
                Ok(o) => {
                    let s = String::from_utf8_lossy(&o.stdout);
                    !s.contains(&record.pid.to_string())
                }
                Err(_) => true,
            }
        }
    }
}

impl Drop for WriterLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
        // Keep the inode: unlinking after unlock races a new owner.
    }
}

/// OS-owned per-transaction lease shared by live work and recovery.
pub(crate) fn transaction_lease(
    lokai_dir: &Path,
    id: &TransactionId,
) -> Result<File, TransactionError> {
    if id.0.is_empty()
        || !id
            .0
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(TransactionError::Other("invalid transaction id".into()));
    }
    let dir = lokai_dir.join("leases");
    std::fs::create_dir_all(&dir).map_err(|e| TransactionError::Io(e.to_string()))?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join(format!("{}.lock", id.0)))
        .map_err(|e| TransactionError::Io(e.to_string()))?;
    file.try_lock_exclusive()
        .map_err(|e| TransactionError::LockContention(e.to_string()))?;
    Ok(file)
}

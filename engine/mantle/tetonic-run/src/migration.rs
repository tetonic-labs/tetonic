//! Migration coordination and Safe Mode (M3-4 / R05).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tracing::{error, info, warn};

pub struct MigrationManager {
    safe_mode: AtomicBool,
    db_path: Mutex<Option<PathBuf>>,
}

impl Default for MigrationManager {
    fn default() -> Self {
        Self {
            safe_mode: AtomicBool::new(false),
            db_path: Mutex::new(None),
        }
    }
}

impl MigrationManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_db_path(&self, path: PathBuf) {
        *self.db_path.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
    }

    pub fn enter_safe_mode(&self, reason: &str) {
        error!("Entering Safe Mode: {}", reason);
        self.safe_mode.store(true, Ordering::SeqCst);
    }

    pub fn is_safe_mode(&self) -> bool {
        self.safe_mode.load(Ordering::SeqCst)
    }

    /// Create a real on-disk backup of the configured database before a major migration.
    ///
    /// On failure: enters Safe Mode and returns `Err` (fail-closed).
    pub fn pre_migration_backup(&self) -> Result<PathBuf, String> {
        let path = self
            .db_path
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or_else(|| "pre-migration backup: no database path configured".to_string())?;
        info!("Creating pre-migration backup for {}", path.display());
        match tetonic_memory::pre_migration_backup(&path) {
            Ok(dest) => {
                info!("Pre-migration backup written to {}", dest.display());
                Ok(dest)
            }
            Err(e) => {
                self.enter_safe_mode(&format!("pre-migration backup failed: {e}"));
                Err(e.to_string())
            }
        }
    }

    pub fn handle_v1_legacy_session(&self, session_id: &str) {
        warn!(
            "Detected V1 session: {}. Tagging as LegacyExecution.",
            session_id
        );
    }
}

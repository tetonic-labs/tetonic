//! Transaction size limits and path security.

use std::path::Path;

use crate::error::TransactionError;
use crate::fs_ops::resolve_under_root;
use tetonic_domain::WorkspacePath;

#[derive(Debug, Clone)]
pub struct SecurityLimits {
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
}

impl Default for SecurityLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 8 * 1024 * 1024,
            max_total_bytes: 32 * 1024 * 1024,
        }
    }
}

pub fn validate_path(root: &Path, rel: &str) -> Result<WorkspacePath, TransactionError> {
    let norm = rel.replace('\\', "/");
    if norm.is_empty() || norm.split('/').any(forbidden_component) {
        return Err(TransactionError::OutsideWorkspace(rel.to_string()));
    }
    let resolved = resolve_under_root(root, &norm)
        .map_err(|_| TransactionError::OutsideWorkspace(rel.to_string()))?;
    // Existing ancestors may have alternate filesystem spellings (e.g. Windows
    // short names). Check the filesystem's resolved spelling as well as input.
    let canonical_root = std::fs::canonicalize(root)
        .map_err(|_| TransactionError::OutsideWorkspace(rel.to_string()))?;
    if let Some(existing) = resolved.ancestors().find(|p| p.exists()) {
        let canonical = std::fs::canonicalize(existing)
            .map_err(|_| TransactionError::OutsideWorkspace(rel.to_string()))?;
        let relative = canonical
            .strip_prefix(&canonical_root)
            .map_err(|_| TransactionError::OutsideWorkspace(rel.to_string()))?;
        if relative
            .components()
            .any(|c| forbidden_component(&c.as_os_str().to_string_lossy()))
        {
            return Err(TransactionError::OutsideWorkspace(rel.to_string()));
        }
    }
    Ok(WorkspacePath::new(norm))
}

fn forbidden_component(component: &str) -> bool {
    if component == ".." {
        return true;
    }
    // Reserve these names on every platform, including case-insensitive volumes.
    let name = component.trim_end_matches([' ', '.']).to_ascii_lowercase();
    if matches!(
        name.as_str(),
        ".lokai"
            | ".ssh"
            | ".aws"
            | "lokai.db"
            | "lokai.db-wal"
            | "lokai.db-shm"
            | "lokai.db-journal"
    ) {
        return true;
    }
    // Reject alternate data streams and ambiguous Win32 path normalization.
    #[cfg(windows)]
    if component.contains(':')
        || (component != "." && component != component.trim_end_matches([' ', '.']))
    {
        return true;
    }
    false
}

pub fn check_content_size(
    limits: &SecurityLimits,
    staged_total: u64,
    new_bytes: u64,
) -> Result<(), TransactionError> {
    if new_bytes > limits.max_file_bytes {
        return Err(TransactionError::SizeLimit(format!(
            "file exceeds {} bytes",
            limits.max_file_bytes
        )));
    }
    if staged_total + new_bytes > limits.max_total_bytes {
        return Err(TransactionError::SizeLimit(format!(
            "transaction exceeds {} bytes",
            limits.max_total_bytes
        )));
    }
    Ok(())
}

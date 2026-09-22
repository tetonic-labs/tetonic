//! Shared store utilities (ids, timestamps, workspace keys).

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::Utc;

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) fn now() -> String {
    Utc::now().to_rfc3339()
}

/// A short, locally-unique id with the given prefix (`sess`, `tc`, ...).
pub fn new_id(prefix: &str) -> String {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = Utc::now().timestamp_nanos_opt().unwrap_or_default() as u128;
    format!("{prefix}_{:x}{:x}", nanos, n)
}

/// Canonical workspace key stored in `lokai.db` (forward slashes, canonical path when present).
pub fn workspace_storage_key(path: &Path) -> String {
    if let Ok(c) = std::fs::canonicalize(path) {
        return c.to_string_lossy().replace('\\', "/");
    }
    path.to_string_lossy().replace('\\', "/")
}

pub fn workspace_storage_key_str(raw: &str) -> String {
    workspace_storage_key(Path::new(raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn workspace_key_canonicalizes_existing_dir() {
        let dir = tempdir().unwrap();
        let a = workspace_storage_key(dir.path());
        let b = workspace_storage_key_str(&dir.path().to_string_lossy());
        assert_eq!(a, b);
        assert!(!a.contains('\\'));
    }

    #[test]
    fn workspace_key_normalizes_slashes_for_missing_path() {
        let key = workspace_storage_key_str("/tmp/nonexistent-lokai-test-ws");
        assert_eq!(key, "/tmp/nonexistent-lokai-test-ws");
    }

    /// H4-1: one on-disk directory is one storage key whether the caller passed
    /// a plain path or a Windows `\\?\` verbatim path. Canonicalize on Windows
    /// typically returns the verbatim form; slash-normalization then yields
    /// `//?/C:/...`, which is self-consistent.
    #[test]
    fn windows_verbatim_and_plain_paths_share_one_storage_key() {
        let dir = tempdir().unwrap();
        let plain = workspace_storage_key(dir.path());
        let canon = dir.path().canonicalize().unwrap();
        let via_canon = workspace_storage_key(&canon);
        assert_eq!(plain, via_canon);
        assert!(!plain.contains('\\'));

        #[cfg(windows)]
        {
            let raw = canon.to_string_lossy();
            let verbatim = if raw.starts_with(r"\\?\") {
                raw.into_owned()
            } else {
                format!(r"\\?\{raw}")
            };
            let via_verbatim = workspace_storage_key(Path::new(&verbatim));
            assert_eq!(
                plain, via_verbatim,
                "verbatim prefix must not mint a second workspace key"
            );
        }
    }
}

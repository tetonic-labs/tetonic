//! Verification view and unexpected mutation detection.

use std::path::Path;

use lokai_domain::{ContentDigest, WorkspacePath};

use crate::error::TransactionError;
use crate::staging::{ReadWriteSets, StagingArea};

pub fn detect_unexpected_mutations(
    root: &Path,
    staging: &StagingArea,
    sets: &ReadWriteSets,
    before: &std::collections::BTreeMap<WorkspacePath, ContentDigest>,
) -> Result<Vec<WorkspacePath>, TransactionError> {
    let mut unexpected = Vec::new();
    for (path, old) in before {
        if sets.writes.contains_key(path) {
            continue;
        }
        let abs = crate::version::resolve_safe(root, path)?;
        if abs.is_file() {
            let now = crate::fs_ops::digest_file(&abs)?;
            if &now != old {
                unexpected.push(path.clone());
            }
        }
    }
    let _ = staging;
    Ok(unexpected)
}

pub fn snapshot_read_digests(
    root: &Path,
    paths: &[WorkspacePath],
) -> Result<std::collections::BTreeMap<WorkspacePath, ContentDigest>, TransactionError> {
    let mut out = std::collections::BTreeMap::new();
    for p in paths {
        let abs = crate::version::resolve_safe(root, p)?;
        if abs.is_file() {
            out.insert(p.clone(), crate::fs_ops::digest_file(&abs)?);
        }
    }
    Ok(out)
}

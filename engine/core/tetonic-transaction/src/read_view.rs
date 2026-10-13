//! Reads from the owning transaction without copying the whole workspace.

use std::path::Path;

use tetonic_domain::StagedOperationKind;

use crate::error::TransactionError;
use crate::fs_ops::{
    digest_string, is_symlink_or_reparse, read_to_string_nofollow, resolve_under_root,
};
use crate::security::validate_path;
use crate::staging::{ReadWriteSets, StagingArea};

pub(crate) fn read_staged_file(
    root: &Path,
    staging: &StagingArea,
    sets: &ReadWriteSets,
    rel: &str,
) -> Result<Option<String>, TransactionError> {
    let path = validate_path(root, rel)?;
    let resolved = resolve_under_root(root, &path.0)?;
    if is_symlink_or_reparse(&resolved) {
        return Err(TransactionError::OutsideWorkspace(rel.into()));
    }
    for operation in sets.writes.values() {
        // Compare resolved paths so `./file` and platform separators see the
        // same pending write, without weakening the existing path checks.
        if resolve_under_root(root, &operation.path.0)? != resolved {
            continue;
        }
        if matches!(
            operation.kind,
            StagedOperationKind::Delete | StagedOperationKind::Rename
        ) {
            return Err(TransactionError::Other(format!(
                "file removed in this transaction: {rel}"
            )));
        }
        let Some(content_path) = operation.new_content_path.as_ref() else {
            return Ok(None);
        };
        let relative = Path::new(content_path)
            .strip_prefix(&staging.dir)
            .map_err(|_| TransactionError::OutsideWorkspace(rel.into()))?;
        let staged = resolve_under_root(&staging.dir, &relative.to_string_lossy())?;
        let text = read_to_string_nofollow(&staged)?;
        if operation.new_digest.as_ref() != Some(&digest_string(&text)) {
            return Err(TransactionError::Other(format!(
                "staged content changed unexpectedly: {rel}"
            )));
        }
        return Ok(Some(text));
    }
    for operation in sets.writes.values() {
        if operation.kind == StagedOperationKind::Rename {
            if let Some(to) = &operation.destination {
                if resolve_under_root(root, &to.0)? == resolved {
                    let source = resolve_under_root(root, &operation.path.0)?;
                    return read_to_string_nofollow(&source).map(Some);
                }
            }
        }
    }
    Ok(None)
}

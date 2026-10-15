//! Read-only inspection of this transaction's pending changes and check receipt.
use super::*;
use std::io::Read;
use tetonic_domain::StagedOperationKind;

impl WorkspaceTransaction {
    pub fn inspection_report(&self) -> Result<String, TransactionError> {
        let operations = StagingArea::operations_in_order(&self.sets);
        let mut report = format!(
            "Pending changes for this agent only. Not published; not Git status.\nPatch: {}\nState: {:?}\n",
            patch_digest(&operations).0, self.state
        );
        match &self.verification {
            Some(check) => report.push_str(&format!(
                "Requested check {}: {:?}\nThis records that command's result, not proof of overall correctness.\n",
                if check.success { "passed" } else { "failed" }, check.command
            )),
            None => report.push_str(if self.verification_required {
                "Verification: needs a new passing check before delivery.\n"
            } else {
                "Verification: not checked.\n"
            }),
        }
        report.push_str(&format!("{} changed paths:\n", operations.len()));
        for op in &operations {
            report.push_str(&format!("  {:?}: {:?}\n", op.kind, op.path.0));
        }
        for op in &operations {
            if !matches!(
                op.kind,
                StagedOperationKind::Create
                    | StagedOperationKind::Replace
                    | StagedOperationKind::Edit
            ) {
                report.push_str(&format!(
                    "\n{:?}: {:?}, destination {:?}\n",
                    op.kind, op.path.0, op.destination
                ));
                continue;
            }
            let old = if op.kind == StagedOperationKind::Create {
                String::new()
            } else {
                let path = crate::fs_ops::resolve_under_root(&self.root, &op.path.0)?;
                if crate::fs_ops::is_symlink_or_reparse(&path) {
                    return Err(TransactionError::Other(
                        "cannot inspect a symbolic link".into(),
                    ));
                }
                let mut bytes = Vec::new();
                crate::fs_ops::open_read_nofollow(&path)?
                    .read_to_end(&mut bytes)
                    .map_err(|e| TransactionError::Io(e.to_string()))?;
                if op.base_digest.as_ref() != Some(&crate::fs_ops::digest_bytes(&bytes)) {
                    report.push_str(&format!("\n{:?}: live file changed since staging; diff unavailable. Resolve the conflict before delivery.\n", op.path.0));
                    continue;
                }
                match String::from_utf8(bytes) {
                    Ok(text) => text,
                    Err(_) => {
                        report.push_str(&format!(
                            "\n{:?}: binary change; no text diff.\n",
                            op.path.0
                        ));
                        continue;
                    }
                }
            };
            let new = self.read_staged_file(&op.path.0)?.unwrap_or_default();
            report.push_str(&text_diff(&op.path.0, &old, &new));
        }
        Ok(report)
    }
}

// A linear single-hunk diff. Trim equal edges and retain three context lines;
// avoid quadratic edit-distance work for large generated files. Pages bound output.
fn text_diff(path: &str, before: &str, after: &str) -> String {
    if before == after {
        return format!("\n{path:?}: no content change.\n");
    }
    let old: Vec<_> = before.split_inclusive('\n').collect();
    let new: Vec<_> = after.split_inclusive('\n').collect();
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let start = prefix.saturating_sub(3);
    let old_end = (old.len() - suffix + 3).min(old.len());
    let new_end = (new.len() - suffix + 3).min(new.len());
    let mut out = format!(
        "\n--- a/{path}\n+++ b/{path}\n@@ -{},{} +{},{} @@\n",
        if old.is_empty() { 0 } else { start + 1 },
        old_end - start,
        if new.is_empty() { 0 } else { start + 1 },
        new_end - start
    );
    let mut line = |marker: char, value: &str| {
        out.push(marker);
        out.push_str(value);
        if !value.ends_with('\n') {
            out.push_str("\n\\ No newline at end of file\n");
        }
    };
    for value in &old[start..prefix] {
        line(' ', value);
    }
    for value in &old[prefix..old.len() - suffix] {
        line('-', value);
    }
    for value in &new[prefix..new.len() - suffix] {
        line('+', value);
    }
    for value in &old[old.len() - suffix..old_end] {
        line(' ', value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diff_reports_replacements_and_newline_changes() {
        let diff = text_diff("a", "same\nold\nend\n", "same\nnew\nend\n");
        assert!(diff.contains(" same\n-old\n+new\n end\n"), "{diff}");
        assert!(text_diff("a", "x\n", "x").contains("No newline"));
        assert!(text_diff("a", "", "new\n").contains("@@ -0,0 +1,1 @@"));
    }
}

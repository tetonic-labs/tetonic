//! Unified diff generation for transaction preview.

use tetonic_domain::{StagedOperation, StagedOperationKind, WorkspacePath};

pub fn unified_diff(root_display: &str, ops: &[StagedOperation]) -> String {
    let mut out = String::new();
    for op in ops {
        match op.kind {
            StagedOperationKind::Create
            | StagedOperationKind::Replace
            | StagedOperationKind::Edit => {
                if let Some(cp) = &op.new_content_path {
                    let content = std::fs::read_to_string(cp).unwrap_or_default();
                    out.push_str(&format!(
                        "--- /dev/null\n+++ b/{}/{}\n",
                        root_display, op.path.0
                    ));
                    for line in content.lines() {
                        out.push('+');
                        out.push_str(line);
                        out.push('\n');
                    }
                }
            }
            StagedOperationKind::Delete => {
                out.push_str(&format!(
                    "--- a/{}/{}\n+++ /dev/null\n",
                    root_display, op.path.0
                ));
                out.push_str(&format!("-(deleted {})\n", op.path.0));
            }
            StagedOperationKind::Rename => {
                if let Some(to) = &op.destination {
                    out.push_str(&format!("rename from {}\nrename to {}\n", op.path.0, to.0));
                }
            }
            StagedOperationKind::ModeChange => {
                out.push_str(&format!("mode change {}\n", op.path.0));
            }
        }
    }
    out
}

pub fn classify_paths(
    ops: &[StagedOperation],
) -> (Vec<WorkspacePath>, Vec<WorkspacePath>, Vec<WorkspacePath>) {
    let mut created = Vec::new();
    let mut deleted = Vec::new();
    let mut modified = Vec::new();
    for op in ops {
        match op.kind {
            StagedOperationKind::Create => created.push(op.path.clone()),
            StagedOperationKind::Delete => deleted.push(op.path.clone()),
            StagedOperationKind::Rename => {
                deleted.push(op.path.clone());
                if let Some(d) = &op.destination {
                    created.push(d.clone());
                }
            }
            _ => modified.push(op.path.clone()),
        }
    }
    (created, deleted, modified)
}

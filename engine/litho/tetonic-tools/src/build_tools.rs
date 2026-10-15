//! Small workspace tools composed from the existing readers and transaction owner.
use crate::{InspectChangesArgs, ReadFilesArgs, ToolError, ToolOutcome, Tools};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tetonic_domain::work_scope::CancellationSignal;

impl Tools {
    pub(crate) fn read_files(
        &self,
        args: Value,
        cancel: Option<&CancellationSignal>,
    ) -> Result<ToolOutcome, ToolError> {
        let mut args: ReadFilesArgs = Self::parse(args)?;
        let count = args.files.len();
        let budget = args.max_bytes.unwrap_or(2048).min(4096);
        if !(1..=4).contains(&count) || budget < count * 4 {
            return Err(ToolError::BadArgs(
                "Use one to four files and at least four bytes per file.".into(),
            ));
        }
        let mut results = Vec::new();
        let mut complete = true;
        let mut ok = true;
        for file in &mut args.files {
            if cancel.is_some_and(CancellationSignal::is_canceled) {
                return Err(ToolError::Other("batch read canceled".into()));
            }
            // Share the source budget, rather than multiplying it per file. Each
            // bounded reader retains its own truthful continuation and hash.
            file.max_bytes = Some(file.max_bytes.unwrap_or(budget / count).min(budget / count));
            let result = self
                .read_file(
                    serde_json::to_value(&file).map_err(|e| ToolError::BadArgs(e.to_string()))?,
                )
                .unwrap_or_else(crate::types::outcome_err);
            complete &= result.ok && result.content_complete;
            ok &= result.ok;
            results.push(format!(
                "[FILE {} — {}]\n{}",
                serde_json::to_string(&file.path).unwrap_or_default(),
                if !result.ok {
                    "failed"
                } else if result.content_complete {
                    "requested range complete"
                } else {
                    "partial"
                },
                result.content
            ));
        }
        Ok(ToolOutcome {
            ok,
            content_complete: complete,
            summary: format!(
                "Read {count} requested files/ranges{}",
                if ok { "" } else { " with errors" }
            ),
            content: results.join("\n\n"),
            error_kind: (!ok).then(|| "partial_read_failure".into()),
            change: None,
        })
    }

    pub(crate) fn inspect_changes(&self, args: Value) -> Result<ToolOutcome, ToolError> {
        let args: InspectChangesArgs = Self::parse(args)?;
        let text = self.mutation.transaction_service().with_active_if_any(|txn| txn.inspection_report())
            .map_err(|e| ToolError::Other(e.to_string()))?
            .unwrap_or_else(|| "No pending file-tool changes for this agent. This does not inspect Git or changes made by shell commands.\nVerification: no pending patch to check.\n".into());
        let digest = format!("{:x}", Sha256::digest(text.as_bytes()));
        if args
            .expected_sha256
            .as_ref()
            .is_some_and(|expected| expected != &digest)
        {
            return Err(ToolError::Other("Changes or verification changed since the previous page. Inspect again from the beginning.".into()));
        }
        let start = args.offset_bytes.unwrap_or(0);
        let limit = args.max_bytes.unwrap_or(2048).min(4096);
        if limit < 4 || start > text.len() || !text.is_char_boundary(start) {
            return Err(ToolError::BadArgs(
                "Use max_bytes >= 4 and a valid report byte offset.".into(),
            ));
        }
        let mut end = start.saturating_add(limit).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let complete = end == text.len();
        let next = if complete {
            "none".into()
        } else {
            json!({"offset_bytes":end,"max_bytes":limit,"expected_sha256":digest}).to_string()
        };
        let mut result = ToolOutcome::ok("Inspect pending changes", format!(
            "Report bytes {start}..{end}/{}. SHA-256: {digest}\nNext inspect_changes: {next}\n--- report ---\n{}\n--- end page ---",
            text.len(), &text[start..end]));
        result.content_complete = complete;
        Ok(result)
    }
}

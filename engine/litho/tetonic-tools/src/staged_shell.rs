//! Explicit staged checks through the ordinary governed shell capability.
use crate::{types::outcome_err, RunShellArgs, ShellView, ToolError, ToolOutcome, Tools};
use tetonic_domain::{
    execution::CanonicalActionParameters, work_scope::CancellationSignal, AuthorizedAction,
};
use tetonic_transaction::{StagedCheckBinding, StagedCheckResult};

/// Validate the host binding at the approval boundary without accepting an
/// arbitrary path under the workspace or trusting a model-provided directory.
pub fn staged_shell_directory(
    root: &std::path::Path,
    params: &CanonicalActionParameters,
) -> Option<std::path::PathBuf> {
    if params.tool_name.as_deref() != Some("run_shell") {
        return None;
    }
    let args: RunShellArgs = serde_json::from_value(params.tool_arguments.clone()?).ok()?;
    if args.view != ShellView::Staged {
        return None;
    }
    let binding: StagedCheckBinding =
        serde_json::from_str(params.filesystem_access_scope.as_deref()?).ok()?;
    let cwd = binding.working_directory(root).ok()?;
    (params.working_directory.as_deref() == Some(cwd.to_string_lossy().as_ref())).then_some(cwd)
}

impl Tools {
    pub(crate) fn prepare_shell_view(
        &self,
        name: &str,
        mut params: CanonicalActionParameters,
    ) -> Result<CanonicalActionParameters, String> {
        if name != "run_shell" {
            return Ok(params);
        }
        let args: RunShellArgs =
            serde_json::from_value(params.tool_arguments.clone().unwrap_or_default())
                .map_err(|e| e.to_string())?;
        self.validate_shell_command(&args.command)
            .map_err(|e| e.to_string())?;
        if args.view == ShellView::Staged {
            let binding = self
                .mutation
                .transaction_service()
                .staged_check_binding()
                .map_err(|e| e.to_string())?;
            let cwd = binding
                .working_directory(self.ws.root())
                .map_err(|e| e.to_string())?;
            if crate::command_escapes_workspace(&cwd, &args.command) {
                return Err("Use relative paths for a staged check so it tests pending edits, not the original folder.".into());
            }
            params.working_directory = Some(cwd.display().to_string());
            params.filesystem_access_scope =
                Some(serde_json::to_string(&binding).map_err(|e| e.to_string())?);
        }
        Ok(params)
    }

    pub(crate) fn run_authorized_shell(
        &self,
        args: &serde_json::Value,
        authorized: &AuthorizedAction,
        cancel: Option<&CancellationSignal>,
    ) -> ToolOutcome {
        let run = || -> Result<ToolOutcome, ToolError> {
            let params = &authorized.action.parameters;
            let a: RunShellArgs = Self::parse(args.clone())?;
            self.validate_shell_command(&a.command)?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            tetonic_domain::validate_authorized_action(authorized, now)
                .map_err(|e| ToolError::Other(format!("shell capability denied: {e}")))?;
            if authorized.action.kind != tetonic_domain::ActionKind::ExecuteShell
                || params.tool_name.as_deref() != Some("run_shell")
                || params.tool_arguments.as_ref() != Some(args)
                || params.script_bytes.as_deref() != Some(a.command.as_bytes())
            {
                return Err(ToolError::Other(
                    "shell capability does not match this invocation".into(),
                ));
            }
            if a.view == ShellView::Workspace {
                if params.working_directory.as_deref()
                    != Some(self.ws.root().to_string_lossy().as_ref())
                {
                    return Err(ToolError::Other(
                        "shell working folder does not match its approved view".into(),
                    ));
                }
                let mut result = crate::tool_outcome_from_execution(
                    self.executor
                        .run_process_sync_cancellable(authorized, cancel),
                );
                if self.staged_transaction_preview()?.is_some() {
                    result.content.insert_str(0, "Ran on committed workspace files. Pending file-tool edits were NOT checked; use view=staged to test them.\n");
                }
                return Ok(result);
            }
            let cwd = staged_shell_directory(self.ws.root(), params).ok_or_else(|| {
                ToolError::Other("staged check binding is missing or invalid".into())
            })?;
            if crate::command_escapes_workspace(&cwd, &a.command) {
                return Err(ToolError::Other(
                    "staged checks require relative workspace paths".into(),
                ));
            }
            let binding: StagedCheckBinding = serde_json::from_str(
                params
                    .filesystem_access_scope
                    .as_deref()
                    .unwrap_or_default(),
            )
            .map_err(|e| ToolError::Other(e.to_string()))?;
            let uncanceled = CancellationSignal::from_flag(std::sync::Arc::new(
                std::sync::atomic::AtomicBool::new(false),
            ));
            let cancel = cancel.unwrap_or(&uncanceled);
            let result = self
                .mutation
                .transaction_service()
                .run_staged_check(&binding, &a.command, cancel, |_| {
                    // Exact issued cwd and original consumer/backend; no post-approval
                    // parameter changes, second execution path or weaker sandbox.
                    let outcome = crate::tool_outcome_from_execution(
                        self.executor
                            .run_process_sync_cancellable(authorized, Some(cancel)),
                    );
                    StagedCheckResult {
                        success: outcome.ok,
                        output: outcome.content,
                    }
                })
                .map_err(|e| ToolError::Other(e.to_string()))?;
            Ok(ToolOutcome {
                content_complete: true, ok: result.success,
                summary: if result.success { "Pending edits passed the requested check" } else { "Pending edits did not pass the requested check" }.into(),
                content: format!("{}\nChecked pending edits in a disposable workspace copy (patch {}). No edits or command-generated files were published. Build caches/dependencies and protected folders are not copied.\n{}",
                    if result.success { "Check passed for this patch." } else { "Check failed. Repair the pending files or the check, then run view=staged again before finishing." },
                    binding.patch_digest.0, result.output),
                error_kind: (!result.success).then(|| "verification_failed".into()), change: None,
            })
        };
        run().unwrap_or_else(outcome_err)
    }
}

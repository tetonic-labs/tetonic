//! Coding process executor and validator (AC2-3 / C1).

use crate::{exec, RunShellArgs, ToolError, ToolOutcome};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use lokai_domain::execution::AuthorizedAction;
pub use lokai_sandbox::process_executor::{
    shell_command, EnforcementLevel, NonCodingProcessValidator, ProcessExecutor,
    ProcessResourceValidator, ProcessRunResult,
};

/// Coding process validator: enforces live workspace version and validates permissible staged/workspace cwd.
#[derive(Debug, Clone)]
pub struct CodingProcessValidator {
    pub workspace_root: PathBuf,
    pub overlay_dir: Option<PathBuf>,
}

impl CodingProcessValidator {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            overlay_dir: None,
        }
    }

    pub fn with_overlay(mut self, overlay: impl Into<PathBuf>) -> Self {
        self.overlay_dir = Some(overlay.into());
        self
    }
}

impl ProcessResourceValidator for CodingProcessValidator {
    fn validate(
        &self,
        action: &AuthorizedAction,
        executor_cwd: &Path,
    ) -> Result<(), lokai_domain::CapabilityError> {
        crate::mutation::enforce_live_workspace_version(action, &self.workspace_root)?;
        if let Some(ref cwd_str) = action.action.parameters.working_directory {
            let cwd = Path::new(cwd_str);
            let valid = cwd == self.workspace_root
                || cwd == executor_cwd
                || self
                    .overlay_dir
                    .as_ref()
                    .is_some_and(|o| cwd == o || cwd.starts_with(o))
                || cwd.starts_with(&self.workspace_root);
            if !valid {
                return Err(lokai_domain::CapabilityError::ScopeMismatch);
            }
        }
        Ok(())
    }
}

/// Convenience constructor for coding process executor.
pub fn coding_executor(workspace: impl Into<PathBuf>, level: EnforcementLevel) -> ProcessExecutor {
    let ws = workspace.into();
    let validator = Arc::new(CodingProcessValidator::new(&ws));
    ProcessExecutor::new(ws, level, validator)
}

pub(crate) fn run_py_compile(workspace: &Path, rel_path: &str) -> Result<(), String> {
    let python = if cfg!(windows) { "python" } else { "python3" };
    let pe = coding_executor(workspace, EnforcementLevel::Sandboxed);
    match pe.sandbox_run_direct(
        python,
        &[
            "-m".to_string(),
            "py_compile".to_string(),
            rel_path.to_string(),
        ],
        lokai_domain::execution::ProcessClass::BuildVerification,
        None,
        lokai_sandbox::exec::DEFAULT_VERIFY_TIMEOUT,
        None,
    ) {
        Ok((true, _, _)) => Ok(()),
        Ok((false, output, _)) => {
            let msg = output
                .trim()
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("py_compile failed")
                .to_string();
            Err(msg)
        }
        Err(e) => Err(format!("py_compile could not run: {e}")),
    }
}

// Named shell dispatch stays in the coding tool adapter; OS ownership stays in sandbox.
impl crate::Tools {
    pub(crate) fn run_shell(
        &self,
        args: Value,
        cancel: Option<&lokai_domain::work_scope::CancellationSignal>,
    ) -> Result<ToolOutcome, ToolError> {
        let a: RunShellArgs = Self::parse(args)?;
        if !self.allow_shell {
            return Err(ToolError::ShellNotApproved);
        }
        let r = self
            .executor
            .run_shell_with_signal(&a.command, exec::DEFAULT_SHELL_TIMEOUT, cancel)
            .map_err(ToolError::Io)?;
        let cmd = exec::clip_shell_command(&a.command);
        let mut summary = if r.success {
            format!("`{cmd}` ok")
        } else {
            format!("`{cmd}` failed")
        };
        if let Some(audit) = &r.sandbox_audit {
            summary = format!("{summary}; {audit}");
        }
        Ok(ToolOutcome {
            ok: r.success,
            summary,
            content: r.output,
            error_kind: if r.success {
                None
            } else {
                Some("nonzero_exit".to_string())
            },
            change: None,
        })
    }
}

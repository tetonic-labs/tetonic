//! Canonical subprocess sink — Sandboxed production tier; Constrained for tests (R6-2 / M2-3).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use tetonic_domain::{ActionKind, AuthorizedAction, ExecutionId, ExecutionOutcome};

use crate::backend::{platform_backend, SandboxBackend};
use crate::exec::{self, EnvMode};
use crate::sandbox_bridge::{
    format_sandbox_audit, process_class_for_git, process_class_for_shell, process_class_for_verify,
    sandbox_request_from_authorized,
};
use crate::types::SandboxedProcess;

/// R4-3: redact secrets from process stdout/stderr before they reach tool summaries.
fn redact_process_io(text: &str) -> String {
    tetonic_secrets::redact_text_sync_lossy(tetonic_secrets::shared_scanner(), text)
}

/// Resolve `name` (or `name.exe` on Windows) on `PATH` for sandbox CreateProcess.
fn resolve_executable_on_path(name: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate.display().to_string());
        }
        #[cfg(windows)]
        {
            let exe = dir.join(format!("{name}.exe"));
            if exe.is_file() {
                return Some(exe.display().to_string());
            }
        }
    }
    None
}

/// Trait for validating process execution resources (working directory, workspace version, etc.).
pub trait ProcessResourceValidator: Send + Sync {
    fn validate(
        &self,
        action: &AuthorizedAction,
        executor_cwd: &Path,
    ) -> Result<(), tetonic_domain::CapabilityError>;
}

/// Non-coding process validator: accepts only actions without workspace-version claims.
/// The action's canonical working directory must match the executor's configured cwd.
#[derive(Debug, Clone, Default)]
pub struct NonCodingProcessValidator;

impl ProcessResourceValidator for NonCodingProcessValidator {
    fn validate(
        &self,
        action: &AuthorizedAction,
        executor_cwd: &Path,
    ) -> Result<(), tetonic_domain::CapabilityError> {
        // Non-coding actions must not claim a workspace version
        if action.capability.workspace_version.is_some() {
            return Err(tetonic_domain::CapabilityError::ScopeMismatch);
        }
        let Some(ref action_cwd_str) = action.action.parameters.working_directory else {
            return Err(tetonic_domain::CapabilityError::ScopeMismatch);
        };
        let action_cwd = Path::new(action_cwd_str);
        if !action_cwd.is_absolute() {
            return Err(tetonic_domain::CapabilityError::ScopeMismatch);
        }
        let matches = match (
            std::fs::canonicalize(action_cwd),
            std::fs::canonicalize(executor_cwd),
        ) {
            (Ok(action), Ok(executor)) => action == executor,
            _ => false,
        };
        if !matches {
            return Err(tetonic_domain::CapabilityError::ScopeMismatch);
        }
        Ok(())
    }
}

/// Process enforcement tier (see execution-contract-v1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EnforcementLevel {
    /// Declared intent + audit only; inherits full environment (no env scrub).
    Advisory,
    /// argv/workdir, env allowlist, timeouts, output limits, kill on expiry.
    /// Test/legacy only — production assembly must use [`Sandboxed`] (R6-2).
    Constrained,
    /// OS sandbox — Job Objects / namespaces / Seatbelt (M2-3 / H1-3 / R6-2).
    #[default]
    Sandboxed,
}

/// Result of a bounded process run.
#[derive(Debug, Clone)]
pub struct ProcessRunResult {
    pub success: bool,
    pub output: String,
    pub execution_id: ExecutionId,
    /// Sandbox enforcement summary for audit when OS sandbox is active.
    pub sandbox_audit: Option<String>,
}

/// Mandatory subprocess executor for verify, shell, and git paths.
#[derive(Clone)]
pub struct ProcessExecutor {
    workspace: PathBuf,
    level: EnforcementLevel,
    validator: Arc<dyn ProcessResourceValidator>,
    consumer: Option<Arc<dyn tetonic_domain::CapabilityConsumer>>,
    sandbox: Arc<dyn SandboxBackend>,
}

impl ProcessExecutor {
    pub fn new(
        workspace: impl Into<PathBuf>,
        level: EnforcementLevel,
        validator: Arc<dyn ProcessResourceValidator>,
    ) -> Self {
        Self {
            workspace: workspace.into(),
            level,
            validator,
            consumer: None,
            sandbox: platform_backend(),
        }
    }

    pub fn with_sandbox_backend(mut self, sandbox: Arc<dyn SandboxBackend>) -> Self {
        self.sandbox = sandbox;
        self
    }

    pub fn sandbox_backend(&self) -> &Arc<dyn SandboxBackend> {
        &self.sandbox
    }

    pub fn with_capability_consumer(
        mut self,
        consumer: Arc<dyn tetonic_domain::CapabilityConsumer>,
    ) -> Self {
        self.consumer = Some(consumer);
        self
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub fn level(&self) -> EnforcementLevel {
        self.level
    }

    pub fn with_level(mut self, level: EnforcementLevel) -> Self {
        self.level = level;
        if self.uses_os_sandbox() {
            self.log_sandbox_capabilities();
        }
        self
    }

    pub fn validator(&self) -> &Arc<dyn ProcessResourceValidator> {
        &self.validator
    }

    pub fn with_validator(mut self, validator: Arc<dyn ProcessResourceValidator>) -> Self {
        self.validator = validator;
        self
    }

    fn log_sandbox_capabilities(&self) {
        let caps = self.sandbox.capabilities();
        tracing::info!(
            "tetonic_sandbox: capabilities platform={} report={}",
            caps.platform,
            serde_json::json!({
                "capabilities": caps,
                "mechanisms": caps.mechanisms,
                "supported_process_classes": caps.supported_process_classes,
            })
        );
    }

    fn validate_capability(
        &self,
        auth: &tetonic_domain::execution::AuthorizedAction,
    ) -> Result<(), tetonic_domain::sinks::ProcessBrokerError> {
        // Reject missing, empty, or relative canonical cwd before consumer authorization
        let cwd_str = auth
            .action
            .parameters
            .working_directory
            .as_deref()
            .unwrap_or("");
        if cwd_str.is_empty() {
            return Err(tetonic_domain::sinks::ProcessBrokerError::Capability(
                tetonic_domain::CapabilityError::ScopeMismatch,
            ));
        }
        let cwd_path = Path::new(cwd_str);
        if !cwd_path.is_absolute() {
            return Err(tetonic_domain::sinks::ProcessBrokerError::Capability(
                tetonic_domain::CapabilityError::ScopeMismatch,
            ));
        }

        // Validate resources via injected validator
        self.validator
            .validate(auth, &self.workspace)
            .map_err(tetonic_domain::sinks::ProcessBrokerError::Capability)?;

        // Validate authorization token
        if let Some(consumer) = &self.consumer {
            consumer
                .authorize(auth)
                .map_err(tetonic_domain::sinks::ProcessBrokerError::Capability)
        } else {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            tetonic_domain::validate_authorized_action(auth, now)
                .map_err(tetonic_domain::sinks::ProcessBrokerError::Capability)
        }
    }

    pub fn ensure_runnable(&self) -> Result<(), String> {
        Ok(())
    }

    pub fn uses_os_sandbox(&self) -> bool {
        self.level == EnforcementLevel::Sandboxed
    }

    pub fn enforcement_level(&self) -> EnforcementLevel {
        self.level
    }

    /// Host-supplied verify command (argv-only, no shell).
    pub fn run_verify(&self, command: &str) -> ProcessRunResult {
        let execution_id = ExecutionId::new(format!("exec_verify_{}", new_exec_suffix()));
        match self.run_verify_inner(command, None) {
            Ok((success, output, sandbox_audit)) => ProcessRunResult {
                success,
                output,
                execution_id,
                sandbox_audit,
            },
            Err(reason) => ProcessRunResult {
                success: false,
                output: reason,
                execution_id,
                sandbox_audit: None,
            },
        }
    }

    pub fn run_verify_with_cancel(
        &self,
        command: &str,
        cancel: Option<&Arc<AtomicBool>>,
    ) -> ProcessRunResult {
        let signal = cancel
            .map(|flag| tetonic_domain::work_scope::CancellationSignal::from_flag(flag.clone()));
        self.run_verify_with_signal(command, signal.as_ref())
    }

    pub fn run_verify_with_signal(
        &self,
        command: &str,
        cancel: Option<&tetonic_domain::work_scope::CancellationSignal>,
    ) -> ProcessRunResult {
        let execution_id = ExecutionId::new(format!("exec_verify_{}", new_exec_suffix()));
        match self.run_verify_inner(command, cancel) {
            Ok((success, output, sandbox_audit)) => ProcessRunResult {
                success,
                output,
                execution_id,
                sandbox_audit,
            },
            Err(reason) => ProcessRunResult {
                success: false,
                output: reason,
                execution_id,
                sandbox_audit: None,
            },
        }
    }

    pub fn run_verify_with_timeout(&self, command: &str, timeout: Duration) -> ProcessRunResult {
        let execution_id = ExecutionId::new(format!("exec_verify_{}", new_exec_suffix()));
        match self.run_verify_inner_with_timeout(command, timeout, None) {
            Ok((success, output, sandbox_audit)) => ProcessRunResult {
                success,
                output,
                execution_id,
                sandbox_audit,
            },
            Err(reason) => ProcessRunResult {
                success: false,
                output: reason,
                execution_id,
                sandbox_audit: None,
            },
        }
    }

    fn run_verify_inner(
        &self,
        command: &str,
        cancel: Option<&tetonic_domain::work_scope::CancellationSignal>,
    ) -> Result<(bool, String, Option<String>), String> {
        self.run_verify_inner_with_timeout(command, exec::DEFAULT_VERIFY_TIMEOUT, cancel)
    }

    fn run_verify_inner_with_timeout(
        &self,
        command: &str,
        timeout: Duration,
        cancel: Option<&tetonic_domain::work_scope::CancellationSignal>,
    ) -> Result<(bool, String, Option<String>), String> {
        self.ensure_runnable()?;
        let parsed = exec::split_verify_command(command, &self.workspace)?;
        let (program, args) = parsed;
        if self.uses_os_sandbox() {
            return self.sandbox_run_direct_with_signal(
                &program,
                &args,
                process_class_for_verify(),
                None,
                timeout,
                cancel,
            );
        }
        let mut cmd = std::process::Command::new(program);
        cmd.args(args);
        cmd.current_dir(&self.workspace);
        let (success, output) = self.run_command_with_signal(&mut cmd, timeout, cancel)?;
        Ok((success, output, None))
    }

    /// Model-driven shell (still workspace-scoped; approval is upstream).
    pub fn run_shell(&self, command: &str) -> Result<ProcessRunResult, String> {
        self.run_shell_with_timeout_and_cancel(command, exec::DEFAULT_SHELL_TIMEOUT, None)
    }

    pub fn run_shell_with_timeout_and_cancel(
        &self,
        command: &str,
        timeout: Duration,
        cancel: Option<&Arc<AtomicBool>>,
    ) -> Result<ProcessRunResult, String> {
        let signal = cancel
            .map(|flag| tetonic_domain::work_scope::CancellationSignal::from_flag(flag.clone()));
        self.run_shell_with_signal(command, timeout, signal.as_ref())
    }

    pub fn run_shell_with_signal(
        &self,
        command: &str,
        timeout: Duration,
        cancel: Option<&tetonic_domain::work_scope::CancellationSignal>,
    ) -> Result<ProcessRunResult, String> {
        self.ensure_runnable()?;
        let execution_id = ExecutionId::new(format!("exec_shell_{}", new_exec_suffix()));
        let (success, output, sandbox_audit) = if self.uses_os_sandbox() {
            self.sandbox_run_direct_with_signal(
                "",
                &[],
                process_class_for_shell(),
                Some(command),
                timeout,
                cancel,
            )?
        } else {
            let mut cmd = shell_command(command);
            cmd.current_dir(&self.workspace);
            let (success, output) = self.run_command_with_signal(&mut cmd, timeout, cancel)?;
            (success, output, None)
        };
        Ok(ProcessRunResult {
            success,
            output,
            execution_id,
            sandbox_audit,
        })
    }

    /// Git subprocess in workspace root (worktree management).
    /// Always goes through the OS sandbox profile (R09) — no raw git process spawn.
    pub fn run_git<I, S>(&self, args: I) -> Result<ProcessRunResult, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.ensure_runnable()?;
        let execution_id = ExecutionId::new(format!("exec_git_{}", new_exec_suffix()));
        let arg_vec: Vec<String> = args.into_iter().map(|s| s.as_ref().to_string()).collect();
        let git =
            resolve_executable_on_path("git").ok_or_else(|| "git not found on PATH".to_string())?;
        let (success, output, sandbox_audit) = self.sandbox_run_direct(
            &git,
            &arg_vec,
            process_class_for_git(),
            None,
            exec::DEFAULT_SHELL_TIMEOUT,
            None,
        )?;
        Ok(ProcessRunResult {
            success,
            output,
            execution_id,
            sandbox_audit,
        })
    }

    /// Git helper returning success bool (R09: delegates to sandboxed [`run_git`]).
    pub fn run_git_status(&self, args: &[&str]) -> Result<bool, String> {
        Ok(self.run_git(args.iter().copied())?.success)
    }

    fn env_mode(&self) -> EnvMode {
        match self.level {
            EnforcementLevel::Constrained | EnforcementLevel::Sandboxed => EnvMode::Minimal,
            EnforcementLevel::Advisory => EnvMode::Inherited,
        }
    }

    fn run_command_with_signal(
        &self,
        cmd: &mut std::process::Command,
        timeout: Duration,
        cancel: Option<&tetonic_domain::work_scope::CancellationSignal>,
    ) -> Result<(bool, String), String> {
        let output = exec::command_output_with_signal(cmd, timeout, cancel, self.env_mode())?;
        let mut buf = String::new();
        let stdout = redact_process_io(&String::from_utf8_lossy(&output.stdout));
        let stderr = redact_process_io(&String::from_utf8_lossy(&output.stderr));
        if !stdout.is_empty() {
            buf.push_str("stdout:\n");
            buf.push_str(&stdout);
        }
        if !stderr.is_empty() {
            buf.push_str("\nstderr:\n");
            buf.push_str(&stderr);
        }
        let code = output.status.code().unwrap_or(-1);
        Ok((
            output.status.success(),
            crate::exec::truncate_output(&format!("exit code: {code}\n{buf}")),
        ))
    }

    /// Record the start of a long-lived internal service (e.g. LSP) through the
    /// broker policy path; actual spawn uses the sandbox long-lived interface.
    pub fn note_lsp_service_start(&self, program: &str) -> Result<(), String> {
        self.ensure_runnable()?;
        let execution_id = ExecutionId::new(format!("lsp_{}", new_exec_suffix()));
        if self.uses_os_sandbox() {
            use crate::profiles::profile_for_class;
            use crate::types::ProcessMode;
            use tetonic_domain::execution::ProcessClass;
            let profile = profile_for_class(ProcessClass::InternalService, &self.workspace);
            let caps = self.sandbox.capabilities();
            tracing::info!(
                "tetonic_sandbox: lsp_service_start execution_id={} program={} mode={:?} caps={} audit={}",
                execution_id,
                program,
                ProcessMode::LongLived,
                caps.platform,
                serde_json::json!({
                    "profile": profile.process_class,
                    "capabilities": caps,
                    "workspace": self.workspace.display().to_string(),
                })
            );
        } else {
            tracing::info!(
                "lokai_tools::broker: lsp_service_start execution_id={} program={} workspace={} policy=Allow",
                execution_id,
                program,
                self.workspace.display()
            );
        }
        Ok(())
    }

    pub fn sandbox_run_direct(
        &self,
        executable: &str,
        arguments: &[String],
        process_class: tetonic_domain::execution::ProcessClass,
        shell_script: Option<&str>,
        runtime_limit: Duration,
        cancel: Option<&Arc<AtomicBool>>,
    ) -> Result<(bool, String, Option<String>), String> {
        let signal = cancel
            .map(|flag| tetonic_domain::work_scope::CancellationSignal::from_flag(flag.clone()));
        self.sandbox_run_direct_with_signal(
            executable,
            arguments,
            process_class,
            shell_script,
            runtime_limit,
            signal.as_ref(),
        )
    }

    fn sandbox_run_direct_with_signal(
        &self,
        executable: &str,
        arguments: &[String],
        process_class: tetonic_domain::execution::ProcessClass,
        shell_script: Option<&str>,
        runtime_limit: Duration,
        cancel: Option<&tetonic_domain::work_scope::CancellationSignal>,
    ) -> Result<(bool, String, Option<String>), String> {
        use crate::profiles::{apply_executable, apply_shell, profile_for_class};
        use crate::types::ProcessMode;

        if cancel.is_some_and(|f| f.is_canceled()) {
            return Err("command canceled".into());
        }

        let mut req = profile_for_class(process_class, &self.workspace);
        req.runtime_limit = runtime_limit;
        req.mode = ProcessMode::OneShot;
        if let Some(script) = shell_script {
            #[cfg(windows)]
            let shell = "cmd";
            #[cfg(not(windows))]
            let shell = "sh";
            req = apply_shell(req, shell, script);
        } else {
            req = apply_executable(req, executable, arguments);
        }

        let backend = self.sandbox.clone();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("sandbox runtime: {e}"))?;
        let signal = cancel.cloned();
        let result = rt.block_on(async move {
            match signal {
                Some(signal) => backend.execute_cancellable(req, signal).await,
                None => backend.execute(req).await,
            }
        });

        match result {
            Ok(SandboxedProcess::Completed(r)) => {
                let audit = format_sandbox_audit(&r.report);
                tracing::info!("tetonic_sandbox: {audit}");
                let body = format!(
                    "{}\nexit code: {}\nstdout:\n{}\nstderr:\n{}",
                    r.report.format_user_visible(),
                    r.exit_code.unwrap_or(-1),
                    redact_process_io(&r.stdout),
                    redact_process_io(&r.stderr)
                );
                Ok((r.success, exec::truncate_output(&body), Some(audit)))
            }
            Ok(SandboxedProcess::Service(_)) => Err("expected one-shot sandbox result".into()),
            Err(crate::SandboxError::Timeout) => Err(format!(
                "command timed out after {}s",
                runtime_limit.as_secs()
            )),
            Err(crate::SandboxError::Canceled) => Err("command canceled".into()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Host cwd for verify/process: canonical cwd with no fallback (C1).
    fn effective_cwd(&self, authorized: &AuthorizedAction) -> PathBuf {
        authorized
            .action
            .parameters
            .working_directory
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| self.workspace.clone())
    }

    fn sandbox_run_authorized(
        &self,
        authorized: &AuthorizedAction,
        runtime_limit: Duration,
        cancel: Option<&tetonic_domain::work_scope::CancellationSignal>,
    ) -> Result<(bool, String, Option<String>), String> {
        let cwd = self.effective_cwd(authorized);
        let req = sandbox_request_from_authorized(authorized, &cwd, runtime_limit)?;
        let backend = self.sandbox.clone();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("sandbox runtime: {e}"))?;
        let signal = cancel.cloned();
        let result = rt.block_on(async move {
            match signal {
                Some(signal) => backend.execute_cancellable(req, signal).await,
                None => backend.execute(req).await,
            }
        });
        match result {
            Ok(SandboxedProcess::Completed(r)) => {
                let audit = format_sandbox_audit(&r.report);
                tracing::info!("tetonic_sandbox: {audit}");
                let body = format!(
                    "{}\nexit code: {}\nstdout:\n{}\nstderr:\n{}",
                    r.report.format_user_visible(),
                    r.exit_code.unwrap_or(-1),
                    redact_process_io(&r.stdout),
                    redact_process_io(&r.stderr)
                );
                Ok((r.success, exec::truncate_output(&body), Some(audit)))
            }
            Ok(SandboxedProcess::Service(_)) => Err("expected one-shot sandbox".into()),
            Err(crate::SandboxError::Timeout) => Err(format!(
                "command timed out after {}s",
                runtime_limit.as_secs()
            )),
            Err(crate::SandboxError::Canceled) => Err("command canceled".into()),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn run_process_sync(&self, authorized: &AuthorizedAction) -> ExecutionOutcome {
        self.run_process_sync_cancellable(authorized, None)
    }

    pub fn run_process_sync_cancellable(
        &self,
        authorized: &AuthorizedAction,
        cancel: Option<&tetonic_domain::work_scope::CancellationSignal>,
    ) -> ExecutionOutcome {
        let execution_id = ExecutionId::new(format!("exec_{}", authorized.action.action_id));
        if let Err(e) = self.validate_capability(authorized) {
            return ExecutionOutcome::Failed {
                execution_id,
                reason: e.to_string(),
            };
        }
        if cancel.is_some_and(|signal| signal.is_canceled()) {
            return ExecutionOutcome::Failed {
                execution_id,
                reason: "command canceled".into(),
            };
        }
        if self.uses_os_sandbox() {
            let limit = match &authorized.action.kind {
                ActionKind::ExecuteShell => exec::DEFAULT_SHELL_TIMEOUT,
                _ => exec::DEFAULT_VERIFY_TIMEOUT,
            };
            return match self.sandbox_run_authorized(authorized, limit, cancel) {
                Ok((true, summary, audit)) => ExecutionOutcome::Completed {
                    execution_id,
                    ok: true,
                    summary: prepend_sandbox_audit(summary, audit),
                },
                Ok((false, summary, audit)) => ExecutionOutcome::Completed {
                    execution_id,
                    ok: false,
                    summary: prepend_sandbox_audit(summary, audit),
                },
                Err(reason) => ExecutionOutcome::Failed {
                    execution_id,
                    reason,
                },
            };
        }
        let result = match &authorized.action.kind {
            ActionKind::ExecuteProcess => {
                let cwd = self.effective_cwd(authorized);
                if let Some(tetonic_domain::execution::ProcessClass::BuildVerification) =
                    authorized.action.parameters.process_class
                {
                    let program = authorized
                        .action
                        .parameters
                        .executable_identity
                        .clone()
                        .unwrap_or_default();
                    if program.is_empty() {
                        return ExecutionOutcome::Failed {
                            execution_id,
                            reason: "empty executable identity".into(),
                        };
                    }
                    let mut cmd = std::process::Command::new(program);
                    cmd.args(&authorized.action.parameters.arguments);
                    cmd.current_dir(&cwd);
                    self.run_command_with_signal(&mut cmd, exec::DEFAULT_VERIFY_TIMEOUT, cancel)
                } else {
                    let program = authorized
                        .action
                        .parameters
                        .executable_identity
                        .clone()
                        .unwrap_or_default();
                    if program.is_empty() {
                        return ExecutionOutcome::Failed {
                            execution_id,
                            reason: "empty executable identity".into(),
                        };
                    }
                    let mut cmd = std::process::Command::new(program);
                    cmd.args(&authorized.action.parameters.arguments);
                    cmd.current_dir(&cwd);
                    self.run_command_with_signal(&mut cmd, exec::DEFAULT_SHELL_TIMEOUT, cancel)
                }
            }
            ActionKind::ExecuteShell => {
                let command = authorized
                    .action
                    .parameters
                    .script_bytes
                    .as_ref()
                    .and_then(|b| std::str::from_utf8(b).ok())
                    .unwrap_or("");
                if command.is_empty() {
                    return ExecutionOutcome::Failed {
                        execution_id,
                        reason: "empty shell command".into(),
                    };
                }
                let mut cmd = shell_command(command);
                cmd.current_dir(self.effective_cwd(authorized));
                self.run_command_with_signal(&mut cmd, exec::DEFAULT_SHELL_TIMEOUT, cancel)
            }
            _ => {
                return ExecutionOutcome::Failed {
                    execution_id,
                    reason: format!(
                        "ProcessExecutor cannot run action kind {:?}",
                        authorized.action.kind
                    ),
                };
            }
        };
        match result {
            Ok((true, summary)) => ExecutionOutcome::Completed {
                execution_id,
                ok: true,
                summary,
            },
            Ok((false, summary)) => ExecutionOutcome::Completed {
                execution_id,
                ok: false,
                summary,
            },
            Err(reason) => ExecutionOutcome::Failed {
                execution_id,
                reason,
            },
        }
    }
}

#[path = "process_service.rs"]
mod service;

fn new_exec_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn prepend_sandbox_audit(summary: String, audit: Option<String>) -> String {
    match audit {
        Some(line) => format!("{line}\n{summary}"),
        None => summary,
    }
}

/// Build a shell-backed command for the current platform.
#[cfg(windows)]
pub fn shell_command(command: &str) -> Command {
    let mut c = Command::new("cmd");
    c.arg("/C").arg(command);
    c
}

#[cfg(not(windows))]
pub fn shell_command(command: &str) -> Command {
    let mut c = Command::new("sh");
    c.arg("-c").arg(command);
    c
}

//! Linux sandbox backend — namespaces + rlimits (H1-3 network denial).

use std::path::PathBuf;
use std::process::Stdio;

use async_trait::async_trait;
use tokio::process::Command;

use crate::backend::unix_common::{
    apply_memory_limit, collect_oneshot, resolve_command, UnixLongLived,
};
use crate::backend::{
    build_minimal_env, evaluate_outcome, make_report, path_denied, validate_working_directory,
    SandboxBackend,
};
use crate::backend::{linux_fs, linux_net};
use crate::types::{
    FilesystemPolicy, LongLivedInner, LongLivedSandboxHandle, NetworkPolicy, ProcessMode,
    SandboxCapabilities, SandboxControl, SandboxError, SandboxRequest, SandboxRunResult,
    SandboxedProcess,
};

pub struct LinuxSandboxBackend;

impl LinuxSandboxBackend {
    pub fn new() -> Self {
        Self
    }

    fn detect_capabilities() -> SandboxCapabilities {
        let network_denial = linux_net::network_denial_available();
        let fs_confinement = linux_fs::filesystem_confinement_available();
        let mut mechanisms = vec![
            "process groups (killpg)".into(),
            "setrlimit RLIMIT_AS".into(),
            "minimal environment".into(),
            "bounded stdout/stderr".into(),
        ];
        if network_denial {
            mechanisms.push("unprivileged user+network namespace (DenyAll)".into());
        }
        if fs_confinement {
            mechanisms
                .push("Landlock LSM filesystem confinement (workspace/temp allowlist)".into());
        }
        SandboxCapabilities {
            process_tree_containment: true,
            child_breakaway_prevention: false,
            runtime_enforcement: true,
            memory_limits: true,
            cpu_limits: false,
            process_count_limits: false,
            output_limits: true,
            environment_filtering: true,
            filesystem_read_restrictions: fs_confinement,
            filesystem_write_restrictions: fs_confinement,
            network_denial,
            network_allowlisting: false,
            long_lived_process_support: true,
            interactive_stdin_support: true,
            supported_process_classes: vec![
                lokai_domain::execution::ProcessClass::InternalService,
                lokai_domain::execution::ProcessClass::RepositoryTool,
                lokai_domain::execution::ProcessClass::BuildVerification,
                lokai_domain::execution::ProcessClass::ModelRequestedShell,
                lokai_domain::execution::ProcessClass::HardwareProbe,
            ],
            platform: "linux".into(),
            mechanisms,
        }
    }
}

impl Default for LinuxSandboxBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SandboxBackend for LinuxSandboxBackend {
    fn capabilities(&self) -> SandboxCapabilities {
        Self::detect_capabilities()
    }

    async fn execute(&self, request: SandboxRequest) -> Result<SandboxedProcess, SandboxError> {
        self.execute_cancellable(request, Default::default()).await
    }
    async fn execute_cancellable(
        &self,
        request: SandboxRequest,
        cancel: lokai_domain::work_scope::CancellationSignal,
    ) -> Result<SandboxedProcess, SandboxError> {
        if cancel.is_canceled() {
            return Err(SandboxError::Canceled);
        }

        let caps = self.capabilities();
        let wd = validate_working_directory(&request)?;
        if path_denied(&wd, &request.filesystem.denied_paths) {
            return Err(SandboxError::Denied(
                "working directory matches denied path scope".into(),
            ));
        }

        let mut enforced = vec![
            SandboxControl::RuntimeLimit,
            SandboxControl::EnvironmentFiltering,
            SandboxControl::OutputLimit,
            SandboxControl::WorkingDirectoryRestriction,
            SandboxControl::ProcessTreeContainment,
        ];
        if request.resources.max_memory_bytes.is_some() {
            enforced.push(SandboxControl::MemoryLimit);
        }
        if caps.network_denial
            && matches!(
                request.network,
                NetworkPolicy::DenyAll | NetworkPolicy::AllowLoopback
            )
        {
            enforced.push(SandboxControl::NetworkDenial);
        }
        if caps.filesystem_read_restrictions {
            enforced.push(SandboxControl::FilesystemReadRestriction);
        }
        if caps.filesystem_write_restrictions {
            enforced.push(SandboxControl::FilesystemWriteRestriction);
        }

        let missing = crate::backend::collect_missing_controls(&request, &caps);

        let outcome = evaluate_outcome(&caps, &request, enforced.clone(), missing)?;
        let report = make_report(caps, outcome, request.process_class.clone());

        match request.mode {
            ProcessMode::OneShot => {
                let result = run_oneshot(request, wd, report, cancel).await?;
                Ok(SandboxedProcess::Completed(result))
            }
            ProcessMode::LongLived => {
                let handle = spawn_long_lived(request, wd, report).await?;
                Ok(SandboxedProcess::Service(handle))
            }
        }
    }
}

/// One composed child hook: network namespace + Landlock + rlimit.
/// Exposes the FnMut contract required by both std and Tokio pre_exec.
pub(crate) fn linux_child_isolation_hook(
    network: NetworkPolicy,
    filesystem: FilesystemPolicy,
    wd: PathBuf,
    max_memory_bytes: Option<u64>,
) -> impl FnMut() -> std::io::Result<()> + Send + Sync + 'static {
    move || {
        if matches!(
            network,
            NetworkPolicy::DenyAll | NetworkPolicy::AllowLoopback
        ) && linux_net::network_denial_available()
        {
            linux_net::apply_in_child(&network)?;
        }
        if linux_fs::filesystem_confinement_available() {
            linux_fs::apply_in_child(&filesystem, &wd)?;
        }
        if let Some(bytes) = max_memory_bytes {
            apply_memory_limit(bytes)?;
        }
        Ok(())
    }
}

fn attach_child_isolation(cmd: &mut Command, request: &SandboxRequest, wd: &std::path::Path) {
    unsafe {
        cmd.pre_exec(linux_child_isolation_hook(
            request.network.clone(),
            request.filesystem.clone(),
            wd.to_path_buf(),
            request.resources.max_memory_bytes,
        ));
    }
}

async fn run_oneshot(
    request: SandboxRequest,
    wd: PathBuf,
    report: crate::types::SandboxRunReport,
    cancel: lokai_domain::work_scope::CancellationSignal,
) -> Result<SandboxRunResult, SandboxError> {
    if cancel.is_canceled() {
        return Err(SandboxError::Canceled);
    }
    let (exe, args) = resolve_command(&request)?;
    let env = build_minimal_env(&request.environment);
    let mut cmd = Command::new(&exe);
    cmd.args(&args)
        .current_dir(&wd)
        .process_group(0)
        .kill_on_drop(true);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.env_clear();
    for (k, v) in env {
        cmd.env(k, v);
    }
    attach_child_isolation(&mut cmd, &request, &wd);

    let child = cmd
        .spawn()
        .map_err(|e| SandboxError::SpawnFailed(e.to_string()))?;
    collect_oneshot(child, request, report, cancel).await
}

async fn spawn_long_lived(
    request: SandboxRequest,
    wd: PathBuf,
    report: crate::types::SandboxRunReport,
) -> Result<LongLivedSandboxHandle, SandboxError> {
    let (exe, args) = resolve_command(&request)?;
    let env = build_minimal_env(&request.environment);
    let mut cmd = Command::new(&exe);
    cmd.args(&args)
        .current_dir(&wd)
        .process_group(0)
        .kill_on_drop(true);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    cmd.env_clear();
    for (k, v) in env {
        cmd.env(k, v);
    }
    attach_child_isolation(&mut cmd, &request, &wd);
    let mut child = cmd
        .spawn()
        .map_err(|e| SandboxError::SpawnFailed(e.to_string()))?;
    let pgid = child.id().unwrap_or(0);
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    Ok(LongLivedSandboxHandle {
        report,
        inner: LongLivedInner::Unix(Box::new(UnixLongLived::new(pgid, child, stdin, stdout))),
    })
}

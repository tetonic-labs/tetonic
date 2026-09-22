//! macOS sandbox backend — Seatbelt network denial (H1-3).

use std::path::PathBuf;
use std::process::Stdio;

use async_trait::async_trait;
use tokio::process::Command;

use crate::backend::unix_common::{
    attach_memory_limit_pre_exec, collect_oneshot, resolve_command, UnixLongLived,
};
use crate::backend::{
    build_minimal_env, evaluate_outcome, make_report, path_denied, validate_working_directory,
    SandboxBackend,
};
use crate::types::{
    LongLivedInner, LongLivedSandboxHandle, NetworkPolicy, ProcessMode, SandboxCapabilities,
    SandboxControl, SandboxError, SandboxRequest, SandboxRunResult, SandboxedProcess,
};

pub struct MacOsSandboxBackend;

impl MacOsSandboxBackend {
    pub fn new() -> Self {
        Self
    }

    fn sandbox_exec_path() -> Option<&'static str> {
        const CANDIDATES: &[&str] = &["/usr/bin/sandbox-exec"];
        CANDIDATES
            .iter()
            .copied()
            .find(|p| PathBuf::from(p).is_file())
    }

    pub fn network_denial_available() -> bool {
        Self::sandbox_exec_path().is_some()
    }

    pub fn filesystem_confinement_available() -> bool {
        Self::sandbox_exec_path().is_some()
    }

    fn detect_capabilities() -> SandboxCapabilities {
        let network_denial = Self::network_denial_available();
        let fs_confinement = Self::filesystem_confinement_available();
        let mut mechanisms = vec![
            "process groups (killpg)".into(),
            "setrlimit RLIMIT_AS".into(),
            "minimal environment".into(),
            "bounded stdout/stderr".into(),
        ];
        if network_denial {
            mechanisms.push("sandbox-exec Seatbelt deny network*".into());
        }
        if fs_confinement {
            mechanisms.push(
                "sandbox-exec Seatbelt workspace file-write* / denied file-read* scoping".into(),
            );
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
                tetonic_domain::execution::ProcessClass::InternalService,
                tetonic_domain::execution::ProcessClass::RepositoryTool,
                tetonic_domain::execution::ProcessClass::BuildVerification,
                tetonic_domain::execution::ProcessClass::ModelRequestedShell,
                tetonic_domain::execution::ProcessClass::HardwareProbe,
            ],
            platform: "macos".into(),
            mechanisms,
        }
    }
}

impl Default for MacOsSandboxBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SandboxBackend for MacOsSandboxBackend {
    fn capabilities(&self) -> SandboxCapabilities {
        Self::detect_capabilities()
    }

    async fn execute(&self, request: SandboxRequest) -> Result<SandboxedProcess, SandboxError> {
        self.execute_cancellable(request, Default::default()).await
    }
    async fn execute_cancellable(
        &self,
        request: SandboxRequest,
        cancel: tetonic_domain::work_scope::CancellationSignal,
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

fn seatbelt_path(path: &std::path::Path) -> Result<String, SandboxError> {
    let text = path
        .to_str()
        .filter(|s| !s.chars().any(char::is_control))
        .ok_or_else(|| {
            SandboxError::Denied("Seatbelt paths must be UTF-8 without control characters".into())
        })?;
    Ok(format!(
        "\"{}\"",
        text.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

fn build_seatbelt_profile(
    request: &SandboxRequest,
    wd: &std::path::Path,
) -> Result<String, SandboxError> {
    let mut s = String::from("(version 1)\n(allow default)\n");
    match request.network {
        NetworkPolicy::DenyAll => {
            s.push_str("(deny network*)\n");
        }
        NetworkPolicy::AllowLoopback => {
            s.push_str("(deny network*)\n");
            s.push_str("(allow network* (local ip \"localhost:*\"))\n");
            s.push_str("(allow network* (remote ip \"localhost:*\"))\n");
            s.push_str("(allow network* (local ip \"127.0.0.1:*\"))\n");
            s.push_str("(allow network* (remote ip \"127.0.0.1:*\"))\n");
            s.push_str("(allow network* (local ip \"::1:*\"))\n");
            s.push_str("(allow network* (remote ip \"::1:*\"))\n");
        }
        _ => {}
    }

    // Filesystem write scoping
    s.push_str("(deny file-write*)\n");
    s.push_str(&format!(
        "(allow file-write* (subpath {}))\n",
        seatbelt_path(wd)?
    ));
    for w in &request.filesystem.write_roots {
        s.push_str(&format!(
            "(allow file-write* (subpath {}))\n",
            seatbelt_path(&w.path)?
        ));
    }
    if let Some(tmp) = &request.filesystem.temporary_root {
        s.push_str(&format!(
            "(allow file-write* (subpath {}))\n",
            seatbelt_path(tmp)?
        ));
    }
    s.push_str("(allow file-write* (subpath \"/private/tmp\"))\n");
    s.push_str("(allow file-write* (subpath \"/tmp\"))\n");
    s.push_str("(allow file-write* (subpath \"/dev/fd\"))\n");
    s.push_str("(allow file-write* (literal \"/dev/null\"))\n");
    s.push_str("(allow file-write* (literal \"/dev/zero\"))\n");
    s.push_str("(allow file-write* (literal \"/dev/dtracehelper\"))\n");
    s.push_str("(allow file-write* (literal \"/dev/tty\"))\n");

    // Denied read paths (e.g. ~/.ssh, ~/.aws, ~/.lokai)
    for d in &request.filesystem.denied_paths {
        if d.recursive {
            s.push_str(&format!(
                "(deny file-read* (subpath {}))\n",
                seatbelt_path(&d.path)?
            ));
        } else {
            s.push_str(&format!(
                "(deny file-read* (literal {}))\n",
                seatbelt_path(&d.path)?
            ));
        }
    }

    Ok(s)
}

pub(crate) fn wrap_command(
    request: &SandboxRequest,
    wd: &std::path::Path,
) -> Result<(String, Vec<String>), SandboxError> {
    let (exe, args) = resolve_command(request)?;
    if !MacOsSandboxBackend::filesystem_confinement_available() {
        return Err(SandboxError::Denied(
            "sandbox-exec / Seatbelt not available; refusing unsandboxed spawn".into(),
        ));
    }
    let sandbox = MacOsSandboxBackend::sandbox_exec_path().unwrap();
    let profile = build_seatbelt_profile(request, wd)?;
    let mut wrapped = vec!["-p".into(), profile, exe];
    wrapped.extend(args);
    Ok((sandbox.to_string(), wrapped))
}

async fn run_oneshot(
    request: SandboxRequest,
    wd: PathBuf,
    report: crate::types::SandboxRunReport,
    cancel: tetonic_domain::work_scope::CancellationSignal,
) -> Result<SandboxRunResult, SandboxError> {
    if cancel.is_canceled() {
        return Err(SandboxError::Canceled);
    }
    let (exe, args) = wrap_command(&request, &wd)?;
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
    attach_memory_limit_pre_exec(&mut cmd, request.resources.max_memory_bytes);

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
    let (exe, args) = wrap_command(&request, &wd)?;
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
    attach_memory_limit_pre_exec(&mut cmd, request.resources.max_memory_bytes);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn seatbelt_paths_cannot_break_out_of_string_literals() {
        assert_eq!(
            seatbelt_path(Path::new("/tmp/a\"b\\c")).unwrap(),
            "\"/tmp/a\\\"b\\\\c\""
        );
        assert!(seatbelt_path(Path::new("/tmp/line\nbreak")).is_err());
    }

    #[test]
    fn generated_profile_is_accepted_by_native_seatbelt() {
        let wd = std::env::temp_dir().canonicalize().unwrap();
        let mut req =
            crate::profile_for_class(tetonic_domain::execution::ProcessClass::RepositoryTool, &wd);
        req.filesystem.write_roots.push(crate::types::PathScope {
            path: wd.join("quote\"and\\slash"),
            recursive: true,
        });
        let profile = build_seatbelt_profile(&req, &wd).unwrap();
        let result = std::process::Command::new("/usr/bin/sandbox-exec")
            .args(["-p", &profile, "/usr/bin/true"])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

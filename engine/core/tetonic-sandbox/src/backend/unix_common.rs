//! Shared unix helpers for Linux and macOS backends.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use nix::unistd::Pid;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

use crate::types::{SandboxError, SandboxRequest};
#[cfg(target_os = "macos")]
use tokio::process::Command;

pub(crate) fn resolve_command(
    request: &SandboxRequest,
) -> Result<(String, Vec<String>), SandboxError> {
    if let Some(script) = &request.shell_script {
        return Ok(("/bin/sh".into(), vec!["-c".into(), script.clone()]));
    }
    if request.executable.is_empty() {
        return Err(SandboxError::Denied("empty executable".into()));
    }
    Ok((request.executable.clone(), request.arguments.clone()))
}

pub(crate) fn apply_memory_limit(bytes: u64) -> std::io::Result<()> {
    // rlim_t is narrower on some supported Unix architectures.
    #[allow(clippy::useless_conversion)]
    let limit = bytes.try_into().map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "memory limit exceeds platform range",
        )
    })?;
    let lim = libc::rlimit {
        rlim_cur: limit,
        rlim_max: limit,
    };
    if unsafe { libc::setrlimit(libc::RLIMIT_AS, &lim) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Register rlimit in the child. Must be composed with other unix `pre_exec`
/// hooks by the caller when more than one isolation step is required (CH-4).
#[cfg(target_os = "macos")]
pub(crate) fn attach_memory_limit_pre_exec(cmd: &mut Command, bytes: Option<u64>) {
    let Some(bytes) = bytes else {
        return;
    };
    unsafe {
        cmd.pre_exec(move || apply_memory_limit(bytes));
    }
}

pub(crate) fn kill_group(pgid: u32) {
    if pgid == 0 {
        return;
    }
    let _ = nix::sys::signal::kill(
        Pid::from_raw(-(pgid as i32)),
        nix::sys::signal::Signal::SIGKILL,
    );
}

/// Owns a process group established by Command::process_group(0).
/// Idempotent termination avoids signalling a stale group on a later Drop.
/// This is lifecycle cleanup, not protection against a child calling setsid().
pub(crate) struct ProcessGroup(AtomicU32);

impl ProcessGroup {
    pub(crate) fn new(pgid: u32) -> Self {
        Self(AtomicU32::new(pgid))
    }
    pub(crate) fn terminate(&self) {
        kill_group(self.0.swap(0, Ordering::AcqRel));
    }
}

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        self.terminate();
    }
}

/// Both backends share the same exit/deadline/drop cleanup and pipe draining.
pub(crate) async fn collect_oneshot(
    mut child: tokio::process::Child,
    request: crate::types::SandboxRequest,
    report: crate::types::SandboxRunReport,
    cancel: tetonic_domain::work_scope::CancellationSignal,
) -> Result<crate::types::SandboxRunResult, SandboxError> {
    let group = ProcessGroup::new(child.id().unwrap_or(0));
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| SandboxError::Io("stdout unavailable".into()))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| SandboxError::Io("stderr unavailable".into()))?;
    let mut collector = crate::output::OutputCollector::new(request.resources.clone());
    let wait = tokio::time::timeout(request.runtime_limit, async {
        let work = async {
            let drain = crate::output::drain_pipes(&mut stdout, &mut stderr, &mut collector);
            tokio::pin!(drain);
            tokio::select! {
                status = child.wait() => {
                    // A descendant may still own a pipe after the leader exits.
                    group.terminate();
                    let status = status.map_err(|e| SandboxError::Io(e.to_string()))?;
                    drain.await?;
                    Ok(status)
                }
                drained = &mut drain => {
                    drained?;
                    let status = child.wait().await.map_err(|e| SandboxError::Io(e.to_string()))?;
                    group.terminate();
                    Ok(status)
                }
            }
        };
        tokio::select! {
            biased;
            _ = async { while !cancel.is_canceled() { tokio::time::sleep(Duration::from_millis(10)).await; } } => Err(SandboxError::Canceled),
            result = work => result,
        }
    })
    .await;
    let exit_code = match wait {
        Ok(Ok(status)) => status.code(),
        result => {
            group.terminate();
            let _ = child.kill().await;
            return Err(match result {
                Ok(Err(e)) => e,
                _ => SandboxError::Timeout,
            });
        }
    };
    let (stdout, stderr, truncated) = collector.into_strings();
    Ok(crate::types::SandboxRunResult {
        success: exit_code == Some(0),
        exit_code,
        stdout,
        stderr,
        truncated,
        report,
    })
}

pub struct UnixLongLived {
    group: ProcessGroup,
    child: Mutex<tokio::process::Child>,
    stdin: Mutex<Option<tokio::process::ChildStdin>>,
    stdout: Mutex<Option<tokio::process::ChildStdout>>,
}

impl UnixLongLived {
    pub fn new(
        pgid: u32,
        child: tokio::process::Child,
        stdin: Option<tokio::process::ChildStdin>,
        stdout: Option<tokio::process::ChildStdout>,
    ) -> Self {
        Self {
            group: ProcessGroup::new(pgid),
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            stdout: Mutex::new(stdout),
        }
    }

    pub async fn write_stdin(&self, data: &[u8]) -> Result<(), SandboxError> {
        let mut guard = self.stdin.lock().await;
        if let Some(stdin) = guard.as_mut() {
            stdin
                .write_all(data)
                .await
                .map_err(|e| SandboxError::Io(e.to_string()))
        } else {
            Err(SandboxError::Io("stdin unavailable".into()))
        }
    }

    pub async fn read_stdout(&self, max_bytes: usize) -> Result<Vec<u8>, SandboxError> {
        let mut guard = self.stdout.lock().await;
        if let Some(stdout) = guard.as_mut() {
            let mut buf = vec![0u8; max_bytes.clamp(1, 4096)];
            let n = stdout
                .read(&mut buf)
                .await
                .map_err(|e| SandboxError::Io(e.to_string()))?;
            buf.truncate(n);
            Ok(buf)
        } else {
            Ok(Vec::new())
        }
    }

    pub async fn graceful_stop(&self, grace: Duration) -> Result<(), SandboxError> {
        tokio::time::sleep(grace).await;
        self.force_terminate().await
    }

    pub async fn force_terminate(&self) -> Result<(), SandboxError> {
        self.group.terminate();
        let mut child = self.child.lock().await;
        let _ = child.kill().await;
        Ok(())
    }

    pub fn is_alive(&self) -> bool {
        if let Ok(mut child) = self.child.try_lock() {
            match child.try_wait() {
                Ok(None) => true,
                _ => {
                    self.group.terminate();
                    false
                }
            }
        } else {
            true
        }
    }
}

impl Drop for UnixLongLived {
    fn drop(&mut self) {
        self.group.terminate();
        // kill_on_drop plus Tokio's child reaper handles the leader. No await in Drop.
        let _ = self.child.get_mut().start_kill();
    }
}

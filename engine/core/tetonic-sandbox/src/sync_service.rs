//! Synchronous long-lived sandbox service handles (LSP stdio bridge).

use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::backend::{
    build_minimal_env, evaluate_outcome, make_report, missing_fs_controls, missing_network_policy,
    path_denied, validate_working_directory,
};
use crate::types::{
    IsolationOutcome, ProcessMode, SandboxControl, SandboxError, SandboxRequest, SandboxRunReport,
    ServiceResourceAccounting,
};

/// Blocking long-lived sandbox service with stdin/stdout pipes.
pub struct SyncLongLivedService {
    pub report: SandboxRunReport,
    pub accounting: ServiceResourceAccounting,
    stdin: Box<dyn Write + Send>,
    stdout: Box<dyn Read + Send>,
    inner: SyncServiceInner,
}

impl Drop for SyncLongLivedService {
    fn drop(&mut self) {
        // Kill before BufWriter drops: flushing a buffered pipe to a child that
        // no longer reads must not block service destruction indefinitely.
        let _ = self.force_terminate();
    }
}

#[cfg(windows)]
struct WinHandle(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
unsafe impl Send for WinHandle {}

#[cfg(windows)]
unsafe impl Sync for WinHandle {}

enum SyncServiceInner {
    #[cfg(windows)]
    Windows(WindowsSyncService),
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    Unix(UnixSyncService),
}

#[cfg(windows)]
struct WindowsSyncService {
    job: WinHandle,
    process: WinHandle,
    main_thread: WinHandle,
}

#[cfg(windows)]
impl Drop for WindowsSyncService {
    fn drop(&mut self) {
        windows_terminate_job(self.job.0);
        windows_close_process(self);
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
struct UnixSyncService {
    child: std::sync::Mutex<std::process::Child>,
    group: crate::backend::unix_common::ProcessGroup,
}

impl SyncLongLivedService {
    /// Start a long-lived sandboxed process with synchronous stdio handles.
    pub fn spawn(request: SandboxRequest) -> Result<Self, SandboxError> {
        Self::spawn_with_execution_id(request, format!("svc_{}", uuid_suffix()))
    }

    pub fn spawn_with_execution_id(
        mut request: SandboxRequest,
        execution_id: String,
    ) -> Result<Self, SandboxError> {
        request.mode = ProcessMode::LongLived;
        let wd = validate_working_directory(&request)?;
        if path_denied(&wd, &request.filesystem.denied_paths) {
            return Err(SandboxError::Denied(
                "working directory matches denied path scope".into(),
            ));
        }

        let caps = crate::platform_backend().capabilities();
        #[cfg(windows)]
        let caps = {
            let mut caps = caps;
            // The synchronous spawn path does not install a firewall guard.
            // Do not borrow a network guarantee from the asynchronous backend.
            caps.network_denial = false;
            caps.network_allowlisting = false;
            caps
        };
        let mut enforced = vec![
            SandboxControl::ProcessTreeContainment,
            SandboxControl::RuntimeLimit,
            SandboxControl::EnvironmentFiltering,
            SandboxControl::OutputLimit,
            SandboxControl::HandleInheritanceRestriction,
            SandboxControl::WorkingDirectoryRestriction,
            SandboxControl::LongLivedService,
        ];
        if caps.child_breakaway_prevention {
            enforced.push(SandboxControl::ChildBreakawayPrevention);
        }
        if request.resources.max_memory_bytes.is_some() {
            enforced.push(SandboxControl::MemoryLimit);
        }
        if request.resources.max_child_processes.is_some() {
            enforced.push(SandboxControl::ProcessCountLimit);
        }

        let mut missing = missing_fs_controls(&request, &caps);
        if let Some(m) = missing_network_policy(&request, &caps) {
            missing.push(m);
        }

        let outcome = evaluate_outcome(&caps, &request, enforced.clone(), missing.clone())?;
        if matches!(outcome, IsolationOutcome::Denied { .. }) {
            return Err(SandboxError::Denied("isolation denied".into()));
        }

        let report = make_report(caps, outcome, request.process_class.clone());
        let accounting = ServiceResourceAccounting {
            execution_id,
            started_at: Instant::now(),
            restart_count: 0,
            process_class: request.process_class.clone(),
        };

        #[cfg(windows)]
        {
            spawn_windows(request, wd, report, accounting)
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            spawn_unix(request, wd, report, accounting)
        }
    }

    pub fn stdin(&mut self) -> &mut (dyn Write + Send) {
        self.stdin.as_mut()
    }

    pub fn stdout(&mut self) -> &mut (dyn Read + Send) {
        self.stdout.as_mut()
    }

    pub fn take_stdio(mut self) -> (Box<dyn Write + Send>, Box<dyn Read + Send>, Self) {
        let stdin = std::mem::replace(&mut self.stdin, Box::new(std::io::sink()));
        let stdout = std::mem::replace(&mut self.stdout, Box::new(std::io::empty()));
        (stdin, stdout, self)
    }

    pub fn is_alive(&self) -> bool {
        match &self.inner {
            #[cfg(windows)]
            SyncServiceInner::Windows(w) => {
                if w.process.0.is_null() {
                    return false;
                }
                windows_alive(w.process.0)
            }
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            SyncServiceInner::Unix(u) => {
                let mut child = u.child.lock().unwrap_or_else(|e| e.into_inner());
                match child.try_wait() {
                    Ok(None) => true,
                    _ => {
                        u.group.terminate();
                        false
                    }
                }
            }
        }
    }

    pub fn graceful_stop(&mut self, grace: Duration) -> Result<(), SandboxError> {
        std::thread::sleep(grace);
        self.force_terminate()
    }

    pub fn force_terminate(&mut self) -> Result<(), SandboxError> {
        match &mut self.inner {
            #[cfg(windows)]
            SyncServiceInner::Windows(w) => {
                windows_terminate_job(w.job.0);
                windows_close_process(w);
                w.job.0 = std::ptr::null_mut();
                w.process.0 = std::ptr::null_mut();
                w.main_thread.0 = std::ptr::null_mut();
                Ok(())
            }
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            SyncServiceInner::Unix(u) => {
                u.group.terminate();
                let child = u.child.get_mut().unwrap_or_else(|e| e.into_inner());
                let _ = child.kill();
                child.wait().map_err(|e| SandboxError::Io(e.to_string()))?;
                Ok(())
            }
        }
    }

    /// Terminate and respawn with the same execution id (audit correlation preserved).
    pub fn restart(&mut self, request: SandboxRequest) -> Result<(), SandboxError> {
        self.force_terminate()?;
        let execution_id = self.accounting.execution_id.clone();
        let restart_count = self.accounting.restart_count + 1;
        let mut next = Self::spawn_with_execution_id(request, execution_id)?;
        next.accounting.restart_count = restart_count;
        *self = next;
        Ok(())
    }
}

#[cfg(windows)]
fn spawn_windows(
    request: SandboxRequest,
    wd: PathBuf,
    report: SandboxRunReport,
    accounting: ServiceResourceAccounting,
) -> Result<SyncLongLivedService, SandboxError> {
    use std::os::windows::io::FromRawHandle;

    use crate::backend::windows::{create_job, resolve_command, spawn_suspended, terminate_job};
    use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
    use windows_sys::Win32::System::Threading::{ResumeThread, TerminateProcess};

    let job_owner = crate::backend::windows::OwnedNativeHandle(create_job(&request)?);
    let job = job_owner.0;
    let (exe, args) = resolve_command(&request)?;
    let env = build_minimal_env(&request.environment);
    let mut child = spawn_suspended(&exe, &args, &wd, &env, true)?;
    unsafe {
        if AssignProcessToJobObject(job, child.process) == 0 {
            let _ = TerminateProcess(child.process, 1);
            child.close_handles();
            return Err(SandboxError::SpawnFailed("job assign failed".into()));
        }
        if ResumeThread(child.main_thread) == u32::MAX {
            terminate_job(job);
            child.close_handles();
            return Err(SandboxError::SpawnFailed("ResumeThread failed".into()));
        }
    }

    let stdin = unsafe { std::fs::File::from_raw_handle(child.stdin_write as _) };
    let stdout = unsafe { std::fs::File::from_raw_handle(child.stdout_read as _) };
    child.stdin_write = std::ptr::null_mut();
    child.stdout_read = std::ptr::null_mut();
    child.close_stderr();

    let process = std::mem::replace(&mut child.process, std::ptr::null_mut());
    let main_thread = std::mem::replace(&mut child.main_thread, std::ptr::null_mut());
    std::mem::forget(job_owner); // Transfer ownership to WindowsSyncService.

    Ok(SyncLongLivedService {
        report,
        accounting,
        stdin: Box::new(std::io::BufWriter::new(stdin)),
        stdout: Box::new(std::io::BufReader::new(stdout)),
        inner: SyncServiceInner::Windows(WindowsSyncService {
            job: WinHandle(job),
            process: WinHandle(process),
            main_thread: WinHandle(main_thread),
        }),
    })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn spawn_unix(
    request: SandboxRequest,
    wd: PathBuf,
    report: SandboxRunReport,
    accounting: ServiceResourceAccounting,
) -> Result<SyncLongLivedService, SandboxError> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    #[cfg(target_os = "linux")]
    use crate::backend::unix_common::resolve_command;

    #[cfg(target_os = "linux")]
    let (exe, args) = resolve_command(&request)?;
    #[cfg(target_os = "macos")]
    let (exe, args) = crate::backend::macos::wrap_command(&request, &wd)?;
    let env = build_minimal_env(&request.environment);
    let mut cmd = Command::new(&exe);
    cmd.args(&args)
        .process_group(0)
        .current_dir(&wd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    cmd.env_clear();
    for (k, v) in env {
        cmd.env(k, v);
    }
    #[cfg(target_os = "linux")]
    {
        unsafe {
            cmd.pre_exec(crate::backend::linux::linux_child_isolation_hook(
                request.network.clone(),
                request.filesystem.clone(),
                wd.clone(),
                request.resources.max_memory_bytes,
            ));
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(mem) = request.resources.max_memory_bytes {
            unsafe {
                cmd.pre_exec(move || crate::backend::unix_common::apply_memory_limit(mem));
            }
        }
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| SandboxError::SpawnFailed(e.to_string()))?;
    let pgid = child.id();
    let stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");

    Ok(SyncLongLivedService {
        report,
        accounting,
        stdin: Box::new(std::io::BufWriter::new(stdin)),
        stdout: Box::new(std::io::BufReader::new(stdout)),
        inner: SyncServiceInner::Unix(UnixSyncService {
            child: std::sync::Mutex::new(child),
            group: crate::backend::unix_common::ProcessGroup::new(pgid),
        }),
    })
}

#[cfg(windows)]
fn windows_alive(process: windows_sys::Win32::Foundation::HANDLE) -> bool {
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    use windows_sys::Win32::System::Threading::WaitForSingleObject;
    unsafe { WaitForSingleObject(process, 0) != WAIT_OBJECT_0 }
}

#[cfg(windows)]
fn windows_terminate_job(job: windows_sys::Win32::Foundation::HANDLE) {
    use crate::backend::windows::terminate_job;
    terminate_job(job);
}

#[cfg(windows)]
fn windows_close_process(w: &WindowsSyncService) {
    use windows_sys::Win32::Foundation::CloseHandle;
    unsafe {
        if !w.main_thread.0.is_null() {
            CloseHandle(w.main_thread.0);
        }
        if !w.process.0.is_null() {
            CloseHandle(w.process.0);
        }
        if !w.job.0.is_null() {
            CloseHandle(w.job.0);
        }
    }
}

fn uuid_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

//! Windows sandbox backend using Job Objects (M2-3).

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::Mutex;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, SetHandleInformation, GENERIC_READ, HANDLE, HANDLE_FLAG_INHERIT,
    INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOB_OBJECT_LIMIT_PROCESS_MEMORY,
};
use windows_sys::Win32::System::Pipes::{CreatePipe, PeekNamedPipe};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, GetExitCodeProcess, ResumeThread, TerminateProcess, WaitForSingleObject,
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION,
    STARTF_USESTDHANDLES, STARTUPINFOW,
};

use crate::backend::{
    build_minimal_env, evaluate_outcome, make_report, path_denied, validate_working_directory,
    SandboxBackend,
};
use crate::output::{truncate_for_display, OutputCollector};
use crate::types::{
    IsolationOutcome, LongLivedInner, LongLivedSandboxHandle, ProcessMode, SandboxCapabilities,
    SandboxControl, SandboxError, SandboxRequest, SandboxRunResult, SandboxedProcess,
};

#[path = "windows_cancel.rs"]
mod cancellation;
use cancellation::terminate_canceled_job;

pub struct WindowsSandboxBackend;

impl WindowsSandboxBackend {
    pub fn new() -> Self {
        Self
    }

    fn detect_capabilities() -> SandboxCapabilities {
        SandboxCapabilities {
            process_tree_containment: true,
            child_breakaway_prevention: true,
            runtime_enforcement: true,
            memory_limits: true,
            cpu_limits: false,
            process_count_limits: true,
            output_limits: true,
            environment_filtering: true,
            filesystem_read_restrictions: false,
            filesystem_write_restrictions: false,
            network_denial: crate::backend::windows_net::network_denial_available(),
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
            platform: "windows".into(),
            mechanisms: {
                let mut m = vec![
                    "Job Objects (KILL_ON_JOB_CLOSE, ACTIVE_PROCESS, PROCESS_MEMORY)".into(),
                    "CREATE_SUSPENDED + AssignProcessToJobObject before resume".into(),
                    "Minimal allowlisted environment".into(),
                    "Bounded stdout/stderr collection".into(),
                    "TerminateJobObject for tree kill".into(),
                ];
                if crate::backend::windows_net::network_denial_available() {
                    m.push(
                        "Windows Firewall outbound block per child executable (elevated)".into(),
                    );
                }
                m
            },
        }
    }
}

impl Default for WindowsSandboxBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SandboxBackend for WindowsSandboxBackend {
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
            SandboxControl::ProcessTreeContainment,
            SandboxControl::ChildBreakawayPrevention,
            SandboxControl::RuntimeLimit,
            SandboxControl::EnvironmentFiltering,
            SandboxControl::OutputLimit,
            SandboxControl::HandleInheritanceRestriction,
            SandboxControl::WorkingDirectoryRestriction,
        ];
        if request.resources.max_memory_bytes.is_some() {
            enforced.push(SandboxControl::MemoryLimit);
        }
        if request.resources.max_child_processes.is_some() {
            enforced.push(SandboxControl::ProcessCountLimit);
        }
        if caps.network_denial
            && matches!(
                request.network,
                crate::types::NetworkPolicy::DenyAll | crate::types::NetworkPolicy::AllowLoopback
            )
        {
            enforced.push(SandboxControl::NetworkDenial);
        }

        let missing = crate::backend::collect_missing_controls(&request, &caps);

        let outcome = evaluate_outcome(&caps, &request, enforced.clone(), missing.clone())?;
        if matches!(outcome, IsolationOutcome::Denied { .. }) {
            return Err(SandboxError::Denied("isolation denied".into()));
        }

        let report = make_report(caps.clone(), outcome, request.process_class.clone());

        match request.mode {
            ProcessMode::OneShot => {
                let result = tokio::task::spawn_blocking(move || {
                    run_jobbed_oneshot(request, wd, report, enforced, cancel)
                })
                .await
                .map_err(|e| SandboxError::Internal(e.to_string()))??;
                Ok(SandboxedProcess::Completed(result))
            }
            ProcessMode::LongLived => {
                let handle = spawn_long_lived(request, wd, report)?;
                Ok(SandboxedProcess::Service(handle))
            }
        }
    }
}

/// Own each native handle immediately, including partially constructed processes.
pub(crate) struct OwnedNativeHandle(pub(crate) HANDLE);
impl Drop for OwnedNativeHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

fn run_jobbed_oneshot(
    request: SandboxRequest,
    wd: std::path::PathBuf,
    report: crate::types::SandboxRunReport,
    enforced: Vec<SandboxControl>,
    cancel: tetonic_domain::work_scope::CancellationSignal,
) -> Result<SandboxRunResult, SandboxError> {
    if cancel.is_canceled() {
        return Err(SandboxError::Canceled);
    }
    let job_owner = OwnedNativeHandle(create_job(&request)?);
    let job = job_owner.0;
    let (exe, args) = resolve_command(&request)?;
    let _firewall = if matches!(
        request.network,
        crate::types::NetworkPolicy::DenyAll | crate::types::NetworkPolicy::AllowLoopback
    ) && crate::backend::windows_net::network_denial_available()
    {
        let path = std::path::Path::new(&exe);
        Some(
            crate::backend::windows_net::block_outbound_for_exe(path)
                .map_err(|e| SandboxError::SpawnFailed(format!("network denial: {e}")))?,
        )
    } else {
        None
    };
    let env = build_minimal_env(&request.environment);
    let mut child = spawn_suspended(&exe, &args, &wd, &env, false)?;
    unsafe {
        if AssignProcessToJobObject(job, child.process) == 0 {
            let _ = TerminateProcess(child.process, 1);
            close_process(&mut child);

            return Err(SandboxError::SpawnFailed(format!(
                "AssignProcessToJobObject failed: {}",
                GetLastError()
            )));
        }
        if ResumeThread(child.main_thread) == u32::MAX {
            terminate_job(job);
            close_process(&mut child);

            return Err(SandboxError::SpawnFailed("ResumeThread failed".into()));
        }
    }

    let deadline = Instant::now() + request.runtime_limit;
    let mut collector = OutputCollector::new(request.resources.clone());
    let mut stdout_buf = vec![0u8; 4096];
    let mut stderr_buf = vec![0u8; 4096];
    let exit_code: Option<i32>;

    loop {
        if cancel.is_canceled() {
            let stopped = terminate_canceled_job(job);
            close_process(&mut child);
            stopped?;
            return Err(SandboxError::Canceled);
        }
        if Instant::now() >= deadline {
            terminate_job(job);
            close_process(&mut child);
            return Err(SandboxError::Timeout);
        }
        // Non-blocking drain while the child is alive — blocking ReadFile here
        // never rechecks the deadline and deadlocks if the child is quiet or
        // waiting on stdin (classic after inheritable stdio was fixed).
        if drain_available(child.stdout_read, &mut stdout_buf, &mut collector, true).is_err()
            || drain_available(child.stderr_read, &mut stderr_buf, &mut collector, false).is_err()
        {
            terminate_job(job);
            close_process(&mut child);
            let (stdout, stderr, _truncated) = collector.into_strings();
            return Ok(SandboxRunResult {
                success: false,
                exit_code: None,
                stdout,
                stderr,
                truncated: true,
                report: crate::types::SandboxRunReport {
                    audit_summary: format!(
                        "{}; enforced={:?}; output_abuse=true",
                        report.audit_summary, enforced
                    ),
                    ..report
                },
            });
        }

        let wait = unsafe { WaitForSingleObject(child.process, 50) };
        if wait == WAIT_OBJECT_0 {
            let mut code = 0u32;
            unsafe {
                GetExitCodeProcess(child.process, &mut code);
            }
            exit_code = Some(code as i32);
            terminate_job(job);
            drain_available(child.stdout_read, &mut stdout_buf, &mut collector, true).ok();
            drain_available(child.stderr_read, &mut stderr_buf, &mut collector, false).ok();
            break;
        }
    }

    terminate_job(job);
    close_process(&mut child);

    let (stdout, stderr, truncated) = collector.into_strings();
    let combined = format!(
        "exit code: {}\nstdout:\n{}\nstderr:\n{}",
        exit_code.unwrap_or(-1),
        stdout,
        stderr
    );
    let _display = truncate_for_display(&combined, request.resources.max_output_bytes_per_stream);

    Ok(SandboxRunResult {
        success: exit_code == Some(0),
        exit_code,
        stdout,
        stderr,
        truncated,
        report: crate::types::SandboxRunReport {
            audit_summary: format!("{}; enforced={:?}", report.audit_summary, enforced),
            ..report
        },
    })
}

fn spawn_long_lived(
    request: SandboxRequest,
    wd: std::path::PathBuf,
    report: crate::types::SandboxRunReport,
) -> Result<LongLivedSandboxHandle, SandboxError> {
    let job_owner = OwnedNativeHandle(create_job(&request)?);
    let job = job_owner.0;
    let (exe, args) = resolve_command(&request)?;
    let firewall = if matches!(
        request.network,
        crate::types::NetworkPolicy::DenyAll | crate::types::NetworkPolicy::AllowLoopback
    ) && crate::backend::windows_net::network_denial_available()
    {
        Some(
            crate::backend::windows_net::block_outbound_for_exe(std::path::Path::new(&exe))
                .map_err(|e| SandboxError::SpawnFailed(format!("network denial: {e}")))?,
        )
    } else {
        None
    };
    let env = build_minimal_env(&request.environment);
    let mut child = spawn_suspended(&exe, &args, &wd, &env, true)?;
    unsafe {
        if AssignProcessToJobObject(job, child.process) == 0 {
            let _ = TerminateProcess(child.process, 1);
            close_process(&mut child);

            return Err(SandboxError::SpawnFailed("job assign failed".into()));
        }
        if ResumeThread(child.main_thread) == u32::MAX {
            terminate_job(job);
            close_process(&mut child);
            return Err(SandboxError::SpawnFailed("ResumeThread failed".into()));
        }
    }
    std::mem::forget(job_owner); // Ownership passes to WindowsLongLived.
    Ok(LongLivedSandboxHandle {
        report,
        inner: LongLivedInner::Windows(WindowsLongLived {
            job: JobHandle(job),
            child: Mutex::new(child),
            _firewall: firewall,
        }),
    })
}

pub struct WindowsLongLived {
    job: JobHandle,
    child: Mutex<SandboxChild>,
    _firewall: Option<crate::backend::windows_net::FirewallRuleGuard>,
}

struct JobHandle(HANDLE);

impl Drop for JobHandle {
    fn drop(&mut self) {
        // The job remains owned until the service is dropped. Explicit stop
        // terminates it without closing a handle that another call can reuse.
        terminate_job(self.0);
        unsafe {
            CloseHandle(self.0);
        }
    }
}

unsafe impl Send for JobHandle {}
unsafe impl Sync for JobHandle {}

impl WindowsLongLived {
    pub async fn write_stdin(&self, data: &[u8]) -> Result<(), SandboxError> {
        let child = self.child.lock().await;
        write_pipe(child.stdin_write, data)
    }

    pub async fn read_stdout(&self, max_bytes: usize) -> Result<Vec<u8>, SandboxError> {
        let child = self.child.lock().await;
        read_pipe_limited(child.stdout_read, max_bytes)
    }

    pub async fn graceful_stop(&self, grace: Duration) -> Result<(), SandboxError> {
        tokio::time::sleep(grace).await;
        self.force_terminate().await
    }

    pub async fn force_terminate(&self) -> Result<(), SandboxError> {
        let mut child = self.child.lock().await;
        terminate_job(self.job.0);
        close_process(&mut child);
        Ok(())
    }

    pub fn is_alive(&self) -> bool {
        if let Ok(child) = self.child.try_lock() {
            if child.process.is_null() {
                return false;
            }
            unsafe {
                let w = WaitForSingleObject(child.process, 0);
                w != WAIT_OBJECT_0
            }
        } else {
            true
        }
    }
}

pub(crate) struct SandboxChild {
    pub(crate) process: HANDLE,
    pub(crate) main_thread: HANDLE,
    pub(crate) stdin_write: HANDLE,
    pub(crate) stdout_read: HANDLE,
    stderr_read: HANDLE,
}

impl Drop for SandboxChild {
    fn drop(&mut self) {
        close_process(self);
    }
}

impl SandboxChild {
    pub(crate) fn close_stderr(&mut self) {
        unsafe {
            if !self.stderr_read.is_null() && self.stderr_read != INVALID_HANDLE_VALUE {
                CloseHandle(self.stderr_read);
            }
        }
        self.stderr_read = std::ptr::null_mut();
    }

    pub(crate) fn close_handles(&mut self) {
        close_process(self);
    }
}

pub(crate) fn create_job(request: &SandboxRequest) -> Result<HANDLE, SandboxError> {
    unsafe {
        let job = CreateJobObjectW(ptr::null(), ptr::null());
        if job.is_null() {
            return Err(SandboxError::SpawnFailed(format!(
                "CreateJobObjectW: {}",
                GetLastError()
            )));
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
        if let Some(max_procs) = request.resources.max_child_processes {
            info.BasicLimitInformation.ActiveProcessLimit = max_procs;
        }
        if let Some(mem) = request.resources.max_memory_bytes {
            info.ProcessMemoryLimit = mem as usize;
            info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_PROCESS_MEMORY;
        }
        if SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *mut core::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) == 0
        {
            CloseHandle(job);
            return Err(SandboxError::SpawnFailed(format!(
                "SetInformationJobObject: {}",
                GetLastError()
            )));
        }
        Ok(job)
    }
}

pub(crate) fn resolve_command(
    request: &SandboxRequest,
) -> Result<(String, Vec<String>), SandboxError> {
    if let Some(script) = &request.shell_script {
        let shell = request.shell_identity.as_deref().unwrap_or("cmd");
        if shell.eq_ignore_ascii_case("cmd") {
            return Ok((
                std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".into()),
                vec!["/C".into(), script.clone()],
            ));
        }
        return Ok((shell.to_string(), vec!["-c".into(), script.clone()]));
    }
    if request.executable.is_empty() {
        return Err(SandboxError::Denied("empty executable".into()));
    }
    let exe = resolve_windows_executable(&request.executable);
    Ok((exe, request.arguments.clone()))
}

/// CreateProcessW lpApplicationName does not search PATH; resolve bare names.
fn resolve_windows_executable(name: &str) -> String {
    let as_path = Path::new(name);
    if as_path.is_file() {
        return name.to_string();
    }
    let path = match std::env::var_os("PATH") {
        Some(p) => p,
        None => return name.to_string(),
    };
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return candidate.to_string_lossy().into_owned();
        }
        let exe = dir.join(format!("{name}.exe"));
        if exe.is_file() {
            return exe.to_string_lossy().into_owned();
        }
    }
    name.to_string()
}

pub(crate) fn spawn_suspended(
    exe: &str,
    args: &[String],
    wd: &Path,
    env: &[(String, String)],
    pipe_stdin: bool,
) -> Result<SandboxChild, SandboxError> {
    let cmdline = build_command_line(exe, args);
    let mut cmd_wide: Vec<u16> = cmdline.encode_utf16().chain(std::iter::once(0)).collect();
    let app_wide = wide_null(exe);
    // canonicalize() yields \\?\C:\...; cmd.exe rejects that as "UNC" and falls back to C:\Windows.
    let cwd_wide = wide_null(&path_for_create_process_cwd(wd));

    let (stdin_read, stdin_write) = if pipe_stdin {
        create_pipe_pair()?
    } else {
        // STARTF_USESTDHANDLES requires a real stdin handle; NULL makes some
        // console apps block forever waiting for input.
        (open_nul_stdin()?, INVALID_HANDLE_VALUE)
    };
    let _stdin_read_owner = OwnedNativeHandle(stdin_read);
    let stdin_write_owner = OwnedNativeHandle(stdin_write);
    let (stdout_read, stdout_write) = create_pipe_pair()?;
    let stdout_read_owner = OwnedNativeHandle(stdout_read);
    let _stdout_write_owner = OwnedNativeHandle(stdout_write);
    let (stderr_read, stderr_write) = create_pipe_pair()?;

    let stderr_read_owner = OwnedNativeHandle(stderr_read);
    let _stderr_write_owner = OwnedNativeHandle(stderr_write);

    // Parent keeps read ends (stdout/stderr) and stdin write; child must not inherit those.
    make_non_inheritable(stdout_read)?;
    make_non_inheritable(stderr_read)?;
    make_non_inheritable(stdin_write)?;

    let env_block = build_env_block(env);

    let mut si: STARTUPINFOW = unsafe { std::mem::zeroed() };
    si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    si.dwFlags = STARTF_USESTDHANDLES;
    si.hStdInput = stdin_read;
    si.hStdOutput = stdout_write;
    si.hStdError = stderr_write;

    let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };

    let ok = unsafe {
        CreateProcessW(
            app_wide.as_ptr(),
            cmd_wide.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            1, // inherit handles for pipes
            CREATE_SUSPENDED | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            env_block.as_ptr() as *mut _,
            cwd_wide.as_ptr(),
            &si as *const STARTUPINFOW as *mut STARTUPINFOW,
            &mut pi,
        )
    };
    if ok == 0 {
        return Err(SandboxError::SpawnFailed(format!(
            "CreateProcessW: {}",
            unsafe { GetLastError() }
        )));
    }
    std::mem::forget((stdin_write_owner, stdout_read_owner, stderr_read_owner));
    Ok(SandboxChild {
        process: pi.hProcess,
        main_thread: pi.hThread,
        stdin_write,
        stdout_read,
        stderr_read,
    })
}

fn build_command_line(exe: &str, args: &[String]) -> String {
    let mut parts = vec![quote_arg(exe)];
    parts.extend(args.iter().map(|a| quote_arg(a)));
    parts.join(" ")
}

fn quote_arg(s: &str) -> String {
    if !s.is_empty() && !s.contains([' ', '\t', '"']) {
        return s.to_string();
    }
    // CRT argv quoting, not shell escaping. Backslashes are special only
    // immediately before a quote (including our closing quote).
    let mut quoted = String::from("\"");
    let mut slashes = 0;
    for ch in s.chars() {
        if ch == '\\' {
            slashes += 1;
            continue;
        }
        let count = if ch == '"' { slashes * 2 + 1 } else { slashes };
        quoted.extend(std::iter::repeat_n('\\', count));
        quoted.push(ch);
        slashes = 0;
    }
    quoted.extend(std::iter::repeat_n('\\', slashes * 2));
    quoted.push('"');
    quoted
}

#[test]
fn crt_quoting_preserves_empty_quotes_and_trailing_slashes() {
    assert_eq!(quote_arg(""), "\"\"");
    assert_eq!(quote_arg("/C"), "/C");
    assert_eq!(
        quote_arg("C:\\Program Files\\"),
        "\"C:\\Program Files\\\\\""
    );
    assert_eq!(quote_arg("a\\\"b"), "\"a\\\\\\\"b\"");
}

fn wide_null(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Strip Win32 extended/verbatim prefixes so console apps (esp. cmd.exe) accept the cwd.
pub(crate) fn path_for_create_process_cwd(wd: &Path) -> String {
    let s = wd.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        if let Some(unc) = rest.strip_prefix("UNC\\") {
            return format!(r"\\{unc}");
        }
        return rest.to_string();
    }
    if let Some(rest) = s.strip_prefix(r"\\.\") {
        // Device path form of a drive letter (rare); cmd also rejects these.
        if rest.len() >= 2 && rest.as_bytes()[1] == b':' {
            return rest.to_string();
        }
    }
    s.into_owned()
}

fn build_env_block(env: &[(String, String)]) -> Vec<u16> {
    let mut block = Vec::new();
    for (k, v) in env {
        let entry = format!("{k}={v}");
        block.extend(entry.encode_utf16());
        block.push(0);
    }
    block.push(0);
    block
}

fn create_pipe_pair() -> Result<(HANDLE, HANDLE), SandboxError> {
    // Child stdio requires inheritable pipe ends. CreatePipe(NULL) yields
    // non-inheritable handles; CreateProcess then gives the child broken
    // stdout/stderr → empty I/O and exit 1 for even `echo` / `exit 0`.
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: ptr::null_mut(),
        bInheritHandle: 1,
    };
    let mut read: HANDLE = std::ptr::null_mut();
    let mut write: HANDLE = std::ptr::null_mut();
    unsafe {
        if CreatePipe(&mut read, &mut write, &sa, 0) == 0 {
            return Err(SandboxError::SpawnFailed(format!(
                "CreatePipe failed: {}",
                GetLastError()
            )));
        }
    }
    Ok((read, write))
}

fn open_nul_stdin() -> Result<HANDLE, SandboxError> {
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: ptr::null_mut(),
        bInheritHandle: 1,
    };
    let path = wide_null("NUL");
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &sa,
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(SandboxError::SpawnFailed(format!(
            "CreateFileW(NUL): {}",
            unsafe { GetLastError() }
        )));
    }
    Ok(handle)
}

/// Mark a handle non-inheritable so only the child's end is duplicated into the child.
fn make_non_inheritable(handle: HANDLE) -> Result<(), SandboxError> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Ok(());
    }
    unsafe {
        if SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) == 0 {
            return Err(SandboxError::SpawnFailed(format!(
                "SetHandleInformation failed: {}",
                GetLastError()
            )));
        }
    }
    Ok(())
}

/// Read only bytes already buffered on the pipe (never blocks).
fn drain_available(
    handle: HANDLE,
    buf: &mut [u8],
    collector: &mut OutputCollector,
    stdout: bool,
) -> Result<(), SandboxError> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Ok(());
    }
    loop {
        let mut avail = 0u32;
        let ok = unsafe {
            PeekNamedPipe(
                handle,
                ptr::null_mut(),
                0,
                ptr::null_mut(),
                &mut avail,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            let err = unsafe { GetLastError() };
            if err == 109 || err == 233 {
                return Ok(());
            }
            return Err(SandboxError::Io(format!("PeekNamedPipe: {err}")));
        }
        if avail == 0 {
            return Ok(());
        }
        let want = (avail as usize).min(buf.len());
        let n = read_pipe_once(handle, &mut buf[..want])?;
        if n == 0 {
            return Ok(());
        }
        if stdout {
            collector.push_stdout(&buf[..n])?;
        } else {
            collector.push_stderr(&buf[..n])?;
        }
    }
}

fn read_pipe_once(handle: HANDLE, buf: &mut [u8]) -> Result<usize, SandboxError> {
    let mut read = 0u32;
    let ok = unsafe {
        ReadFile(
            handle,
            buf.as_mut_ptr(),
            buf.len() as u32,
            &mut read,
            ptr::null_mut(),
        )
    };
    if ok == 0 {
        let err = unsafe { GetLastError() };
        if err == 109 || err == 233 {
            return Ok(0);
        }
        return Err(SandboxError::Io(format!("ReadFile: {err}")));
    }
    Ok(read as usize)
}

fn write_pipe(handle: HANDLE, data: &[u8]) -> Result<(), SandboxError> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(SandboxError::Io("stdin not available".into()));
    }
    let mut written = 0u32;
    let ok = unsafe {
        WriteFile(
            handle,
            data.as_ptr(),
            data.len() as u32,
            &mut written,
            ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(SandboxError::Io(format!("WriteFile: {}", unsafe {
            GetLastError()
        })));
    }
    Ok(())
}

fn read_pipe_limited(handle: HANDLE, max: usize) -> Result<Vec<u8>, SandboxError> {
    let mut out = Vec::new();
    let mut buf = vec![0u8; 4096.min(max.max(1))];
    while out.len() < max {
        let n = read_pipe_once(handle, &mut buf)?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n.min(max - out.len())]);
    }
    Ok(out)
}

pub(crate) fn terminate_job(job: HANDLE) {
    if job.is_null() || job == INVALID_HANDLE_VALUE {
        return;
    }
    unsafe {
        TerminateJobObject(job, 1);
    }
}

fn close_process(child: &mut SandboxChild) {
    unsafe {
        if !child.stdin_write.is_null() && child.stdin_write != INVALID_HANDLE_VALUE {
            CloseHandle(child.stdin_write);
            child.stdin_write = std::ptr::null_mut();
        }
        if !child.stdout_read.is_null() && child.stdout_read != INVALID_HANDLE_VALUE {
            CloseHandle(child.stdout_read);
            child.stdout_read = std::ptr::null_mut();
        }
        if !child.stderr_read.is_null() && child.stderr_read != INVALID_HANDLE_VALUE {
            CloseHandle(child.stderr_read);
            child.stderr_read = std::ptr::null_mut();
        }
        if !child.main_thread.is_null() {
            CloseHandle(child.main_thread);
            child.main_thread = std::ptr::null_mut();
        }
        if !child.process.is_null() {
            CloseHandle(child.process);
            child.process = std::ptr::null_mut();
        }
    }
}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;

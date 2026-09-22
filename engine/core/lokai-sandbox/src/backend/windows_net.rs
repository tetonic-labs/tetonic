//! Windows OS network denial (H1-3).
//!
//! Outbound block via Windows Firewall (`New-NetFirewallRule`) scoped to the
//! child executable. Requires an elevated process (typical on CI
//! `windows-latest`). When not elevated, `network_denial` stays false and
//! profiles stay on the H1-2 warn path (Standard) rather than hard-denying
//! every shell.

use std::os::windows::ffi::OsStringExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

static ELEVATED: OnceLock<bool> = OnceLock::new();
static RULE_SEQ: AtomicU64 = AtomicU64::new(1);

pub fn network_denial_available() -> bool {
    *ELEVATED.get_or_init(is_elevated)
}

fn is_elevated() -> bool {
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut ret_len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

// Program paths are data in the child environment, never PowerShell source.
const INSTALL_SCRIPT: &str = "$ErrorActionPreference = 'Stop'; New-NetFirewallRule -ErrorAction Stop -DisplayName $env:LOKAI_FW_RULE -Name $env:LOKAI_FW_RULE -Direction Outbound -Action Block -Program $env:LOKAI_FW_PROGRAM -Profile Any | Out-Null";
const REMOVE_SCRIPT: &str = "$ErrorActionPreference = 'Stop'; Get-NetFirewallRule -Name $env:LOKAI_FW_RULE -ErrorAction SilentlyContinue | Remove-NetFirewallRule -ErrorAction Stop";

fn powershell_path() -> Result<PathBuf, String> {
    let mut buffer = vec![0u16; 32768];
    let count = unsafe {
        windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW(
            buffer.as_mut_ptr(),
            buffer.len() as u32,
        )
    } as usize;
    if count == 0 || count >= buffer.len() {
        return Err("cannot resolve Windows system directory".into());
    }
    Ok(
        PathBuf::from(std::ffi::OsString::from_wide(&buffer[..count]))
            .join("WindowsPowerShell/v1.0/powershell.exe"),
    )
}

fn firewall_command(script: &str, name: &str, exe: Option<&Path>) -> Result<Command, String> {
    let mut command = Command::new(powershell_path()?);
    command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ])
        .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
        .env("LOKAI_FW_RULE", name)
        .env_remove("LOKAI_FW_PROGRAM")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(exe) = exe {
        command.env("LOKAI_FW_PROGRAM", exe);
    }
    Ok(command)
}

fn run_command(mut command: Command) -> Result<(), String> {
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(format!("firewall adapter failed: {status}"))
                }
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(match result {
                    Err(e) => e.to_string(),
                    _ => "firewall adapter timed out".into(),
                });
            }
        }
    }
}

/// Install a temporary outbound-block rule. The guard also attempts cleanup
/// when installation fails after the OS may have created a rule.
pub fn block_outbound_for_exe(exe: &Path) -> Result<FirewallRuleGuard, String> {
    if !network_denial_available() {
        return Err("Windows network denial requires elevation".into());
    }
    let exe =
        std::fs::canonicalize(exe).map_err(|e| format!("invalid firewall executable: {e}"))?;
    let id = RULE_SEQ.fetch_add(1, Ordering::Relaxed);
    let guard = FirewallRuleGuard {
        name: format!("LokaiSandboxDeny-{id}-{}", std::process::id()),
    };
    run_command(firewall_command(INSTALL_SCRIPT, &guard.name, Some(&exe))?)?;
    Ok(guard)
}

pub struct FirewallRuleGuard {
    name: String,
}
impl Drop for FirewallRuleGuard {
    fn drop(&mut self) {
        if let Err(error) = firewall_command(REMOVE_SCRIPT, &self.name, None).and_then(run_command)
        {
            tracing::warn!(rule = %self.name, %error, "firewall rule cleanup failed");
        }
    }
}

#[cfg(test)]
#[path = "windows_net_tests.rs"]
mod tests;

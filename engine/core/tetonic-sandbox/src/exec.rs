//! Bounded subprocess execution (D2): timeout + output cap + minimal env (AR2-3).

use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// Default wall-clock limit for shell commands.
pub const DEFAULT_SHELL_TIMEOUT: Duration = Duration::from_secs(120);
/// Default wall-clock limit for verify-before-finish.
pub const DEFAULT_VERIFY_TIMEOUT: Duration = Duration::from_secs(300);

pub const ENV_ALLOWLIST: &[&str] = &[
    "PATH",
    "PATHEXT",
    "HOME",
    "USERPROFILE",
    "SYSTEMROOT",
    "SystemRoot",
    "WINDIR",
    "ComSpec",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TMP",
    "TEMP",
    "OS",
    "ProgramFiles",
    "ProgramFiles(x86)",
];

/// Whether subprocesses inherit a scrubbed or full environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EnvMode {
    /// Minimal allowlist (Constrained tier — SEC-011).
    #[default]
    Minimal,
    /// Full inherited environment (Advisory tier — audit only).
    Inherited,
}

pub(crate) fn is_profile_home(key: &str) -> bool {
    key.eq_ignore_ascii_case("HOME") || key.eq_ignore_ascii_case("USERPROFILE")
}

pub(crate) fn is_temp_dir_key(key: &str) -> bool {
    matches!(
        key.to_ascii_uppercase().as_str(),
        "TMP" | "TEMP" | "TMPDIR"
    )
}

/// Replace inherited environment with a minimal allowlist (SEC-011).
/// HOME and USERPROFILE point at the command working directory, not the
/// operator profile, so a shell cannot read profile credential files.
pub fn apply_minimal_env(cmd: &mut Command) {
    let keep: HashMap<String, String> = std::env::vars()
        .filter(|(k, _)| {
            !is_profile_home(k)
                && !is_temp_dir_key(k)
                && ENV_ALLOWLIST
                    .iter()
                    .any(|allowed| k.eq_ignore_ascii_case(allowed))
        })
        .collect();
    cmd.env_clear();
    for (k, v) in keep {
        cmd.env(k, v);
    }
    if let Some(dir) = cmd.get_current_dir().map(|dir| dir.to_path_buf()) {
        cmd.env("HOME", &dir);
        cmd.env("USERPROFILE", &dir);
        cmd.env("TMP", &dir);
        cmd.env("TEMP", &dir);
        cmd.env("TMPDIR", &dir);
    }
}

fn apply_env_mode(cmd: &mut Command, mode: EnvMode) {
    if mode == EnvMode::Minimal {
        apply_minimal_env(cmd);
    }
}

fn command_has_shell_metachar(s: &str) -> bool {
    s.contains(';')
        || s.contains('|')
        || s.contains('&')
        || s.contains('`')
        || s.contains("$(")
        || s.contains('\n')
        || s.contains('\r')
}

const ALLOWED_VERIFY_BINARIES: &[&str] = &[
    "cargo", "npm", "pnpm", "yarn", "bun", "node", "deno", "go", "python", "python3", "pytest",
    "uv", "poetry", "make", "ctest", "mvn", "gradle", "gradlew", "ninja",
];

/// Parse a verify command into argv without invoking a shell (SEC-004 / SEC-012 / VER-102).
pub fn split_verify_command(
    command: &str,
    workspace: &Path,
) -> Result<(String, Vec<String>), String> {
    let cmd = command.trim();
    if cmd.is_empty() {
        return Err("empty verify command".into());
    }
    if command_has_shell_metachar(cmd) {
        return Err("verify command contains shell metacharacters".into());
    }
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        return Err("empty verify command".into());
    }
    let exe = parts[0];
    let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
    if args.iter().any(|arg| {
        matches!(
            arg.to_ascii_lowercase().as_str(),
            "-c" | "--command" | "-command" | "-e" | "--eval" | "-encodedcommand" | "-enc"
        )
    }) {
        return Err("verify command cannot run inline code".into());
    }

    if ALLOWED_VERIFY_BINARIES.contains(&exe) {
        if exe == "python" || exe == "python3" {
            if let Some(first) = args.first() {
                if first == "-m" {
                    if args.len() < 2 {
                        return Err("missing python module name".into());
                    }
                    return Ok((exe.to_string(), args));
                } else if first.starts_with('-') {
                    return Ok((exe.to_string(), args));
                } else {
                    if args.len() != 1 {
                        return Err(
                            "verify script command must specify a single relative script path"
                                .into(),
                        );
                    }
                    validate_workspace_script_path(workspace, first)?;
                    return Ok((exe.to_string(), args));
                }
            }
        }
        return Ok((exe.to_string(), args));
    }

    let rel_script = exe.strip_prefix("./").unwrap_or(exe);
    if validate_workspace_script_path(workspace, rel_script).is_ok() {
        return Ok((rel_script.to_string(), args));
    }

    Err(format!("verify command not in argv allowlist: {exe}"))
}

fn validate_workspace_script_path(workspace: &Path, rel: &str) -> Result<(), String> {
    if rel.contains("..") || rel.starts_with('/') || rel.contains('\\') {
        return Err("verify script path must be a simple relative path".into());
    }
    if command_has_shell_metachar(rel) {
        return Err("verify script path contains shell metacharacters".into());
    }
    let abs = workspace.join(rel);
    if !abs.is_file() {
        return Err(format!("verify script not found: {rel}"));
    }
    Ok(())
}

/// Run `cmd` with a wall-clock timeout. Kills the child on expiry.
pub fn command_output_with_timeout(cmd: &mut Command, timeout: Duration) -> Result<Output, String> {
    command_output_with_timeout_and_cancel(cmd, timeout, None, EnvMode::Minimal)
}

/// Run `cmd` with timeout, optional cancel, and environment mode.
pub fn command_output_with_timeout_and_cancel(
    cmd: &mut Command,
    timeout: Duration,
    cancel: Option<&std::sync::Arc<std::sync::atomic::AtomicBool>>,
    env_mode: EnvMode,
) -> Result<Output, String> {
    let signal =
        cancel.map(|flag| tetonic_domain::work_scope::CancellationSignal::from_flag(flag.clone()));
    command_output_with_signal(cmd, timeout, signal.as_ref(), env_mode)
}

pub fn command_output_with_signal(
    cmd: &mut Command,
    timeout: Duration,
    cancel: Option<&tetonic_domain::work_scope::CancellationSignal>,
    env_mode: EnvMode,
) -> Result<Output, String> {
    if cancel.is_some_and(|s| s.is_canceled()) {
        return Err("command canceled".into());
    }
    apply_env_mode(cmd, env_mode);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("spawn failed: {e}"))?;
    let start = Instant::now();
    loop {
        if cancel.is_some_and(|f| f.is_canceled()) {
            stop_owned_process(&mut child);
            return Err("command canceled".into());
        }
        match child.try_wait() {
            Ok(Some(_status)) => {
                return child
                    .wait_with_output()
                    .map_err(|e| format!("wait failed: {e}"));
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    stop_owned_process(&mut child);
                    return Err(format!("command timed out after {}s", timeout.as_secs()));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("wait failed: {e}")),
        }
    }
}

/// Stop the spawned process and processes it started. `Child::kill` stops only
/// the direct process, which leaves a shell's children running.
fn stop_owned_process(child: &mut std::process::Child) {
    let pid = child.id();
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(unix)]
    {
        // SAFETY: `pid` is the child spawned above as its own process-group leader.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

pub const MAX_OUTPUT_BYTES: usize = 60_000;

pub fn truncate_output(s: &str) -> String {
    if s.len() <= MAX_OUTPUT_BYTES {
        s.to_string()
    } else {
        let mut cut = MAX_OUTPUT_BYTES;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}\n…[truncated {} bytes]", &s[..cut], s.len() - cut)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tetonic_domain::work_scope::WorkScope;

    #[test]
    fn cancel_stops_the_spawned_process() {
        let scope = WorkScope::default();
        let signal = scope.cancellation_signal();
        let join = std::thread::spawn(move || {
            let mut cmd = Command::new("ping");
            if cfg!(windows) {
                cmd.args(["-n", "30", "127.0.0.1"]);
            } else {
                cmd.args(["-c", "30", "127.0.0.1"]);
            }
            command_output_with_signal(
                &mut cmd,
                Duration::from_secs(40),
                Some(&signal),
                EnvMode::Minimal,
            )
        });
        std::thread::sleep(Duration::from_millis(400));
        scope.cancel();
        let started = Instant::now();
        let result = join.join().expect("cancel thread");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "cancel did not stop the owned process"
        );
        assert!(result.expect_err("canceled command").contains("canceled"));
    }
}

pub fn clip_shell_command(cmd: &str) -> String {
    let one = cmd.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= 72 {
        one
    } else {
        format!("{}…", one.chars().take(71).collect::<String>())
    }
}

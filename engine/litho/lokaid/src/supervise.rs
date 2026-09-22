//! Restart the coordinator after a crash (H3-3).
//!
//! Release builds keep `panic = "abort"`. Abort kills in-flight Infer and shell.
//! This parent loop restarts the same binary without `--supervise` and inherits
//! stdio. It does **not** continue the crashed turn; recovery is rehydrate from
//! `lokai.db` (pending approvals in live memory are not restored).

use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub const MAX_RESTARTS_IN_WINDOW: u32 = 8;
pub const RESTART_WINDOW: Duration = Duration::from_secs(60);
pub const INITIAL_BACKOFF: Duration = Duration::from_millis(200);
pub const MAX_BACKOFF: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildOutcome {
    Clean,
    Crash { code: Option<i32> },
}

pub fn child_outcome(status: ExitStatus) -> ChildOutcome {
    if status.success() {
        ChildOutcome::Clean
    } else {
        ChildOutcome::Crash {
            code: status.code(),
        }
    }
}

pub fn should_restart(outcome: ChildOutcome, restarts_in_window: u32, max: u32) -> bool {
    match outcome {
        ChildOutcome::Clean => false,
        ChildOutcome::Crash { .. } => restarts_in_window < max,
    }
}

pub fn next_backoff(current: Duration) -> Duration {
    let doubled = current.saturating_mul(2);
    if doubled > MAX_BACKOFF {
        MAX_BACKOFF
    } else {
        doubled
    }
}

pub fn strip_supervise_flag(args: &[String]) -> Vec<String> {
    args.iter()
        .filter(|a| a.as_str() != "--supervise")
        .cloned()
        .collect()
}

pub fn wants_supervise(args: &[String]) -> bool {
    args.iter().any(|a| a == "--supervise")
        || std::env::var("LOKAI_SUPERVISE")
            .ok()
            .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

/// Parent loop. Child is this binary without `--supervise` and without
/// `LOKAI_SUPERVISE` so it cannot fork another supervisor.
pub fn run() -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let child_args = strip_supervise_flag(&raw);
    let mut backoff = INITIAL_BACKOFF;
    let mut window_start = Instant::now();
    let mut restarts = 0u32;
    loop {
        eprintln!("lokaid: supervisor starting coordinator");
        let status = Command::new(&exe)
            .args(&child_args)
            .env_remove("LOKAI_SUPERVISE")
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()?;
        match child_outcome(status) {
            ChildOutcome::Clean => return Ok(()),
            ChildOutcome::Crash { code } => {
                if window_start.elapsed() > RESTART_WINDOW {
                    window_start = Instant::now();
                    restarts = 0;
                    backoff = INITIAL_BACKOFF;
                }
                if !should_restart(
                    ChildOutcome::Crash { code },
                    restarts,
                    MAX_RESTARTS_IN_WINDOW,
                ) {
                    eprintln!(
                        "lokaid: supervisor giving up after {restarts} crashes in {:?}",
                        RESTART_WINDOW
                    );
                    std::process::exit(code.unwrap_or(1));
                }
                restarts += 1;
                eprintln!(
                    "lokaid: coordinator crashed (code={code:?}); restart {restarts}/{} in {:?}",
                    MAX_RESTARTS_IN_WINDOW, RESTART_WINDOW
                );
                thread::sleep(backoff);
                backoff = next_backoff(backoff);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_exit_does_not_restart() {
        assert!(!should_restart(
            ChildOutcome::Clean,
            0,
            MAX_RESTARTS_IN_WINDOW
        ));
    }

    #[test]
    fn crash_restarts_until_window_cap() {
        assert!(should_restart(
            ChildOutcome::Crash { code: Some(1) },
            0,
            MAX_RESTARTS_IN_WINDOW
        ));
        assert!(!should_restart(
            ChildOutcome::Crash { code: Some(1) },
            MAX_RESTARTS_IN_WINDOW,
            MAX_RESTARTS_IN_WINDOW
        ));
    }

    #[test]
    fn strip_supervise_leaves_other_flags() {
        let args = vec![
            "--supervise".into(),
            "--combined".into(),
            "--supervise".into(),
        ];
        assert_eq!(strip_supervise_flag(&args), vec!["--combined".to_string()]);
    }

    #[test]
    fn backoff_caps() {
        assert_eq!(next_backoff(INITIAL_BACKOFF), Duration::from_millis(400));
        assert_eq!(next_backoff(MAX_BACKOFF), MAX_BACKOFF);
    }

    #[test]
    fn wants_supervise_flag() {
        assert!(wants_supervise(&["--supervise".into()]));
        assert!(!wants_supervise(&["--combined".into()]));
    }
}

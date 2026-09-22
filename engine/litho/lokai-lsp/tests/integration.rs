//! End-to-end tests against the in-crate mock language server.

#[path = "support/command_launcher.rs"]
mod command_launcher;

use std::path::PathBuf;

use lokai_lsp::{Lang, LspPool, LspSession, ServerSpec};
use tempfile::TempDir;

struct TrackedLauncher(std::sync::Arc<std::sync::atomic::AtomicBool>);
struct TrackedProcess {
    inner: lokai_lsp::SpawnedLspProcess,
    stopped: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl lokai_lsp::LspProcessHandle for TrackedProcess {
    fn is_alive(&self) -> bool {
        self.inner.is_alive()
    }
    fn stop(&mut self) {
        self.inner.stop();
        self.stopped
            .store(!self.inner.is_alive(), std::sync::atomic::Ordering::SeqCst);
    }
}
impl lokai_lsp::LspProcessLauncher for TrackedLauncher {
    fn spawn(
        &self,
        spec: &ServerSpec,
        root: &std::path::Path,
    ) -> Result<lokai_lsp::SpawnedLspProcess, lokai_lsp::LspError> {
        let mut inner = command_launcher::CommandLspLauncher.spawn(spec, root)?;
        let stdin = std::mem::replace(&mut inner.stdin, Box::new(std::io::sink()));
        let stdout = std::mem::replace(&mut inner.stdout, Box::new(std::io::empty()));
        Ok(lokai_lsp::spawned_from_io(
            stdin,
            stdout,
            Box::new(TrackedProcess {
                inner,
                stopped: self.0.clone(),
            }),
        ))
    }
}

fn enable_mock_detect(args: &str) {
    command_launcher::install_command_launcher();
    std::env::set_var("LOKAI_LSP", "1");
    std::env::set_var("LOKAI_LSP_TEST_MOCK", "1");
    std::env::set_var(
        "LOKAI_LSP_TEST_MOCK_BIN",
        env!("CARGO_BIN_EXE_lsp-mock-server"),
    );
    if args.is_empty() {
        std::env::remove_var("LOKAI_LSP_TEST_MOCK_ARGS");
    } else {
        std::env::set_var("LOKAI_LSP_TEST_MOCK_ARGS", args);
    }
}

fn mock_spec() -> ServerSpec {
    ServerSpec {
        lang: Lang::Rust,
        program: PathBuf::from(env!("CARGO_BIN_EXE_lsp-mock-server")),
        args: Vec::new(),
        label: "mock".into(),
    }
}

#[test]
fn stalled_native_stdin_times_out_and_session_is_not_reused() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("main.rs"), "x".repeat(2 * 1024 * 1024)).unwrap();
    let mut spec = mock_spec();
    spec.args.push("--stall-input".into());
    let stopped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut session =
        LspSession::spawn_with_launcher(spec, dir.path(), Some(&TrackedLauncher(stopped.clone())))
            .unwrap();
    let started = std::time::Instant::now();
    assert!(matches!(
        session.goto_definition("main.rs", 1, 0),
        Err(lokai_lsp::LspError::Timeout)
    ));
    assert!(started.elapsed() < std::time::Duration::from_secs(30));
    assert_eq!(session.lifecycle(), lokai_lsp::LspLifecycle::Unhealthy);
    assert!(
        stopped.load(std::sync::atomic::Ordering::SeqCst),
        "timeout must stop the server before session drop"
    );
    let retry = std::time::Instant::now();
    assert!(session.goto_definition("main.rs", 1, 0).is_err());
    assert!(retry.elapsed() < std::time::Duration::from_secs(1));
    session.close().unwrap(); // Includes joining the formerly blocked writer and reader.
    assert_eq!(session.lifecycle(), lokai_lsp::LspLifecycle::Stopped);
    session.close().unwrap();
}

#[test]
fn explicit_close_joins_transport_and_is_idempotent() {
    let dir = TempDir::new().unwrap();
    let mut session = LspSession::spawn_with_launcher(
        mock_spec(),
        dir.path(),
        Some(&command_launcher::CommandLspLauncher),
    )
    .unwrap();
    assert!(session.is_healthy());
    session.close().unwrap();
    assert_eq!(session.lifecycle(), lokai_lsp::LspLifecycle::Stopped);
    assert!(!session.is_healthy());
    session.close().unwrap();
}

fn mock_crash_spec() -> ServerSpec {
    ServerSpec {
        lang: Lang::Rust,
        program: PathBuf::from(env!("CARGO_BIN_EXE_lsp-mock-server")),
        args: vec!["--crash-after-init".into()],
        label: "mock-crash".into(),
    }
}

#[test]
fn mock_goto_definition_and_diagnostics() {
    command_launcher::install_command_launcher();
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();

    let mut session = LspSession::spawn(mock_spec(), dir.path()).unwrap();
    assert!(session.is_healthy());
    assert_eq!(session.lifecycle(), lokai_lsp::LspLifecycle::Ready);

    let hits = session.goto_definition("main.rs", 1, 0).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "main.rs");

    let diags = session.diagnostics("main.rs").unwrap();
    assert!(!diags.is_empty());
    assert_eq!(diags[0].message, "mock error");
}

#[test]
fn did_change_after_disk_edit() {
    command_launcher::install_command_launcher();
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("main.rs"), "fn old() {}\n").unwrap();

    let mut session = LspSession::spawn(mock_spec(), dir.path()).unwrap();
    session.goto_definition("main.rs", 1, 0).unwrap();

    std::fs::write(dir.path().join("main.rs"), "fn new_name() {}\n").unwrap();
    let hits = session.goto_definition("main.rs", 1, 0).unwrap();
    assert!(!hits.is_empty());
}

#[test]
fn pool_round_trip_via_mock_detect() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
    enable_mock_detect("");

    let pool = LspPool::new(dir.path()).unwrap();
    let hits = pool.goto_definition("main.rs", 1, 0).unwrap();
    assert_eq!(hits[0].line, 1);
}

#[test]
fn pool_respawns_after_unhealthy_session() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
    enable_mock_detect("");

    let pool = LspPool::new(dir.path()).unwrap();
    let bad = LspSession::spawn(mock_crash_spec(), dir.path()).unwrap();
    for _ in 0..100 {
        if !bad.is_healthy() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(!bad.is_healthy());
    pool.inject_session_for_test(Lang::Rust, bad);

    let hits = pool
        .goto_definition("main.rs", 1, 0)
        .expect("pool should respawn healthy session");
    assert_eq!(hits[0].path, "main.rs");
}

#[test]
fn lifecycle_fsm_invalid_transition() {
    use lokai_lsp::LspLifecycle;
    assert!(LspLifecycle::NotStarted
        .transition(LspLifecycle::Ready)
        .is_err());
}

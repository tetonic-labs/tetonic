//! Test-only Command-based LSP launcher (integration tests; not production).

use std::cell::RefCell;
use std::io::{BufReader, BufWriter};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;

use tetonic_lsp::{
    spawned_from_io, LspError, LspProcessHandle, LspProcessLauncher, ServerSpec, SpawnedLspProcess,
};

struct StdChildHandle(RefCell<std::process::Child>);

impl LspProcessHandle for StdChildHandle {
    fn is_alive(&self) -> bool {
        matches!(self.0.borrow_mut().try_wait(), Ok(None))
    }

    fn stop(&mut self) {
        let mut child = self.0.borrow_mut();
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn apply_minimal_env(cmd: &mut Command) {
    use std::collections::HashMap;
    const ALLOW: &[&str] = &[
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
        "TERM",
        "TMP",
        "TEMP",
        "TMPDIR",
    ];
    let keep: HashMap<String, String> = std::env::vars()
        .filter(|(k, _)| ALLOW.iter().any(|a| k.eq_ignore_ascii_case(a)))
        .collect();
    cmd.env_clear();
    for (k, v) in keep {
        cmd.env(k, v);
    }
}

pub struct CommandLspLauncher;

impl LspProcessLauncher for CommandLspLauncher {
    fn spawn(&self, spec: &ServerSpec, root: &Path) -> Result<SpawnedLspProcess, LspError> {
        let root = std::fs::canonicalize(root)?;
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .current_dir(&root);
        apply_minimal_env(&mut cmd);
        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        Ok(spawned_from_io(
            Box::new(BufWriter::new(stdin)),
            Box::new(BufReader::new(stdout)),
            Box::new(StdChildHandle(RefCell::new(child))),
        ))
    }
}

pub fn install_command_launcher() {
    tetonic_lsp::set_process_launcher(Arc::new(CommandLspLauncher));
}

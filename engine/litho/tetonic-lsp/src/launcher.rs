//! Pluggable process launcher for LSP subprocesses (M2-3 / R29 long-lived sandbox path).

use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;

use crate::detect::ServerSpec;
use crate::LspError;

/// Handle for a spawned LSP subprocess (sandbox-backed in production).
pub trait LspProcessHandle: Send {
    fn is_alive(&self) -> bool;
    /// Stop and reap the owned process promptly, releasing both IO endpoints so
    /// transport workers can exit. Implementations must bound this operation;
    /// the session's worker-join deadline cannot interrupt a blocking stop call.
    fn stop(&mut self);
}

/// IO endpoints for an LSP language-server subprocess.
pub struct SpawnedLspProcess {
    pub stdin: Box<dyn Write + Send>,
    pub stdout: Box<dyn Read + Send>,
    handle: Box<dyn LspProcessHandle>,
}

impl SpawnedLspProcess {
    pub fn is_alive(&self) -> bool {
        self.handle.is_alive()
    }

    pub fn stop(&mut self) {
        self.handle.stop();
    }
}

/// Spawns language-server subprocesses. Production installs
/// [`tetonic_app::lsp_launcher::SandboxLspLauncher`] via [`set_process_launcher`].
pub trait LspProcessLauncher: Send + Sync {
    fn spawn(&self, spec: &ServerSpec, root: &Path) -> Result<SpawnedLspProcess, LspError>;
}

use std::sync::RwLock;

static GLOBAL_LAUNCHER: RwLock<Option<Arc<dyn LspProcessLauncher>>> = RwLock::new(None);

/// Install a global custom launcher (thread-safe, OS sandbox long-lived path).
pub fn set_process_launcher(launcher: Arc<dyn LspProcessLauncher>) {
    if let Ok(mut slot) = GLOBAL_LAUNCHER.write() {
        *slot = Some(launcher);
    }
}

pub fn clear_process_launcher() {
    if let Ok(mut slot) = GLOBAL_LAUNCHER.write() {
        *slot = None;
    }
}

#[allow(dead_code)]
pub(crate) fn spawn_process(spec: &ServerSpec, root: &Path) -> Result<SpawnedLspProcess, LspError> {
    spawn_process_with_launcher(spec, root, None)
}

pub(crate) fn spawn_process_with_launcher(
    spec: &ServerSpec,
    root: &Path,
    launcher_override: Option<&dyn LspProcessLauncher>,
) -> Result<SpawnedLspProcess, LspError> {
    if let Some(launcher) = launcher_override {
        return launcher.spawn(spec, root);
    }
    if let Ok(slot) = GLOBAL_LAUNCHER.read() {
        if let Some(launcher) = slot.as_ref() {
            return launcher.spawn(spec, root);
        }
    }
    Err(LspError::Server(
        "LSP process launcher not configured (R29: production uses SandboxLspLauncher via lokai-app)".into(),
    ))
}

/// Wrap custom IO endpoints with a handle (used by sandbox launcher in lokai-app).
pub fn spawned_from_io(
    stdin: Box<dyn Write + Send>,
    stdout: Box<dyn Read + Send>,
    handle: Box<dyn LspProcessHandle>,
) -> SpawnedLspProcess {
    SpawnedLspProcess {
        stdin,
        stdout,
        handle,
    }
}

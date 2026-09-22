//! Sandbox-backed LSP process launcher (M2-3 long-lived interface).
//! Moved out of `lokai-tools` so the generic loop has no `lokai-lsp` edge (M9).

use std::path::Path;
use std::sync::{Arc, Mutex};

use tetonic_domain::execution::ProcessClass;
use tetonic_lsp::{
    spawned_from_io, LspError, LspProcessHandle, LspProcessLauncher, ServerSpec, SpawnedLspProcess,
};
use tetonic_sandbox::{apply_executable, profile_for_class, ProcessMode, SyncLongLivedService};
use tetonic_tools::process_executor::ProcessExecutor;
use tetonic_tools::sandbox_bridge::{format_sandbox_audit, process_class_for_lsp};

struct SandboxServiceHandle {
    service: Mutex<SyncLongLivedService>,
}

impl LspProcessHandle for SandboxServiceHandle {
    fn is_alive(&self) -> bool {
        self.service.lock().map(|s| s.is_alive()).unwrap_or(false)
    }

    fn stop(&mut self) {
        if let Ok(mut guard) = self.service.lock() {
            let _ = guard.force_terminate();
        }
    }
}

pub struct SandboxLspLauncher {
    executor: ProcessExecutor,
}

impl SandboxLspLauncher {
    pub fn new(executor: ProcessExecutor) -> Arc<Self> {
        Arc::new(Self { executor })
    }
}

impl LspProcessLauncher for SandboxLspLauncher {
    fn spawn(&self, spec: &ServerSpec, root: &Path) -> Result<SpawnedLspProcess, LspError> {
        self.executor.ensure_runnable().map_err(LspError::Server)?;
        let root = std::fs::canonicalize(root).map_err(LspError::Io)?;

        if !self.executor.uses_os_sandbox() {
            return Err(LspError::Server(
                "sandbox LSP launcher requires Sandboxed enforcement".into(),
            ));
        }

        let execution_id = format!("lsp_{}_{}", spec.label.replace(' ', "_"), new_exec_suffix());
        self.executor
            .note_lsp_service_start(&spec.program.display().to_string())
            .map_err(LspError::Server)?;
        let mut req = profile_for_class(process_class_for_lsp(), &root);
        req.mode = ProcessMode::LongLived;
        req = apply_executable(req, &spec.program.display().to_string(), &spec.args);
        req.process_class = ProcessClass::InternalService;

        let service = SyncLongLivedService::spawn_with_execution_id(req, execution_id.clone())
            .map_err(|e| LspError::Server(format!("LSP sandbox spawn failed: {e}")))?;

        let audit = format_sandbox_audit(&service.report);
        tracing::info!(
            "tetonic_sandbox: lsp_spawn execution_id={} program={} label={} audit={}",
            execution_id,
            spec.program.display(),
            spec.label,
            audit
        );

        let (stdin, stdout, service) = service.take_stdio();
        let handle = SandboxServiceHandle {
            service: Mutex::new(service),
        };
        Ok(spawned_from_io(stdin, stdout, Box::new(handle)))
    }
}

fn new_exec_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tetonic_tools::process_executor::{coding_executor, EnforcementLevel};

    #[test]
    fn sandbox_launcher_spawns_and_stop_kills() {
        let dir = std::env::temp_dir().join(format!("lsp-sandbox-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);

        let pe = coding_executor(&dir, EnforcementLevel::Sandboxed);
        let launcher = SandboxLspLauncher::new(pe);
        #[cfg(windows)]
        let (program, args) = {
            let comspec = std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".into());
            (
                PathBuf::from(comspec),
                vec![
                    "/C".into(),
                    "ping".into(),
                    "-n".into(),
                    "60".into(),
                    "127.0.0.1".into(),
                ],
            )
        };
        #[cfg(not(windows))]
        let (program, args) = (PathBuf::from("sleep"), vec!["60".into()]);
        let spec = ServerSpec {
            lang: tetonic_lsp::Lang::Rust,
            program,
            args,
            label: "long-lived-probe".into(),
        };

        let mut spawned = launcher
            .spawn(&spec, &dir)
            .expect("sandbox long-lived spawn");
        assert!(spawned.is_alive(), "child must be alive after spawn");
        spawned.stop();
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(!spawned.is_alive(), "stop must terminate long-lived child");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

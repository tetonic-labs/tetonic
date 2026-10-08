//! Application host composition. No work planning or execution authority lives here.
mod bindings;
mod composition;
mod config;
pub(crate) mod initialization;
#[cfg(test)]
mod tests;

pub use bindings::HostServices;
pub use config::{HostConfiguration, StorageConfiguration};

use crate::resources::LocalControl;
use crate::services::InitializationService;
use crate::{commands::InitializeCommand, errors::AppError, events::NoopEventSink};
use crate::{Application, ComputePlaneRequest};
use std::{
    net::IpAddr,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Ready application and its credential/control adapter, sharing one durable store.
/// Dropping this handle does not cancel work held by another application reference.
pub struct ApplicationHost {
    pub app: Arc<Application>,
    pub control: LocalControl,
}

impl ApplicationHost {
    pub async fn open(
        database: PathBuf,
        audience: String,
        workspace: Option<PathBuf>,
        ollama: String,
        config: HostConfiguration,
    ) -> Result<Self, AppError> {
        config.validate()?;
        let control = LocalControl::open_with_read_connections(
            database,
            audience,
            config.storage.read_connections,
        )
        .await
        .map_err(|e| AppError::PersistenceFailed(e.to_string()))?;
        Self::from_control(control, workspace, ollama, config).await
    }

    pub(crate) async fn from_control(
        control: LocalControl,
        workspace: Option<PathBuf>,
        ollama: String,
        config: HostConfiguration,
    ) -> Result<Self, AppError> {
        config.validate()?;
        let store = control.store().clone();
        // This fallback only locates runtime artifacts/policy. It never grants
        // file tools; RegisteredExecutionSettings retains the explicit folder.
        let workspace = runtime_workspace(store.path(), workspace);
        let bootstrap_store = store.clone();
        let bootstrap_workspace = workspace.clone();
        let app = tokio::task::spawn_blocking(move || {
            let init = Arc::new(
                initialization::DefaultInitializationService::with_artifact_root(
                    config.storage.artifact_directory,
                ),
            );
            let bootstrap = init.bootstrap_runtime(
                &InitializeCommand {
                    workspace_root: bootstrap_workspace.display().to_string(),
                    rpc_token_provided: false,
                    rpc_auth_disabled: true,
                },
                &Some(bootstrap_store.clone()),
            )?;
            Ok::<_, AppError>(Arc::new(Application::from_bootstrap(
                init,
                &bootstrap,
                Some(bootstrap_store),
                Arc::new(NoopEventSink),
                None,
            )))
        })
        .await
        .map_err(|_| AppError::PersistenceFailed("host initialization failed".into()))??;
        let guard = match loopback_port(&ollama) {
            Some(port) => Arc::new(tetonic_egress::EgressGuard::loopback_inference(port)),
            None => Arc::new(tetonic_egress::EgressGuard::new()),
        };
        let plane = crate::build_compute_plane(ComputePlaneRequest {
            guard: guard.clone(),
            ollama_base: ollama.clone(),
            policy: app.host.policy.clone(),
            workspace_root: workspace,
            artifact_store: app.host.runtime.artifact_store().clone(),
            store: Some(store),
            coordinator: None,
            placement_sink: None,
            previous_pooled: None,
        })
        .await;
        app.install_compute_services(&plane);
        app.attach_egress(guard, ollama);
        Ok(Self { app, control })
    }
}

fn runtime_workspace(database: &Path, explicit: Option<PathBuf>) -> PathBuf {
    explicit.unwrap_or_else(|| {
        database
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(std::env::temp_dir)
    })
}

fn loopback_port(ollama: &str) -> Option<u16> {
    let rest = ollama
        .strip_prefix("http://")
        .or_else(|| ollama.strip_prefix("https://"))?;
    let (host, port) = rest.split('/').next()?.rsplit_once(':')?;
    let ip = host.parse::<IpAddr>().ok();
    let loopback = host == "localhost" || ip.is_some_and(|ip| ip.is_loopback());
    loopback.then(|| port.parse().ok()).flatten()
}

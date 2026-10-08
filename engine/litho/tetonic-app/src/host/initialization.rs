//! Runtime bootstrap shared by production hosts and application adapters.
use crate::{commands::*, errors::AppError};
use async_trait::async_trait;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Slice 1 — Initialization
// ---------------------------------------------------------------------------

#[async_trait]
pub trait InitializationService: Send + Sync {
    async fn initialize_workspace(
        &self,
        cmd: InitializeCommand,
    ) -> Result<InitializeResultPayload, AppError>;
    fn bootstrap_runtime(
        &self,
        cmd: &InitializeCommand,
        store: &Option<tetonic_memory::SharedStore>,
    ) -> Result<InitializeBootstrapPayload, AppError>;
}

pub struct DefaultInitializationService {
    artifact_root: Option<std::path::PathBuf>,
}

impl Default for DefaultInitializationService {
    fn default() -> Self {
        Self::new()
    }
}

impl DefaultInitializationService {
    pub fn with_artifact_root(artifact_root: Option<std::path::PathBuf>) -> Self {
        Self { artifact_root }
    }

    pub fn new() -> Self {
        Self {
            artifact_root: None,
        }
    }
}

#[async_trait]
impl InitializationService for DefaultInitializationService {
    async fn initialize_workspace(
        &self,
        cmd: InitializeCommand,
    ) -> Result<InitializeResultPayload, AppError> {
        let path = std::path::Path::new(&cmd.workspace_root)
            .canonicalize()
            .map_err(|e| AppError::InvalidRequest(format!("workspace: {}", e)))?;
        Ok(InitializeResultPayload {
            workspace_root: path.display().to_string(),
            index_db_path: None,
        })
    }

    fn bootstrap_runtime(
        &self,
        cmd: &InitializeCommand,
        store: &Option<tetonic_memory::SharedStore>,
    ) -> Result<InitializeBootstrapPayload, AppError> {
        let path = std::path::Path::new(&cmd.workspace_root)
            .canonicalize()
            .map_err(|e| AppError::InvalidRequest(format!("workspace: {}", e)))?;
        let policy = match store {
            Some(s) => s
                .read_sync(|db| tetonic_runtime::load_policy_engine(Some(db)))
                .map_err(|e| AppError::PersistenceFailed(format!("policy load: {e}")))?,
            None => tetonic_runtime::load_policy_engine(None),
        };
        let local_artifacts = tetonic_artifact::LocalArtifactStore::new(
            self.artifact_root
                .clone()
                .unwrap_or_else(|| path.join(".lokai").join("artifacts")),
            crate::secret_scanner_factory::artifact_scan_policy(store),
        )
        .map_err(|e| AppError::PersistenceFailed(format!("artifact store: {e}")))?;
        // Shared startup applies artifact retention before any agent executes.
        if let Err(e) =
            tetonic_artifact::enforce_at_startup(&local_artifacts, local_artifacts.quota())
        {
            tracing::warn!("artifact GC at bootstrap: {e}");
        }
        let artifact_store = Arc::new(local_artifacts);

        let runtime = Arc::new(
            tetonic_runtime::EngineRuntime::new_with_capability_store(
                policy.clone(),
                None,
                artifact_store,
                store.as_ref().map(|s| Arc::new(s.clone())),
            )
            .map_err(|e| AppError::PersistenceFailed(format!("capability store: {e}")))?,
        );
        let inference_defaults = match store {
            Some(s) => s
                .read_sync(|db| {
                    tetonic_capacity::load_inference_defaults(db, tetonic_capacity::LOCAL_NODE_ID)
                })
                .unwrap_or_else(|_| tetonic_capacity::InferenceDefaults::fallback()),
            None => tetonic_capacity::InferenceDefaults::fallback(),
        };
        Ok(InitializeBootstrapPayload {
            workspace_root: path.display().to_string(),
            policy,
            runtime,
            inference_defaults,
        })
    }
}

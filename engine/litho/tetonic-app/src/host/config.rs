//! Supported options for the current local host, separate from job permissions.
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tetonic_telemetry::host::{LoggingConfig, TraceConfig};

use crate::errors::AppError;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct HostConfiguration {
    pub storage: StorageConfiguration,
    pub logging: LoggingConfig,
    pub telemetry: TraceConfig,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct StorageConfiguration {
    /// Initial read connections sharing the same durable writer.
    pub read_connections: usize,
    /// When absent, preserve the existing artifact location for compatibility.
    pub artifact_directory: Option<PathBuf>,
}

impl Default for StorageConfiguration {
    fn default() -> Self {
        Self {
            read_connections: 2,
            artifact_directory: None,
        }
    }
}

impl HostConfiguration {
    pub fn from_json(bytes: &[u8]) -> Result<Self, AppError> {
        let config: Self = serde_json::from_slice(bytes)
            .map_err(|_| AppError::InvalidRequest("invalid host configuration".into()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), AppError> {
        if !(1..=16).contains(&self.storage.read_connections) {
            return Err(AppError::InvalidRequest(
                "storage.read_connections must be between 1 and 16".into(),
            ));
        }
        for path in [&self.storage.artifact_directory, &self.logging.directory]
            .into_iter()
            .flatten()
        {
            if path.as_os_str().is_empty() {
                return Err(AppError::InvalidRequest(
                    "host storage/logging paths must not be empty".into(),
                ));
            }
        }
        self.telemetry
            .validate()
            .map_err(|message| AppError::InvalidRequest(message.into()))
    }

    /// Operator adapters resolve paths once, relative to the config file, so
    /// launch behavior does not depend on the process working directory.
    pub fn resolve_relative_paths(&mut self, base: &Path) {
        for path in [
            &mut self.storage.artifact_directory,
            &mut self.logging.directory,
        ]
        .into_iter()
        .flatten()
        {
            if path.is_relative() {
                *path = base.join(&*path);
            }
        }
    }
}

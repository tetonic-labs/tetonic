//! Declarative Engine Configuration (`tetonic.toml`) and Node Invariants (SAE-401).
//!
//! Provides the canonical configuration model supporting the "1-to-1,000 Scale Invariant",
//! allowing the identical engine binary to run zero-config standalone on a laptop or
//! distributed across a 1,000-node cluster.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Operating role/mode of an engine node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeMode {
    /// Local desktop mode: embedded coordinator, local runner, and local SQLite engine.
    Standalone,
    /// Cluster Keeper: manages cluster metadata, lease proofs, topologies, and failover.
    Coordinator,
    /// Cluster Worker: executes continuous cognitive loops and streams heartbeats.
    Runner,
}

impl Default for NodeMode {
    fn default() -> Self {
        Self::Standalone
    }
}

impl std::fmt::Display for NodeMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Standalone => write!(f, "standalone"),
            Self::Coordinator => write!(f, "coordinator"),
            Self::Runner => write!(f, "runner"),
        }
    }
}

impl std::str::FromStr for NodeMode {
    type Err = EngineConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "standalone" => Ok(Self::Standalone),
            "coordinator" | "keeper" => Ok(Self::Coordinator),
            "runner" | "worker" => Ok(Self::Runner),
            other => Err(EngineConfigError::InvalidValue {
                field: "node.mode".to_string(),
                value: other.to_string(),
            }),
        }
    }
}

/// Backing storage engine mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageMode {
    /// Local SQLite database.
    LocalSqlite,
    /// Distributed database cluster.
    DistributedDb,
    /// Cloud persistent volume claim (PVC) with atomic state checkpoints.
    MoveableVolume,
}

impl Default for StorageMode {
    fn default() -> Self {
        Self::LocalSqlite
    }
}

impl std::fmt::Display for StorageMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LocalSqlite => write!(f, "local_sqlite"),
            Self::DistributedDb => write!(f, "distributed_db"),
            Self::MoveableVolume => write!(f, "moveable_volume"),
        }
    }
}

impl std::str::FromStr for StorageMode {
    type Err = EngineConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "localsqlite" | "local_sqlite" | "sqlite" => Ok(Self::LocalSqlite),
            "distributeddb" | "distributed_db" | "distributed" => Ok(Self::DistributedDb),
            "moveablevolume" | "moveable_volume" | "volume" | "pvc" => Ok(Self::MoveableVolume),
            other => Err(EngineConfigError::InvalidValue {
                field: "storage.storage_mode".to_string(),
                value: other.to_string(),
            }),
        }
    }
}

/// Upstream inference provider type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceProviderKind {
    /// Local resident inference server (e.g. Ollama or embedded model).
    Local,
    /// Decoupled stateless GPU inference fabric (vLLM, TensorRT-LLM, remote cluster pool).
    RemoteFabric,
    /// Hosted cloud provider API.
    Cloud,
}

impl Default for InferenceProviderKind {
    fn default() -> Self {
        Self::Local
    }
}

impl std::fmt::Display for InferenceProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local => write!(f, "local"),
            Self::RemoteFabric => write!(f, "remote_fabric"),
            Self::Cloud => write!(f, "cloud"),
        }
    }
}

impl std::str::FromStr for InferenceProviderKind {
    type Err = EngineConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "remotefabric" | "remote_fabric" | "fabric" => Ok(Self::RemoteFabric),
            "cloud" => Ok(Self::Cloud),
            other => Err(EngineConfigError::InvalidValue {
                field: "inference.provider".to_string(),
                value: other.to_string(),
            }),
        }
    }
}

/// Configuration for node identification and cluster networking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeConfig {
    pub id: String,
    pub mode: NodeMode,
    pub bind_addr: String,
    pub coordinator_url: Option<String>,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            id: "node-standalone-0".to_string(),
            mode: NodeMode::Standalone,
            bind_addr: "127.0.0.1:4430".to_string(),
            coordinator_url: None,
        }
    }
}

/// Configuration for state persistence and moveable volume checkpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageConfig {
    pub storage_mode: StorageMode,
    pub volume_mount_path: PathBuf,
    pub checkpoint_interval_secs: u64,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            storage_mode: StorageMode::LocalSqlite,
            volume_mount_path: PathBuf::from("./data"),
            checkpoint_interval_secs: 60,
        }
    }
}

/// Configuration for decoupled token generation and inference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InferenceConfig {
    pub provider: InferenceProviderKind,
    pub endpoint_url: Option<String>,
    pub default_model: String,
    pub reflex_model: Option<String>,
    pub timeout_ms: u64,
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            provider: InferenceProviderKind::Local,
            endpoint_url: Some("http://127.0.0.1:11434".to_string()),
            default_model: "default-model".to_string(),
            reflex_model: Some("reflex-model".to_string()),
            timeout_ms: 30_000,
        }
    }
}

/// Upstream telemetry and trace sink type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetrySinkKind {
    /// In-memory ring buffer (default for Web UI) and stdout/stderr.
    Embedded,
    /// Universal OpenTelemetry Protocol (OTLP) gRPC/HTTP exporter.
    Otlp,
    /// Local append-only JSON-lines log file.
    File,
}

impl Default for TelemetrySinkKind {
    fn default() -> Self {
        Self::Embedded
    }
}

impl std::fmt::Display for TelemetrySinkKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Embedded => write!(f, "embedded"),
            Self::Otlp => write!(f, "otlp"),
            Self::File => write!(f, "file"),
        }
    }
}

impl std::str::FromStr for TelemetrySinkKind {
    type Err = EngineConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "embedded" | "stdout" | "memory" => Ok(Self::Embedded),
            "otlp" | "opentelemetry" => Ok(Self::Otlp),
            "file" | "jsonl" => Ok(Self::File),
            other => Err(EngineConfigError::InvalidValue {
                field: "telemetry.sink".to_string(),
                value: other.to_string(),
            }),
        }
    }
}

/// Configuration for real-time telemetry, thought stream broadcasting, and trace export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelemetryConfig {
    pub sink: TelemetrySinkKind,
    pub endpoint_url: Option<String>,
    pub ring_buffer_capacity: usize,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            sink: TelemetrySinkKind::Embedded,
            endpoint_url: None,
            ring_buffer_capacity: 256,
        }
    }
}

/// Top-level declarative engine configuration (`tetonic.toml`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EngineConfig {
    pub node: NodeConfig,
    pub storage: StorageConfig,
    pub inference: InferenceConfig,
    pub telemetry: TelemetryConfig,
}

#[derive(Debug, Error)]
pub enum EngineConfigError {
    #[error("I/O error reading engine configuration: {0}")]
    Io(#[from] std::io::Error),
    #[error("Failed to parse TOML configuration on line {line}: {detail}")]
    ParseError { line: usize, detail: String },
    #[error("Invalid value '{value}' for field '{field}'")]
    InvalidValue { field: String, value: String },
    #[error("Missing required configuration section '{0}'")]
    MissingSection(String),
}

impl EngineConfig {
    /// Constructs zero-config desktop defaults with optional environment variable overrides.
    pub fn desktop_default() -> Self {
        let mut cfg = Self::default();
        cfg.apply_env_overrides();
        cfg
    }

    /// Parses a `tetonic.toml` document from string.
    pub fn from_toml_str(toml_str: &str) -> Result<Self, EngineConfigError> {
        let mut sections: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut current_section = String::new();

        for (idx, line) in toml_str.lines().enumerate() {
            let line_no = idx + 1;
            let trimmed = line.trim();

            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Strip trailing comments
            let content = if let Some(hash_pos) = trimmed.find('#') {
                trimmed[..hash_pos].trim()
            } else {
                trimmed
            };

            if content.starts_with('[') && content.ends_with(']') {
                let sec_name = content[1..content.len() - 1].trim().to_lowercase();
                current_section = sec_name;
                sections.entry(current_section.clone()).or_default();
            } else if let Some((key, val)) = content.split_once('=') {
                let k = key.trim().to_lowercase();
                let v = val.trim();
                let unquoted = unquote_str(v);
                if current_section.is_empty() {
                    return Err(EngineConfigError::ParseError {
                        line: line_no,
                        detail: format!("Key '{}' found outside of any section header", k),
                    });
                }
                sections
                    .entry(current_section.clone())
                    .or_default()
                    .insert(k, unquoted);
            }
        }

        let mut config = Self::default();

        if let Some(node_sec) = sections.get("node") {
            if let Some(val) = node_sec.get("id") {
                config.node.id = val.clone();
            }
            if let Some(val) = node_sec.get("mode") {
                config.node.mode = val.parse()?;
            }
            if let Some(val) = node_sec.get("bind_addr") {
                config.node.bind_addr = val.clone();
            }
            config.node.coordinator_url = node_sec.get("coordinator_url").and_then(|val| {
                if val.is_empty() || val == "none" {
                    None
                } else {
                    Some(val.clone())
                }
            });
        }

        if let Some(storage_sec) = sections.get("storage") {
            if let Some(val) = storage_sec.get("storage_mode") {
                config.storage.storage_mode = val.parse()?;
            }
            if let Some(val) = storage_sec.get("volume_mount_path") {
                config.storage.volume_mount_path = PathBuf::from(val);
            }
            if let Some(val) = storage_sec.get("checkpoint_interval_secs") {
                config.storage.checkpoint_interval_secs = val.parse().map_err(|_| {
                    EngineConfigError::InvalidValue {
                        field: "storage.checkpoint_interval_secs".to_string(),
                        value: val.clone(),
                    }
                })?;
            }
        }

        if let Some(inf_sec) = sections.get("inference") {
            if let Some(val) = inf_sec.get("provider") {
                config.inference.provider = val.parse()?;
            }
            config.inference.endpoint_url = inf_sec.get("endpoint_url").and_then(|val| {
                if val.is_empty() || val == "none" {
                    None
                } else {
                    Some(val.clone())
                }
            });
            if let Some(val) = inf_sec.get("default_model") {
                config.inference.default_model = val.clone();
            }
            config.inference.reflex_model = inf_sec.get("reflex_model").and_then(|val| {
                if val.is_empty() || val == "none" {
                    None
                } else {
                    Some(val.clone())
                }
            });
            if let Some(val) = inf_sec.get("timeout_ms") {
                config.inference.timeout_ms = val.parse().map_err(|_| {
                    EngineConfigError::InvalidValue {
                        field: "inference.timeout_ms".to_string(),
                        value: val.clone(),
                    }
                })?;
            }
        }

        if let Some(telem_sec) = sections.get("telemetry") {
            if let Some(val) = telem_sec.get("sink") {
                config.telemetry.sink = val.parse()?;
            }
            config.telemetry.endpoint_url = telem_sec.get("endpoint_url").and_then(|val| {
                if val.is_empty() || val == "none" {
                    None
                } else {
                    Some(val.clone())
                }
            });
            if let Some(val) = telem_sec.get("ring_buffer_capacity") {
                config.telemetry.ring_buffer_capacity = val.parse().map_err(|_| {
                    EngineConfigError::InvalidValue {
                        field: "telemetry.ring_buffer_capacity".to_string(),
                        value: val.clone(),
                    }
                })?;
            }
        }

        Ok(config)
    }

    /// Loads engine configuration from an explicit file path.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, EngineConfigError> {
        let content = std::fs::read_to_string(path)?;
        let mut config = Self::from_toml_str(&content)?;
        config.apply_env_overrides();
        Ok(config)
    }

    /// Loads configuration from an explicit path, or falls back to `./tetonic.toml`,
    /// or returns default desktop configuration if neither exists.
    pub fn load_or_default(explicit_path: Option<&Path>) -> Result<Self, EngineConfigError> {
        if let Some(p) = explicit_path {
            return Self::from_file(p);
        }

        let local_toml = Path::new("tetonic.toml");
        if local_toml.exists() {
            return Self::from_file(local_toml);
        }

        Ok(Self::desktop_default())
    }

    /// Applies environment variable overrides, giving environment configuration
    /// highest precedence over file and defaults.
    pub fn apply_env_overrides(&mut self) {
        if let Ok(val) = std::env::var("TETONIC_NODE_ID") {
            if !val.trim().is_empty() {
                self.node.id = val;
            }
        }
        if let Ok(val) = std::env::var("TETONIC_NODE_MODE") {
            if let Ok(mode) = val.parse() {
                self.node.mode = mode;
            }
        }
        if let Ok(val) = std::env::var("TETONIC_BIND_ADDR") {
            if !val.trim().is_empty() {
                self.node.bind_addr = val;
            }
        }
        if let Ok(val) = std::env::var("TETONIC_COORDINATOR_URL") {
            if !val.trim().is_empty() {
                self.node.coordinator_url = Some(val);
            }
        }

        if let Ok(val) = std::env::var("TETONIC_STORAGE_MODE") {
            if let Ok(mode) = val.parse() {
                self.storage.storage_mode = mode;
            }
        }
        if let Ok(val) = std::env::var("TETONIC_VOLUME_PATH").or_else(|_| std::env::var("TETONIC_STORAGE_PATH")) {
            if !val.trim().is_empty() {
                self.storage.volume_mount_path = PathBuf::from(val);
            }
        }
        if let Ok(val) = std::env::var("TETONIC_CHECKPOINT_INTERVAL_SECS") {
            if let Ok(secs) = val.parse() {
                self.storage.checkpoint_interval_secs = secs;
            }
        }

        if let Ok(val) = std::env::var("TETONIC_INFERENCE_PROVIDER") {
            if let Ok(kind) = val.parse() {
                self.inference.provider = kind;
            }
        }
        if let Ok(val) = std::env::var("TETONIC_INFERENCE_ENDPOINT") {
            if !val.trim().is_empty() {
                self.inference.endpoint_url = Some(val);
            }
        }
        if let Ok(val) = std::env::var("TETONIC_DEFAULT_MODEL") {
            if !val.trim().is_empty() {
                self.inference.default_model = val;
            }
        }
        if let Ok(val) = std::env::var("TETONIC_REFLEX_MODEL") {
            if !val.trim().is_empty() {
                self.inference.reflex_model = Some(val);
            }
        }
        if let Ok(val) = std::env::var("TETONIC_INFERENCE_TIMEOUT_MS") {
            if let Ok(ms) = val.parse() {
                self.inference.timeout_ms = ms;
            }
        }

        if let Ok(val) = std::env::var("TETONIC_TELEMETRY_SINK") {
            if let Ok(sink) = val.parse() {
                self.telemetry.sink = sink;
            }
        }
        if let Ok(val) = std::env::var("TETONIC_TELEMETRY_ENDPOINT") {
            if !val.trim().is_empty() {
                self.telemetry.endpoint_url = Some(val);
            }
        }
        if let Ok(val) = std::env::var("TETONIC_TELEMETRY_BUFFER_SIZE") {
            if let Ok(cap) = val.parse() {
                self.telemetry.ring_buffer_capacity = cap;
            }
        }
    }

    /// Formats the configuration into canonical TOML text.
    pub fn to_toml_string(&self) -> String {
        let mut out = String::new();

        out.push_str("[node]\n");
        out.push_str(&format!("id = \"{}\"\n", self.node.id));
        out.push_str(&format!("mode = \"{}\"\n", self.node.mode));
        out.push_str(&format!("bind_addr = \"{}\"\n", self.node.bind_addr));
        if let Some(coord) = &self.node.coordinator_url {
            out.push_str(&format!("coordinator_url = \"{}\"\n", coord));
        }

        out.push_str("\n[storage]\n");
        out.push_str(&format!("storage_mode = \"{}\"\n", self.storage.storage_mode));
        out.push_str(&format!(
            "volume_mount_path = \"{}\"\n",
            self.storage.volume_mount_path.to_string_lossy().replace('\\', "/")
        ));
        out.push_str(&format!(
            "checkpoint_interval_secs = {}\n",
            self.storage.checkpoint_interval_secs
        ));

        out.push_str("\n[inference]\n");
        out.push_str(&format!("provider = \"{}\"\n", self.inference.provider));
        if let Some(endpoint) = &self.inference.endpoint_url {
            out.push_str(&format!("endpoint_url = \"{}\"\n", endpoint));
        }
        out.push_str(&format!("default_model = \"{}\"\n", self.inference.default_model));
        if let Some(reflex) = &self.inference.reflex_model {
            out.push_str(&format!("reflex_model = \"{}\"\n", reflex));
        }
        out.push_str(&format!("timeout_ms = {}\n", self.inference.timeout_ms));

        out.push_str("\n[telemetry]\n");
        out.push_str(&format!("sink = \"{}\"\n", self.telemetry.sink));
        if let Some(endpoint) = &self.telemetry.endpoint_url {
            out.push_str(&format!("endpoint_url = \"{}\"\n", endpoint));
        }
        out.push_str(&format!(
            "ring_buffer_capacity = {}\n",
            self.telemetry.ring_buffer_capacity
        ));

        out
    }
}

fn unquote_str(s: &str) -> String {
    let t = s.trim();
    if (t.starts_with('"') && t.ends_with('"')) || (t.starts_with('\'') && t.ends_with('\'')) {
        if t.len() >= 2 {
            return t[1..t.len() - 1].to_string();
        }
    }
    t.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_desktop_default_config() {
        let cfg = EngineConfig::default();
        assert_eq!(cfg.node.mode, NodeMode::Standalone);
        assert_eq!(cfg.storage.storage_mode, StorageMode::LocalSqlite);
        assert_eq!(cfg.inference.provider, InferenceProviderKind::Local);
    }

    #[test]
    fn test_from_toml_str_parsing() {
        let sample = r#"
# Sample Tetonic Engine Configuration
[node]
id = "runner-eu-01"
mode = "Runner"
bind_addr = "0.0.0.0:8080"
coordinator_url = "https://keeper.tetonic.internal:4430"

[storage]
storage_mode = "MoveableVolume"
volume_mount_path = "/mnt/agent-pvc"
checkpoint_interval_secs = 30

[inference]
provider = "RemoteFabric"
endpoint_url = "http://inference-fabric.internal:8000"
default_model = "deliberative-base"
reflex_model = "reflexive-fast"
timeout_ms = 15000
"#;

        let cfg = EngineConfig::from_toml_str(sample).expect("parse failed");
        assert_eq!(cfg.node.id, "runner-eu-01");
        assert_eq!(cfg.node.mode, NodeMode::Runner);
        assert_eq!(cfg.node.bind_addr, "0.0.0.0:8080");
        assert_eq!(
            cfg.node.coordinator_url.as_deref(),
            Some("https://keeper.tetonic.internal:4430")
        );

        assert_eq!(cfg.storage.storage_mode, StorageMode::MoveableVolume);
        assert_eq!(cfg.storage.volume_mount_path, PathBuf::from("/mnt/agent-pvc"));
        assert_eq!(cfg.storage.checkpoint_interval_secs, 30);

        assert_eq!(cfg.inference.provider, InferenceProviderKind::RemoteFabric);
        assert_eq!(
            cfg.inference.endpoint_url.as_deref(),
            Some("http://inference-fabric.internal:8000")
        );
        assert_eq!(cfg.inference.default_model, "deliberative-base");
        assert_eq!(cfg.inference.reflex_model.as_deref(), Some("reflexive-fast"));
        assert_eq!(cfg.inference.timeout_ms, 15000);
    }

    #[test]
    fn test_roundtrip_toml_serialization() {
        let original = EngineConfig {
            node: NodeConfig {
                id: "test-node".into(),
                mode: NodeMode::Coordinator,
                bind_addr: "10.0.0.1:4430".into(),
                coordinator_url: None,
            },
            storage: StorageConfig {
                storage_mode: StorageMode::DistributedDb,
                volume_mount_path: PathBuf::from("/var/lib/tetonic"),
                checkpoint_interval_secs: 120,
            },
            inference: InferenceConfig {
                provider: InferenceProviderKind::Cloud,
                endpoint_url: Some("https://api.hosted-inference.com".into()),
                default_model: "large-reasoning".into(),
                reflex_model: None,
                timeout_ms: 45000,
            },
            telemetry: TelemetryConfig {
                sink: TelemetrySinkKind::Otlp,
                endpoint_url: Some("http://otel-collector:4317".into()),
                ring_buffer_capacity: 512,
            },
        };

        let toml_text = original.to_toml_string();
        let parsed = EngineConfig::from_toml_str(&toml_text).expect("roundtrip parse");
        assert_eq!(original, parsed);
    }

    #[test]
    fn test_env_overrides() {
        let mut cfg = EngineConfig::default();
        std::env::set_var("TETONIC_NODE_MODE", "runner");
        std::env::set_var("TETONIC_INFERENCE_ENDPOINT", "http://env-override:9000");
        std::env::set_var("TETONIC_TELEMETRY_SINK", "otlp");

        cfg.apply_env_overrides();

        assert_eq!(cfg.node.mode, NodeMode::Runner);
        assert_eq!(
            cfg.inference.endpoint_url.as_deref(),
            Some("http://env-override:9000")
        );
        assert_eq!(cfg.telemetry.sink, TelemetrySinkKind::Otlp);

        std::env::remove_var("TETONIC_NODE_MODE");
        std::env::remove_var("TETONIC_INFERENCE_ENDPOINT");
        std::env::remove_var("TETONIC_TELEMETRY_SINK");
    }
}

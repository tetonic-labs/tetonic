//! Versioned worker capability document (M5-2).

use chrono::{DateTime, Duration, Utc};
use lokai_domain::ids::WorkerId;
use serde::{Deserialize, Serialize};

use crate::{JobKind, ProtocolVersion, WorkerCapabilityAdvertisement};

/// Maximum serialized capability document size (bytes).
pub const MAX_CAPABILITY_DOCUMENT_BYTES: u32 = 256 * 1024;

/// Default TTL for static capability fields.
pub const DEFAULT_STATIC_CAPABILITY_TTL: Duration = Duration::hours(1);

/// Shorter TTL for dynamic runtime capacity fields.
pub const DEFAULT_DYNAMIC_CAPACITY_TTL: Duration = Duration::seconds(30);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareVersion {
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolRange {
    pub min: ProtocolVersion,
    pub max: ProtocolVersion,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSet {
    #[serde(default)]
    pub features: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionRange {
    pub min: u32,
    pub max: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlSupport {
    Enforced,
    Partial,
    Unsupported,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityEvidenceKind {
    SelfReported,
    WorkerLocallyProbed,
    CoordinatorObserved,
    CoordinatorVerified,
    Attested,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityEvidence {
    pub kind: CapabilityEvidenceKind,
    pub claim: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobCapability {
    pub job_kind: JobKind,
    pub schema_versions: VersionRange,
    pub maximum_input_bytes: u64,
    pub maximum_output_bytes: u64,
    pub maximum_artifacts: u32,
    pub supports_cancellation: bool,
    pub supports_heartbeats: bool,
    pub supports_leases: bool,
    pub supports_streaming: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelLoadState {
    Warm,
    Cold,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelCapability {
    pub local_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantization: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_size_b: Option<f64>,
    #[serde(default)]
    pub max_context_length: u32,
    #[serde(default)]
    pub tool_call_support: bool,
    #[serde(default)]
    pub structured_output_support: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_vram_bytes: Option<u64>,
    #[serde(default = "default_load_state")]
    pub load_state: ModelLoadState,
}

fn default_load_state() -> ModelLoadState {
    ModelLoadState::Unknown
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuDevice {
    pub name: String,
    pub vram_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vram_free_bytes: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_arch: Option<String>,
    #[serde(default)]
    pub logical_cores: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_ram_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_ram_bytes: Option<u64>,
    #[serde(default)]
    pub gpus: Vec<GpuDevice>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuntimeCapacity {
    pub active_jobs: u32,
    pub queued_jobs: u32,
    pub maximum_concurrent_jobs: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_ram_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_vram_bytes: Option<u64>,
    #[serde(default)]
    pub estimated_queue_delay_secs: u64,
    #[serde(default)]
    pub load_score: f32,
    #[serde(default)]
    pub draining: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxCapabilities {
    pub process_tree: ControlSupport,
    pub runtime_limits: ControlSupport,
    pub memory_limits: ControlSupport,
    pub filesystem_read_scope: ControlSupport,
    pub filesystem_write_scope: ControlSupport,
    pub network_denial: ControlSupport,
    pub network_allowlist: ControlSupport,
    pub environment_filtering: ControlSupport,
}

impl Default for SandboxCapabilities {
    fn default() -> Self {
        Self {
            process_tree: ControlSupport::Unsupported,
            runtime_limits: ControlSupport::Unsupported,
            memory_limits: ControlSupport::Unsupported,
            filesystem_read_scope: ControlSupport::Unsupported,
            filesystem_write_scope: ControlSupport::Unsupported,
            network_denial: ControlSupport::Unsupported,
            network_allowlist: ControlSupport::Unsupported,
            environment_filtering: ControlSupport::Unsupported,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerLimits {
    pub max_concurrent_jobs: u32,
    pub max_input_bytes: u64,
    pub max_output_bytes: u64,
}

/// What a worker advertises it can execute.
///
/// A worker is advertised capacity, not a capability: `supported_job_types` is the
/// dimension that says which capabilities it implements. V3 implements WorkerTarget for
/// Infer and Embed only. Process work (verification, shell, index) runs on the
/// LocalTarget under ProcessAuthority, and a process-class job aimed at a worker is
/// refused at the broker dispatch gate (`ComputeBrokerError::WorkerTargetRefused`).
/// Adding a process-class job kind here would need a snapshot-transport protocol first.
/// See INV-EXEC-002, INV-WORKER-001, and M2 for ProcessAuthority.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkerCapabilities {
    pub worker_id: WorkerId,
    pub capability_revision: u64,
    pub generated_at: DateTime<Utc>,
    pub valid_until: DateTime<Utc>,
    pub boot_id: String,
    pub software_version: SoftwareVersion,
    pub protocol_range: ProtocolRange,
    pub supported_features: FeatureSet,
    pub supported_job_types: Vec<JobCapability>,
    pub model_inventory: Vec<ModelCapability>,
    pub hardware: HardwareCapabilities,
    pub runtime_capacity: RuntimeCapacity,
    pub sandbox: SandboxCapabilities,
    pub limits: WorkerLimits,
    pub revocation_epoch: u64,
    #[serde(default)]
    pub evidence: Vec<CapabilityEvidence>,
    /// M5-1 compatibility summary for schedulers that still read the legacy shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_advertisement: Option<WorkerCapabilityAdvertisement>,
}

impl WorkerCapabilities {
    #[allow(clippy::too_many_arguments)]
    pub fn legacy_infer_profile(
        worker_id: WorkerId,
        boot_id: impl Into<String>,
        capability_revision: u64,
        revocation_epoch: u64,
        models: &[String],
        resident_models: &[String],
        vram_total_mb: u32,
        vram_free_mb: u32,
        active_jobs: u32,
        queued_jobs: u32,
        max_concurrent: u32,
    ) -> Self {
        let now = Utc::now();
        let legacy = WorkerCapabilityAdvertisement::legacy_v1_chat_infer_only();
        let resident: std::collections::HashSet<_> = resident_models.iter().collect();
        let model_inventory = models
            .iter()
            .map(|name| ModelCapability {
                local_name: name.clone(),
                model_digest: None,
                quantization: None,
                parameter_size_b: None,
                max_context_length: 0,
                tool_call_support: false,
                structured_output_support: false,
                estimated_vram_bytes: None,
                load_state: if resident.contains(name) {
                    ModelLoadState::Warm
                } else {
                    ModelLoadState::Cold
                },
            })
            .collect();
        Self {
            worker_id,
            capability_revision,
            generated_at: now,
            valid_until: now + DEFAULT_STATIC_CAPABILITY_TTL,
            boot_id: boot_id.into(),
            software_version: SoftwareVersion {
                version: "lokai-node".into(),
                build: None,
            },
            protocol_range: ProtocolRange {
                min: legacy.min_protocol_version.clone(),
                max: legacy.max_protocol_version.clone(),
            },
            supported_features: FeatureSet {
                features: vec!["legacy_v1_chat".into()],
            },
            supported_job_types: vec![JobCapability {
                job_kind: JobKind::Infer,
                schema_versions: VersionRange { min: 1, max: 1 },
                maximum_input_bytes: 2 * 1024 * 1024,
                maximum_output_bytes: 8 * 1024 * 1024,
                maximum_artifacts: 0,
                supports_cancellation: false,
                supports_heartbeats: false,
                supports_leases: false,
                supports_streaming: true,
            }],
            model_inventory,
            hardware: HardwareCapabilities {
                cpu_arch: None,
                logical_cores: 0,
                total_ram_bytes: None,
                available_ram_bytes: None,
                gpus: if vram_total_mb > 0 {
                    vec![GpuDevice {
                        name: "gpu0".into(),
                        vram_bytes: u64::from(vram_total_mb) * 1024 * 1024,
                        vram_free_bytes: Some(u64::from(vram_free_mb) * 1024 * 1024),
                    }]
                } else {
                    vec![]
                },
            },
            runtime_capacity: RuntimeCapacity {
                active_jobs,
                queued_jobs,
                maximum_concurrent_jobs: max_concurrent.max(1),
                available_ram_bytes: None,
                available_vram_bytes: if vram_free_mb > 0 {
                    Some(u64::from(vram_free_mb) * 1024 * 1024)
                } else {
                    None
                },
                estimated_queue_delay_secs: 0,
                load_score: if max_concurrent > 0 {
                    (active_jobs + queued_jobs) as f32 / max_concurrent as f32
                } else {
                    0.0
                },
                draining: false,
            },
            sandbox: SandboxCapabilities::default(),
            limits: WorkerLimits {
                max_concurrent_jobs: max_concurrent.max(1),
                max_input_bytes: 2 * 1024 * 1024,
                max_output_bytes: 8 * 1024 * 1024,
            },
            revocation_epoch,
            evidence: vec![CapabilityEvidence {
                kind: CapabilityEvidenceKind::WorkerLocallyProbed,
                claim: "legacy_v1_chat_capabilities".into(),
            }],
            legacy_advertisement: Some(legacy),
        }
    }

    /// Typed fabric Infer profile (R7-1): advertises `fabric_v1` with cancel/heartbeat/lease.
    #[allow(clippy::too_many_arguments)]
    pub fn typed_infer_profile(
        worker_id: WorkerId,
        boot_id: impl Into<String>,
        capability_revision: u64,
        revocation_epoch: u64,
        models: &[String],
        resident_models: &[String],
        vram_total_mb: u32,
        vram_free_mb: u32,
        active_jobs: u32,
        queued_jobs: u32,
        max_concurrent: u32,
    ) -> Self {
        let now = Utc::now();
        let typed = WorkerCapabilityAdvertisement::fabric_v1_full(vec![JobKind::Infer]);
        let resident: std::collections::HashSet<_> = resident_models.iter().collect();
        let model_inventory = models
            .iter()
            .map(|name| ModelCapability {
                local_name: name.clone(),
                model_digest: None,
                quantization: None,
                parameter_size_b: None,
                max_context_length: 0,
                tool_call_support: false,
                structured_output_support: false,
                estimated_vram_bytes: None,
                load_state: if resident.contains(name) {
                    ModelLoadState::Warm
                } else {
                    ModelLoadState::Cold
                },
            })
            .collect();
        Self {
            worker_id,
            capability_revision,
            generated_at: now,
            valid_until: now + DEFAULT_STATIC_CAPABILITY_TTL,
            boot_id: boot_id.into(),
            software_version: SoftwareVersion {
                version: "lokai-node".into(),
                build: None,
            },
            protocol_range: ProtocolRange {
                min: typed.min_protocol_version.clone(),
                max: typed.max_protocol_version.clone(),
            },
            supported_features: FeatureSet {
                features: vec!["fabric_v1".into(), "legacy_v1_chat".into()],
            },
            supported_job_types: vec![JobCapability {
                job_kind: JobKind::Infer,
                schema_versions: VersionRange { min: 1, max: 1 },
                maximum_input_bytes: 2 * 1024 * 1024,
                maximum_output_bytes: 8 * 1024 * 1024,
                maximum_artifacts: 0,
                supports_cancellation: true,
                supports_heartbeats: true,
                supports_leases: true,
                supports_streaming: true,
            }],
            model_inventory,
            hardware: HardwareCapabilities {
                cpu_arch: None,
                logical_cores: 0,
                total_ram_bytes: None,
                available_ram_bytes: None,
                gpus: if vram_total_mb > 0 {
                    vec![GpuDevice {
                        name: "gpu0".into(),
                        vram_bytes: u64::from(vram_total_mb) * 1024 * 1024,
                        vram_free_bytes: Some(u64::from(vram_free_mb) * 1024 * 1024),
                    }]
                } else {
                    vec![]
                },
            },
            runtime_capacity: RuntimeCapacity {
                active_jobs,
                queued_jobs,
                maximum_concurrent_jobs: max_concurrent.max(1),
                available_ram_bytes: None,
                available_vram_bytes: if vram_free_mb > 0 {
                    Some(u64::from(vram_free_mb) * 1024 * 1024)
                } else {
                    None
                },
                estimated_queue_delay_secs: 0,
                load_score: if max_concurrent > 0 {
                    (active_jobs + queued_jobs) as f32 / max_concurrent as f32
                } else {
                    0.0
                },
                draining: false,
            },
            sandbox: SandboxCapabilities::default(),
            limits: WorkerLimits {
                max_concurrent_jobs: max_concurrent.max(1),
                max_input_bytes: 2 * 1024 * 1024,
                max_output_bytes: 8 * 1024 * 1024,
            },
            revocation_epoch,
            evidence: vec![CapabilityEvidence {
                kind: CapabilityEvidenceKind::WorkerLocallyProbed,
                claim: "fabric_v1_infer_capabilities".into(),
            }],
            legacy_advertisement: Some(typed),
        }
    }

    pub fn supports_job_kind(&self, kind: &JobKind) -> bool {
        self.supported_job_types.iter().any(|j| &j.job_kind == kind)
    }

    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        now >= self.valid_until
    }

    pub fn dynamic_capacity_expired(&self, now: DateTime<Utc>) -> bool {
        now >= self.generated_at + DEFAULT_DYNAMIC_CAPACITY_TTL
    }
}

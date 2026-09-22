//! Per-job-kind resource / safety profiles (M6-1).

use tetonic_fabric_protocol::JobKind;

use crate::budget::{DurationEstimate, ResourceAmount, ResourceRequest};

#[derive(Debug, Clone)]
pub struct JobKindProfile {
    pub side_effect_free: bool,
    pub deterministic: bool,
    pub supports_speculation: bool,
    pub requires_sandbox: bool,
    pub default_resources: ResourceRequest,
}

pub fn profile_for(kind: &JobKind) -> JobKindProfile {
    match kind {
        JobKind::Infer => JobKindProfile {
            side_effect_free: true,
            deterministic: false,
            supports_speculation: true,
            requires_sandbox: false,
            default_resources: ResourceRequest::infer_default(),
        },
        JobKind::Embed => JobKindProfile {
            side_effect_free: true,
            deterministic: true,
            supports_speculation: true,
            requires_sandbox: false,
            default_resources: ResourceRequest {
                inference_slots: 1,
                vram_bytes: 256 * 1024 * 1024,
                total_token_budget: 16_384,
                estimated_duration: DurationEstimate {
                    expected_ms: 5_000,
                    hard_limit_ms: 60_000,
                },
                ..ResourceRequest::default()
            },
        },
        JobKind::AnalyzeCode => JobKindProfile {
            side_effect_free: true,
            deterministic: false,
            supports_speculation: true,
            requires_sandbox: false,
            default_resources: ResourceRequest {
                cpu_cores: ResourceAmount { milli_cores: 1_000 },
                memory_bytes: 1024 * 1024 * 1024,
                inference_slots: 1,
                ..ResourceRequest::default()
            },
        },
        JobKind::IndexShard => JobKindProfile {
            side_effect_free: false,
            deterministic: true,
            supports_speculation: false,
            requires_sandbox: true,
            default_resources: ResourceRequest {
                process_slots: 1,
                inference_slots: 0,
                temporary_storage_bytes: 512 * 1024 * 1024,
                estimated_duration: DurationEstimate {
                    expected_ms: 60_000,
                    hard_limit_ms: 600_000,
                },
                ..ResourceRequest::default()
            },
        },
        JobKind::TestShard => JobKindProfile {
            side_effect_free: false,
            deterministic: false,
            supports_speculation: false,
            requires_sandbox: true,
            default_resources: ResourceRequest {
                process_slots: 1,
                inference_slots: 0,
                cpu_cores: ResourceAmount { milli_cores: 2_000 },
                memory_bytes: 2 * 1024 * 1024 * 1024,
                estimated_duration: DurationEstimate {
                    expected_ms: 120_000,
                    hard_limit_ms: 900_000,
                },
                ..ResourceRequest::default()
            },
        },
        JobKind::ReviewArtifact => JobKindProfile {
            side_effect_free: true,
            deterministic: false,
            supports_speculation: true,
            requires_sandbox: false,
            default_resources: ResourceRequest::infer_default(),
        },
    }
}

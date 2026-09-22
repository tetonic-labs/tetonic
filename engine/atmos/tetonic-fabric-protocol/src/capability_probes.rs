//! Safe coordinator-side capability claim probes (M5-2).

use crate::capability_document::{ControlSupport, ModelCapability, WorkerCapabilities};
use crate::CapabilityEvidenceKind;

/// Plausible upper bound for advertised context length (tokens).
pub const MAX_PLAUSIBLE_CONTEXT_LENGTH: u32 = 10_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbeSeverity {
    Info,
    Degraded,
    Quarantine,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityProbeResult {
    pub name: &'static str,
    pub passed: bool,
    pub message: String,
    pub severity: ProbeSeverity,
}

impl CapabilityProbeResult {
    pub fn passed(name: &'static str) -> Self {
        Self {
            name,
            passed: true,
            message: String::new(),
            severity: ProbeSeverity::Info,
        }
    }

    pub fn failed(name: &'static str, message: impl Into<String>, severity: ProbeSeverity) -> Self {
        Self {
            name,
            passed: false,
            message: message.into(),
            severity,
        }
    }
}

pub fn run_capability_probes(caps: &WorkerCapabilities) -> Vec<CapabilityProbeResult> {
    vec![
        probe_cancellation_claim(caps),
        probe_context_length_claim(caps),
        probe_sandbox_enforcement_claim(caps),
        probe_resource_consistency(caps),
        probe_self_assigned_trust(caps),
    ]
}

pub fn probes_quarantine(results: &[CapabilityProbeResult]) -> bool {
    results
        .iter()
        .any(|r| !r.passed && r.severity == ProbeSeverity::Quarantine)
}

pub fn probes_degraded(results: &[CapabilityProbeResult]) -> bool {
    results
        .iter()
        .any(|r| !r.passed && r.severity == ProbeSeverity::Degraded)
}

pub fn merge_probe_results(
    static_probes: Vec<CapabilityProbeResult>,
    extra: impl IntoIterator<Item = CapabilityProbeResult>,
) -> Vec<CapabilityProbeResult> {
    static_probes.into_iter().chain(extra).collect()
}

/// Compare a freshly fetched advertisement with a cached copy (same worker session).
pub fn run_capability_drift_probes(
    previous: &WorkerCapabilities,
    current: &WorkerCapabilities,
) -> Vec<CapabilityProbeResult> {
    vec![
        probe_gpu_disappeared(previous, current),
        probe_model_digest_drift(previous, current),
    ]
}

/// Coordinator-observed checks using live worker probes (health, model list).
pub fn run_coordinator_observed_probes(
    caps: &WorkerCapabilities,
    health_ok: bool,
    models_verified: bool,
) -> Vec<CapabilityProbeResult> {
    vec![
        probe_model_health_observed(caps, health_ok),
        probe_model_inventory_verified(caps, models_verified),
        probe_lease_heartbeat_claim(caps),
    ]
}

/// Compare advertised inventory local_names to coordinator-fetched verified names.
///
/// Quarantines when the worker advertises a model that Ollama does not list.
pub fn probe_inventory_matches_verified(
    inventory: &[ModelCapability],
    verified_names: &[String],
) -> CapabilityProbeResult {
    for model in inventory {
        if !verified_names.iter().any(|name| name == &model.local_name) {
            return CapabilityProbeResult::failed(
                "inventory_matches_verified",
                format!(
                    "advertised model {} missing from verified Ollama inventory",
                    model.local_name
                ),
                ProbeSeverity::Quarantine,
            );
        }
    }
    CapabilityProbeResult::passed("inventory_matches_verified")
}

pub fn observed_evidence_from_probes(
    results: &[CapabilityProbeResult],
) -> Vec<crate::CapabilityEvidence> {
    results
        .iter()
        .filter(|r| r.passed)
        .map(|r| crate::CapabilityEvidence {
            kind: CapabilityEvidenceKind::CoordinatorObserved,
            claim: r.name.to_string(),
        })
        .collect()
}

/// Coordinator-assigned verification after a successful probe session (not self-reported).
pub fn verified_evidence_from_session(
    probe_results: &[CapabilityProbeResult],
    health_ok: bool,
    models_verified: bool,
) -> Vec<crate::CapabilityEvidence> {
    if !health_ok || !models_verified {
        return Vec::new();
    }
    if probe_results.iter().any(|r| !r.passed) {
        return Vec::new();
    }
    vec![crate::CapabilityEvidence {
        kind: CapabilityEvidenceKind::CoordinatorVerified,
        claim: "capability_probe_session".into(),
    }]
}

fn probe_gpu_disappeared(
    previous: &WorkerCapabilities,
    current: &WorkerCapabilities,
) -> CapabilityProbeResult {
    let had_gpu = !previous.hardware.gpus.is_empty();
    let has_gpu = !current.hardware.gpus.is_empty();
    if had_gpu && !has_gpu {
        return CapabilityProbeResult::failed(
            "gpu_disappeared",
            "hardware GPUs present in prior advertisement but missing now",
            ProbeSeverity::Degraded,
        );
    }
    CapabilityProbeResult::passed("gpu_disappeared")
}

fn probe_model_digest_drift(
    previous: &WorkerCapabilities,
    current: &WorkerCapabilities,
) -> CapabilityProbeResult {
    for model in &current.model_inventory {
        let Some(prev) = previous
            .model_inventory
            .iter()
            .find(|m| m.local_name == model.local_name)
        else {
            continue;
        };
        if prev.model_digest.is_some()
            && model.model_digest.is_some()
            && prev.model_digest != model.model_digest
        {
            return CapabilityProbeResult::failed(
                "model_digest_drift",
                format!(
                    "model {} digest changed under tag {}",
                    model.local_name, model.local_name
                ),
                ProbeSeverity::Degraded,
            );
        }
    }
    CapabilityProbeResult::passed("model_digest_drift")
}

fn probe_model_health_observed(
    caps: &WorkerCapabilities,
    health_ok: bool,
) -> CapabilityProbeResult {
    if health_ok || caps.model_inventory.is_empty() {
        return CapabilityProbeResult::passed("model_health_observed");
    }
    CapabilityProbeResult::failed(
        "model_health_observed",
        "worker health probe failed while models are advertised",
        ProbeSeverity::Degraded,
    )
}

fn probe_model_inventory_verified(
    caps: &WorkerCapabilities,
    models_verified: bool,
) -> CapabilityProbeResult {
    if models_verified || caps.model_inventory.is_empty() {
        return CapabilityProbeResult::passed("model_inventory_verified");
    }
    CapabilityProbeResult::failed(
        "model_inventory_verified",
        "model inventory could not be verified via capabilities fetch",
        ProbeSeverity::Degraded,
    )
}

fn probe_lease_heartbeat_claim(caps: &WorkerCapabilities) -> CapabilityProbeResult {
    let legacy = is_legacy_profile(caps);
    for job in &caps.supported_job_types {
        if legacy && job.supports_leases {
            return CapabilityProbeResult::failed(
                "lease_heartbeat_conformance",
                "legacy worker must not claim lease support",
                ProbeSeverity::Quarantine,
            );
        }
        if legacy && job.supports_heartbeats {
            return CapabilityProbeResult::failed(
                "lease_heartbeat_conformance",
                "legacy worker must not claim heartbeat support",
                ProbeSeverity::Quarantine,
            );
        }
    }
    CapabilityProbeResult::passed("lease_heartbeat_conformance")
}

fn is_legacy_profile(caps: &WorkerCapabilities) -> bool {
    caps.legacy_advertisement
        .as_ref()
        .is_some_and(|l| l.legacy_v1_chat_only)
}

fn probe_cancellation_claim(caps: &WorkerCapabilities) -> CapabilityProbeResult {
    if !is_legacy_profile(caps) {
        return CapabilityProbeResult::passed("cancellation_conformance");
    }
    for job in &caps.supported_job_types {
        if job.supports_cancellation {
            return CapabilityProbeResult::failed(
                "cancellation_conformance",
                "legacy worker must not claim cancellation support",
                ProbeSeverity::Quarantine,
            );
        }
    }
    CapabilityProbeResult::passed("cancellation_conformance")
}

fn probe_context_length_claim(caps: &WorkerCapabilities) -> CapabilityProbeResult {
    for model in &caps.model_inventory {
        if model.max_context_length > MAX_PLAUSIBLE_CONTEXT_LENGTH {
            return CapabilityProbeResult::failed(
                "context_length_claim",
                format!(
                    "model {} claims implausible max_context_length {}",
                    model.local_name, model.max_context_length
                ),
                ProbeSeverity::Quarantine,
            );
        }
    }
    CapabilityProbeResult::passed("context_length_claim")
}

fn sandbox_advertised_enforced(s: &ControlSupport) -> bool {
    matches!(s, ControlSupport::Enforced | ControlSupport::Partial)
}

fn probe_sandbox_enforcement_claim(caps: &WorkerCapabilities) -> CapabilityProbeResult {
    if !is_legacy_profile(caps) {
        return CapabilityProbeResult::passed("sandbox_control_claim");
    }
    let sb = &caps.sandbox;
    if sandbox_advertised_enforced(&sb.process_tree)
        || sandbox_advertised_enforced(&sb.runtime_limits)
        || sandbox_advertised_enforced(&sb.memory_limits)
        || sandbox_advertised_enforced(&sb.network_denial)
        || sandbox_advertised_enforced(&sb.network_allowlist)
    {
        return CapabilityProbeResult::failed(
            "sandbox_control_claim",
            "legacy worker advertises sandbox controls that are not enforced",
            ProbeSeverity::Quarantine,
        );
    }
    CapabilityProbeResult::passed("sandbox_control_claim")
}

fn probe_resource_consistency(caps: &WorkerCapabilities) -> CapabilityProbeResult {
    let total_vram: u64 = caps.hardware.gpus.iter().map(|g| g.vram_bytes).sum();
    if total_vram > 0 {
        if let Some(avail) = caps.runtime_capacity.available_vram_bytes {
            if avail > total_vram {
                return CapabilityProbeResult::failed(
                    "resource_consistency",
                    "available_vram_bytes exceeds total GPU VRAM",
                    ProbeSeverity::Quarantine,
                );
            }
        }
        for gpu in &caps.hardware.gpus {
            if let Some(free) = gpu.vram_free_bytes {
                if free > gpu.vram_bytes {
                    return CapabilityProbeResult::failed(
                        "resource_consistency",
                        format!("GPU {} free VRAM exceeds total", gpu.name),
                        ProbeSeverity::Quarantine,
                    );
                }
            }
        }
    }
    CapabilityProbeResult::passed("resource_consistency")
}

fn probe_self_assigned_trust(caps: &WorkerCapabilities) -> CapabilityProbeResult {
    for ev in &caps.evidence {
        if matches!(
            ev.kind,
            CapabilityEvidenceKind::CoordinatorVerified | CapabilityEvidenceKind::Attested
        ) {
            return CapabilityProbeResult::failed(
                "self_assigned_trust",
                format!("worker must not self-assign evidence kind {:?}", ev.kind),
                ProbeSeverity::Quarantine,
            );
        }
    }
    CapabilityProbeResult::passed("self_assigned_trust")
}

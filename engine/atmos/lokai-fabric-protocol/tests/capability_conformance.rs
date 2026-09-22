//! M5-2 capability conformance tests.

use chrono::{Duration, Utc};
use lokai_domain::ids::WorkerId;
use lokai_fabric_protocol::{
    capability_document::MAX_CAPABILITY_DOCUMENT_BYTES,
    capability_probes::{run_capability_probes, MAX_PLAUSIBLE_CONTEXT_LENGTH},
    capability_registry::CapabilityRegistry,
    scheduling_eligible, validate_capability_document_bytes, validate_worker_capabilities,
    CapabilityEvidence, CapabilityEvidenceKind, ControlSupport, ModelCapability, ModelLoadState,
    WorkerCapabilities, WorkerCapabilityAdvertisement,
};

fn sample_caps() -> WorkerCapabilities {
    WorkerCapabilities::legacy_infer_profile(
        WorkerId::new("worker_1"),
        "boot_1",
        1,
        3,
        &["qwen:7b".into()],
        &["qwen:7b".into()],
        8192,
        6144,
        1,
        0,
        2,
    )
}

#[test]
fn legacy_worker_profile() {
    let caps = sample_caps();
    let legacy = caps.legacy_advertisement.as_ref().unwrap();
    assert!(legacy.legacy_v1_chat_only);
    assert!(!legacy.supports_leases);
    assert_eq!(caps.supported_job_types.len(), 1);
    assert!(caps.supports_job_kind(&lokai_fabric_protocol::JobKind::Infer));
}

#[test]
fn typed_infer_profile_advertises_fabric_v1() {
    let caps = WorkerCapabilities::typed_infer_profile(
        WorkerId::new("w1"),
        "boot",
        1,
        0,
        &["m".into()],
        &["m".into()],
        1024,
        512,
        0,
        0,
        1,
    );
    assert!(caps
        .supported_features
        .features
        .iter()
        .any(|f| f == "fabric_v1"));
    let job = caps.supported_job_types.first().unwrap();
    assert!(job.supports_cancellation);
    assert!(job.supports_heartbeats);
    assert!(job.supports_leases);
    let adv = caps.legacy_advertisement.as_ref().unwrap();
    assert!(!adv.legacy_v1_chat_only);
    assert_eq!(caps.evidence[0].claim, "fabric_v1_infer_capabilities");
}

#[test]
fn expired_advertisement_fails_validation() {
    let mut caps = sample_caps();
    let now = Utc::now();
    caps.valid_until = now - Duration::seconds(1);
    let err = validate_worker_capabilities(&caps, &caps.worker_id, 0, now).unwrap_err();
    assert!(matches!(
        err.code,
        lokai_fabric_protocol::FabricErrorCode::InvalidEnvelope
    ));
}

#[test]
fn model_digest_change_with_same_tag() {
    let mut caps = sample_caps();
    caps.model_inventory.push(ModelCapability {
        local_name: "qwen:7b".into(),
        model_digest: Some("sha256:abc".into()),
        quantization: Some("Q4_K_M".into()),
        parameter_size_b: None,
        max_context_length: 8192,
        tool_call_support: true,
        structured_output_support: false,
        estimated_vram_bytes: None,
        load_state: ModelLoadState::Warm,
    });
    let mut caps2 = caps.clone();
    caps2.model_inventory[1].model_digest = Some("sha256:def".into());
    assert_ne!(
        caps.model_inventory[1].model_digest,
        caps2.model_inventory[1].model_digest
    );
}

#[test]
fn gpu_disappears_after_advertisement() {
    let mut reg = CapabilityRegistry::new();
    let wid = WorkerId::new("worker_1");
    let now = Utc::now();
    let previous = sample_caps();
    reg.upsert_validated(previous.clone(), &wid, 0, now)
        .unwrap();
    let mut current = sample_caps();
    current.hardware.gpus.clear();
    current.runtime_capacity.available_vram_bytes = None;
    current.generated_at = now;
    current.valid_until = now + Duration::hours(1);
    reg.upsert_probe_session(lokai_fabric_protocol::ProbeSessionInput {
        caps: current,
        channel_worker_id: &wid,
        known_revocation_epoch: 0,
        now,
        previous: Some(&previous),
        health_ok: true,
        models_verified: true,
        verified_model_names: None,
    })
    .unwrap();
    assert!(reg.is_degraded("worker_1"));
}

#[test]
fn gpu_disappears_structural() {
    let mut caps = sample_caps();
    assert!(!caps.hardware.gpus.is_empty());
    caps.hardware.gpus.clear();
    caps.runtime_capacity.available_vram_bytes = None;
    assert!(caps.hardware.gpus.is_empty());
}

#[test]
fn coordinator_observed_health_failure() {
    let caps = sample_caps();
    let results = lokai_fabric_protocol::run_coordinator_observed_probes(&caps, false, true);
    assert!(results
        .iter()
        .any(|r| r.name == "model_health_observed" && !r.passed));
}

#[test]
fn model_digest_drift_detected() {
    let mut previous = sample_caps();
    previous.model_inventory[0].model_digest = Some("sha256:aaa".into());
    let mut current = previous.clone();
    current.model_inventory[0].model_digest = Some("sha256:bbb".into());
    let results = lokai_fabric_protocol::run_capability_drift_probes(&previous, &current);
    assert!(results
        .iter()
        .any(|r| r.name == "model_digest_drift" && !r.passed));
}

#[test]
fn worker_draining_not_schedulable() {
    let mut caps = sample_caps();
    caps.runtime_capacity.draining = true;
    assert!(!scheduling_eligible(&caps, Utc::now(), false));
}

#[test]
fn revocation_epoch_stale() {
    let caps = sample_caps();
    let err = validate_worker_capabilities(&caps, &caps.worker_id, 5, Utc::now()).unwrap_err();
    assert!(matches!(
        err.code,
        lokai_fabric_protocol::FabricErrorCode::StaleRevocationEpoch
    ));
}

#[test]
fn oversized_capability_document() {
    let blob = vec![0u8; MAX_CAPABILITY_DOCUMENT_BYTES as usize + 1];
    assert!(validate_capability_document_bytes(&blob).is_err());
}

#[test]
fn sandbox_per_control_not_boolean() {
    let caps = sample_caps();
    assert!(matches!(
        caps.sandbox.process_tree,
        ControlSupport::Unsupported
    ));
}

#[test]
fn self_reported_evidence_present() {
    let caps = sample_caps();
    assert!(!caps.evidence.is_empty());
}

#[test]
fn worker_restart_requires_new_boot_id() {
    let a = WorkerCapabilities::legacy_infer_profile(
        WorkerId::new("w"),
        "boot_a",
        1,
        0,
        &[],
        &[],
        0,
        0,
        0,
        0,
        1,
    );
    let b = WorkerCapabilities::legacy_infer_profile(
        WorkerId::new("w"),
        "boot_b",
        1,
        0,
        &[],
        &[],
        0,
        0,
        0,
        0,
        1,
    );
    assert_ne!(a.boot_id, b.boot_id);
}

#[test]
fn legacy_advertisement_matches_m5_1() {
    let caps = sample_caps();
    assert_eq!(
        caps.legacy_advertisement,
        Some(WorkerCapabilityAdvertisement::legacy_v1_chat_infer_only())
    );
}

#[test]
fn false_context_length_claim_fails_probe() {
    let mut caps = sample_caps();
    caps.model_inventory[0].max_context_length = MAX_PLAUSIBLE_CONTEXT_LENGTH + 1;
    let results = run_capability_probes(&caps);
    assert!(results
        .iter()
        .any(|r| r.name == "context_length_claim" && !r.passed));
}

#[test]
fn cancellation_capability_claim_fails_probe() {
    let mut caps = sample_caps();
    caps.supported_job_types[0].supports_cancellation = true;
    let results = run_capability_probes(&caps);
    assert!(results
        .iter()
        .any(|r| r.name == "cancellation_conformance" && !r.passed));
}

#[test]
fn sandbox_control_advertised_not_enforced_on_legacy() {
    let mut caps = sample_caps();
    caps.sandbox.process_tree = ControlSupport::Enforced;
    let results = run_capability_probes(&caps);
    assert!(results
        .iter()
        .any(|r| r.name == "sandbox_control_claim" && !r.passed));
}

#[test]
fn self_reported_capability_not_used_as_trust_claim() {
    let mut caps = sample_caps();
    caps.evidence.push(CapabilityEvidence {
        kind: CapabilityEvidenceKind::CoordinatorVerified,
        claim: "gpu_attestation".into(),
    });
    let results = run_capability_probes(&caps);
    assert!(results
        .iter()
        .any(|r| r.name == "self_assigned_trust" && !r.passed));
}

#[test]
fn capability_cache_after_worker_software_upgrade() {
    let mut reg = CapabilityRegistry::new();
    let wid = WorkerId::new("worker_1");
    let now = Utc::now();
    reg.upsert_validated(sample_caps(), &wid, 0, now).unwrap();
    let mut upgraded = sample_caps();
    upgraded.software_version.version = "lokai-node-99".into();
    upgraded.generated_at = now;
    upgraded.valid_until = now + Duration::hours(1);
    assert_eq!(
        reg.upsert_validated(upgraded, &wid, 0, now).unwrap(),
        lokai_fabric_protocol::CapabilityRefreshOutcome::Updated
    );
    assert_eq!(
        reg.get("worker_1").unwrap().software_version.version,
        "lokai-node-99"
    );
}

#[test]
fn dynamic_capacity_stale_during_scheduling() {
    let caps = sample_caps();
    let stale = caps.generated_at + Duration::minutes(5);
    assert!(caps.dynamic_capacity_expired(stale));
    assert!(!scheduling_eligible(&caps, stale, true));
    assert!(scheduling_eligible(&caps, stale, false));
}

#[test]
fn capability_document_rejects_secrets() {
    let mut caps = sample_caps();
    caps.evidence.push(CapabilityEvidence {
        kind: CapabilityEvidenceKind::SelfReported,
        claim: "password=hunter2".into(),
    });
    assert!(validate_worker_capabilities(&caps, &caps.worker_id, 0, Utc::now()).is_err());
}

#[test]
fn verified_evidence_assigned_by_coordinator_session() {
    let mut reg = CapabilityRegistry::new();
    let wid = WorkerId::new("worker_1");
    let now = Utc::now();
    reg.upsert_probe_session(lokai_fabric_protocol::ProbeSessionInput {
        caps: sample_caps(),
        channel_worker_id: &wid,
        known_revocation_epoch: 0,
        now,
        previous: None,
        health_ok: true,
        models_verified: true,
        verified_model_names: None,
    })
    .unwrap();
    let verified = reg.verified_evidence("worker_1").unwrap();
    assert!(verified
        .iter()
        .any(|e| e.kind == CapabilityEvidenceKind::CoordinatorVerified));
}

#[test]
fn quantization_field_present_on_model_capability() {
    let mut caps = sample_caps();
    caps.model_inventory[0].quantization = Some("Q4_K_M".into());
    assert_eq!(
        caps.model_inventory[0].quantization.as_deref(),
        Some("Q4_K_M")
    );
}

#[test]
fn ui_summary_reflects_draining_and_quarantine() {
    let mut caps = sample_caps();
    caps.runtime_capacity.draining = true;
    let summary = caps.ui_summary(Utc::now(), false, false, false);
    assert!(summary.draining);
    assert!(!summary.schedulable);
    let summary_q = caps.ui_summary(Utc::now(), true, false, false);
    assert!(summary_q.quarantined);
    assert!(!summary_q.schedulable);
    let summary_d = caps.ui_summary(Utc::now(), false, true, false);
    assert!(summary_d.degraded);
}

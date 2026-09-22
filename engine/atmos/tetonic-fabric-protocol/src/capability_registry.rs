//! Coordinator-side capability cache, probes, and scheduling eligibility (M5-2).

use std::collections::HashMap;
use std::time::Instant;

use chrono::{DateTime, Utc};
use tetonic_domain::ids::WorkerId;

use crate::capability_document::{CapabilityEvidence, WorkerCapabilities};
use crate::capability_metrics::CapabilityProbeMetrics;
use crate::capability_probes::{
    merge_probe_results, observed_evidence_from_probes, probe_inventory_matches_verified,
    probes_degraded, probes_quarantine, run_capability_drift_probes, run_capability_probes,
    run_coordinator_observed_probes, verified_evidence_from_session, CapabilityProbeResult,
};
use crate::capability_validate::{scheduling_eligible, validate_worker_capabilities};
use crate::FabricError;

#[derive(Clone, Debug)]
struct CachedEntry {
    caps: WorkerCapabilities,
    #[allow(dead_code)]
    fetched_at: Instant,
    quarantined: bool,
    degraded: bool,
    probe_results: Vec<CapabilityProbeResult>,
    observed_evidence: Vec<CapabilityEvidence>,
    verified_evidence: Vec<CapabilityEvidence>,
}

#[derive(Clone, Debug, Default)]
pub struct CapabilityRegistry {
    entries: HashMap<String, CachedEntry>,
    metrics: CapabilityProbeMetrics,
    snapshot_invalidate_pending: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CapabilityRefreshOutcome {
    Inserted,
    Updated,
    Unchanged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkerSchedulingState {
    Eligible,
    Degraded,
    Quarantined,
    Expired,
    Draining,
    NotCached,
}

pub struct ProbeSessionInput<'a> {
    pub caps: WorkerCapabilities,
    pub channel_worker_id: &'a WorkerId,
    pub known_revocation_epoch: u64,
    pub now: DateTime<Utc>,
    pub previous: Option<&'a WorkerCapabilities>,
    pub health_ok: bool,
    pub models_verified: bool,
    /// When set, inventory names not present here quarantine the worker (R7-2).
    pub verified_model_names: Option<&'a [String]>,
}

impl CapabilityRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn metrics(&self) -> &CapabilityProbeMetrics {
        &self.metrics
    }

    pub fn needs_snapshot_refresh(&self) -> bool {
        self.snapshot_invalidate_pending
    }

    pub fn take_snapshot_invalidation(&mut self) -> bool {
        std::mem::take(&mut self.snapshot_invalidate_pending)
    }

    pub fn upsert_validated(
        &mut self,
        caps: WorkerCapabilities,
        channel_worker_id: &WorkerId,
        known_revocation_epoch: u64,
        now: DateTime<Utc>,
    ) -> Result<CapabilityRefreshOutcome, FabricError> {
        self.upsert_probe_session(ProbeSessionInput {
            caps,
            channel_worker_id,
            known_revocation_epoch,
            now,
            previous: None,
            health_ok: true,
            models_verified: true,
            verified_model_names: None,
        })
    }

    pub fn upsert_probe_session(
        &mut self,
        input: ProbeSessionInput<'_>,
    ) -> Result<CapabilityRefreshOutcome, FabricError> {
        let ProbeSessionInput {
            caps,
            channel_worker_id,
            known_revocation_epoch,
            now,
            previous,
            health_ok,
            models_verified,
            verified_model_names,
        } = input;
        validate_worker_capabilities(&caps, channel_worker_id, known_revocation_epoch, now)?;
        if let Some(existing) = self.entries.get(&caps.worker_id.0) {
            if existing.caps.boot_id != caps.boot_id
                && caps.capability_revision <= existing.caps.capability_revision
            {
                return Err(FabricError {
                    code: crate::FabricErrorCode::InvalidEnvelope,
                    message: "worker restart must bump capability_revision".into(),
                    details: None,
                });
            }
        }
        let drift = previous
            .filter(|prev| prev.boot_id == caps.boot_id)
            .map(|prev| run_capability_drift_probes(prev, &caps))
            .unwrap_or_default();
        let observed = run_coordinator_observed_probes(&caps, health_ok, models_verified);
        let inventory_match = verified_model_names
            .map(|names| probe_inventory_matches_verified(&caps.model_inventory, names));
        let probe_results = merge_probe_results(
            run_capability_probes(&caps),
            drift.into_iter().chain(observed).chain(inventory_match),
        );
        let quarantined = probes_quarantine(&probe_results);
        let degraded = !quarantined && probes_degraded(&probe_results);
        let observed_evidence = observed_evidence_from_probes(&probe_results);
        let verified_evidence =
            verified_evidence_from_session(&probe_results, health_ok, models_verified);
        self.metrics
            .record_probe_results(&probe_results, quarantined, degraded);
        let key = caps.worker_id.0.clone();
        let outcome = match self.entries.get(&key) {
            None => {
                self.metrics.cache_inserts += 1;
                CapabilityRefreshOutcome::Inserted
            }
            Some(existing)
                if existing.caps.capability_revision != caps.capability_revision
                    || existing.caps.boot_id != caps.boot_id
                    || existing.caps.revocation_epoch != caps.revocation_epoch =>
            {
                self.metrics.cache_updates += 1;
                CapabilityRefreshOutcome::Updated
            }
            Some(existing) if existing.caps.generated_at != caps.generated_at => {
                self.metrics.cache_updates += 1;
                CapabilityRefreshOutcome::Updated
            }
            Some(existing)
                if existing.caps.software_version.version != caps.software_version.version =>
            {
                self.metrics.cache_updates += 1;
                CapabilityRefreshOutcome::Updated
            }
            Some(existing) if gpu_inventory_changed(&existing.caps, &caps) => {
                self.metrics.cache_updates += 1;
                CapabilityRefreshOutcome::Updated
            }
            Some(existing) if model_inventory_changed(&existing.caps, &caps) => {
                self.metrics.cache_updates += 1;
                CapabilityRefreshOutcome::Updated
            }
            Some(_) => CapabilityRefreshOutcome::Unchanged,
        };
        if matches!(
            outcome,
            CapabilityRefreshOutcome::Inserted | CapabilityRefreshOutcome::Updated
        ) {
            self.snapshot_invalidate_pending = true;
        }
        self.entries.insert(
            key,
            CachedEntry {
                caps,
                fetched_at: Instant::now(),
                quarantined,
                degraded,
                probe_results,
                observed_evidence,
                verified_evidence,
            },
        );
        Ok(outcome)
    }

    pub fn get(&self, worker_id: &str) -> Option<&WorkerCapabilities> {
        self.entries.get(worker_id).map(|e| &e.caps)
    }

    pub fn observed_evidence(&self, worker_id: &str) -> Option<&[CapabilityEvidence]> {
        self.entries
            .get(worker_id)
            .map(|e| e.observed_evidence.as_slice())
    }

    pub fn verified_evidence(&self, worker_id: &str) -> Option<&[CapabilityEvidence]> {
        self.entries
            .get(worker_id)
            .map(|e| e.verified_evidence.as_slice())
    }

    pub fn probe_results(&self, worker_id: &str) -> Option<&[CapabilityProbeResult]> {
        self.entries
            .get(worker_id)
            .map(|e| e.probe_results.as_slice())
    }

    pub fn is_quarantined(&self, worker_id: &str) -> bool {
        self.entries.get(worker_id).is_some_and(|e| e.quarantined)
    }

    /// Coordinator-owned quarantine from result-integrity behavior signals (M5-4).
    pub fn force_quarantine(&mut self, worker_id: &str) -> bool {
        if let Some(entry) = self.entries.get_mut(worker_id) {
            entry.quarantined = true;
            entry.degraded = false;
            return true;
        }
        false
    }

    pub fn force_degraded(&mut self, worker_id: &str) -> bool {
        if let Some(entry) = self.entries.get_mut(worker_id) {
            if !entry.quarantined {
                entry.degraded = true;
            }
            return true;
        }
        false
    }

    pub fn is_degraded(&self, worker_id: &str) -> bool {
        self.entries.get(worker_id).is_some_and(|e| e.degraded)
    }

    pub fn scheduling_state(&self, worker_id: &str, now: DateTime<Utc>) -> WorkerSchedulingState {
        let Some(entry) = self.entries.get(worker_id) else {
            return WorkerSchedulingState::NotCached;
        };
        if entry.quarantined {
            return WorkerSchedulingState::Quarantined;
        }
        if entry.caps.is_expired(now) {
            return WorkerSchedulingState::Expired;
        }
        if entry.caps.runtime_capacity.draining {
            return WorkerSchedulingState::Draining;
        }
        if entry.degraded || entry.caps.dynamic_capacity_expired(now) {
            return WorkerSchedulingState::Degraded;
        }
        WorkerSchedulingState::Eligible
    }

    pub fn schedulable(
        &self,
        worker_id: &str,
        now: DateTime<Utc>,
        require_fresh_dynamic: bool,
    ) -> Option<&WorkerCapabilities> {
        let entry = self.entries.get(worker_id)?;
        if entry.quarantined {
            return None;
        }
        if scheduling_eligible(&entry.caps, now, require_fresh_dynamic) {
            Some(&entry.caps)
        } else {
            None
        }
    }

    pub fn invalidate_worker(&mut self, worker_id: &str) {
        self.entries.remove(worker_id);
    }

    pub fn invalidate_all(&mut self) {
        self.entries.clear();
        self.snapshot_invalidate_pending = true;
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn gpu_inventory_changed(previous: &WorkerCapabilities, current: &WorkerCapabilities) -> bool {
    previous.hardware.gpus.len() != current.hardware.gpus.len()
}

fn model_inventory_changed(previous: &WorkerCapabilities, current: &WorkerCapabilities) -> bool {
    if previous.model_inventory.len() != current.model_inventory.len() {
        return true;
    }
    for (a, b) in previous
        .model_inventory
        .iter()
        .zip(current.model_inventory.iter())
    {
        if a.local_name != b.local_name
            || a.model_digest != b.model_digest
            || a.quantization != b.quantization
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use tetonic_domain::ids::WorkerId;

    use super::*;
    use crate::capability_document::{ControlSupport, JobCapability, SandboxCapabilities};
    use crate::JobKind;

    fn sample_caps(revision: u64, boot: &str) -> WorkerCapabilities {
        WorkerCapabilities::legacy_infer_profile(
            WorkerId::new("worker_test"),
            boot,
            revision,
            1,
            &["qwen:latest".into()],
            &["qwen:latest".into()],
            8192,
            4096,
            0,
            0,
            2,
        )
    }

    #[test]
    fn restart_with_reused_revision_rejected_by_boot_change() {
        let mut reg = CapabilityRegistry::new();
        let wid = WorkerId::new("worker_test");
        let now = Utc::now();
        reg.upsert_validated(sample_caps(1, "boot_a"), &wid, 0, now)
            .unwrap();
        let mut caps = sample_caps(1, "boot_b");
        caps.generated_at = now;
        caps.valid_until = now + Duration::hours(1);
        assert!(reg.upsert_validated(caps, &wid, 0, now).is_err());
    }

    #[test]
    fn expired_advertisement_rejected() {
        let mut reg = CapabilityRegistry::new();
        let wid = WorkerId::new("worker_test");
        let now = Utc::now();
        let mut caps = sample_caps(1, "boot_a");
        caps.valid_until = now - Duration::seconds(1);
        assert!(reg.upsert_validated(caps, &wid, 0, now).is_err());
    }

    #[test]
    fn stale_dynamic_capacity_not_schedulable() {
        let mut reg = CapabilityRegistry::new();
        let wid = WorkerId::new("worker_test");
        let now = Utc::now();
        reg.upsert_validated(sample_caps(1, "boot_a"), &wid, 0, now)
            .unwrap();
        let future = now + Duration::minutes(5);
        assert!(reg.schedulable("worker_test", future, true).is_none());
    }

    #[test]
    fn false_cancellation_claim_quarantines_worker() {
        let mut reg = CapabilityRegistry::new();
        let wid = WorkerId::new("worker_test");
        let now = Utc::now();
        let mut caps = sample_caps(1, "boot_a");
        caps.supported_job_types = vec![JobCapability {
            job_kind: JobKind::Infer,
            schema_versions: crate::VersionRange { min: 1, max: 1 },
            maximum_input_bytes: 1024,
            maximum_output_bytes: 1024,
            maximum_artifacts: 0,
            supports_cancellation: true,
            supports_heartbeats: false,
            supports_leases: false,
            supports_streaming: true,
        }];
        reg.upsert_validated(caps, &wid, 0, now).unwrap();
        assert!(reg.is_quarantined("worker_test"));
        assert!(reg.schedulable("worker_test", now, false).is_none());
    }

    #[test]
    fn software_upgrade_updates_cache() {
        let mut reg = CapabilityRegistry::new();
        let wid = WorkerId::new("worker_test");
        let now = Utc::now();
        reg.upsert_validated(sample_caps(1, "boot_a"), &wid, 0, now)
            .unwrap();
        let mut caps = sample_caps(1, "boot_a");
        caps.software_version.version = "lokai-node-2.0".into();
        caps.generated_at = now;
        caps.valid_until = now + Duration::hours(1);
        assert_eq!(
            reg.upsert_validated(caps, &wid, 0, now).unwrap(),
            CapabilityRefreshOutcome::Updated
        );
    }

    #[test]
    fn sandbox_claim_on_legacy_quarantines() {
        let mut reg = CapabilityRegistry::new();
        let wid = WorkerId::new("worker_test");
        let now = Utc::now();
        let mut caps = sample_caps(1, "boot_a");
        caps.sandbox = SandboxCapabilities {
            process_tree: ControlSupport::Enforced,
            ..SandboxCapabilities::default()
        };
        reg.upsert_validated(caps, &wid, 0, now).unwrap();
        assert!(reg.is_quarantined("worker_test"));
    }

    #[test]
    fn gpu_disappears_marks_degraded() {
        let mut reg = CapabilityRegistry::new();
        let wid = WorkerId::new("worker_test");
        let now = Utc::now();
        let previous = sample_caps(1, "boot_a");
        reg.upsert_validated(previous.clone(), &wid, 0, now)
            .unwrap();
        let mut current = sample_caps(1, "boot_a");
        current.hardware.gpus.clear();
        current.runtime_capacity.available_vram_bytes = None;
        current.generated_at = now;
        current.valid_until = now + Duration::hours(1);
        reg.upsert_probe_session(ProbeSessionInput {
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
        assert!(reg.is_degraded("worker_test"));
        assert!(!reg.is_quarantined("worker_test"));
        assert!(reg.schedulable("worker_test", now, false).is_some());
    }

    #[test]
    fn failed_health_probe_marks_degraded() {
        let mut reg = CapabilityRegistry::new();
        let wid = WorkerId::new("worker_test");
        let now = Utc::now();
        let caps = sample_caps(1, "boot_a");
        reg.upsert_probe_session(ProbeSessionInput {
            caps,
            channel_worker_id: &wid,
            known_revocation_epoch: 0,
            now,
            previous: None,
            health_ok: false,
            models_verified: true,
            verified_model_names: None,
        })
        .unwrap();
        assert!(reg.is_degraded("worker_test"));
        assert!(!reg.observed_evidence("worker_test").unwrap().is_empty());
    }

    #[test]
    fn metrics_record_probe_outcomes() {
        let mut reg = CapabilityRegistry::new();
        let wid = WorkerId::new("worker_test");
        let now = Utc::now();
        reg.upsert_probe_session(ProbeSessionInput {
            caps: sample_caps(1, "boot_a"),
            channel_worker_id: &wid,
            known_revocation_epoch: 0,
            now,
            previous: None,
            health_ok: false,
            models_verified: true,
            verified_model_names: None,
        })
        .unwrap();
        assert!(reg.metrics().probes_passed > 0);
        assert!(reg.metrics().workers_degraded > 0);
        assert_eq!(reg.metrics().cache_inserts, 1);
    }

    #[test]
    fn fake_inventory_model_quarantines_and_not_schedulable() {
        let mut reg = CapabilityRegistry::new();
        let wid = WorkerId::new("worker_test");
        let now = Utc::now();
        let caps = sample_caps(1, "boot_a");
        let verified = vec!["other:latest".into()];
        reg.upsert_probe_session(ProbeSessionInput {
            caps,
            channel_worker_id: &wid,
            known_revocation_epoch: 0,
            now,
            previous: None,
            health_ok: true,
            models_verified: false,
            verified_model_names: Some(verified.as_slice()),
        })
        .unwrap();
        assert!(reg.is_quarantined("worker_test"));
        assert!(reg.schedulable("worker_test", now, false).is_none());
        assert!(reg
            .probe_results("worker_test")
            .unwrap()
            .iter()
            .any(|r| r.name == "inventory_matches_verified" && !r.passed));
    }

    #[test]
    fn expired_cached_caps_not_schedulable() {
        let mut reg = CapabilityRegistry::new();
        let wid = WorkerId::new("worker_test");
        let now = Utc::now();
        reg.upsert_validated(sample_caps(1, "boot_a"), &wid, 0, now)
            .unwrap();
        let later = now + Duration::hours(2);
        assert!(matches!(
            reg.scheduling_state("worker_test", later),
            WorkerSchedulingState::Expired
        ));
        assert!(reg.schedulable("worker_test", later, false).is_none());
    }
}

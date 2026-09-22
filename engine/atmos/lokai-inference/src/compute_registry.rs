//! Authorized compute targets for fabric placement (AC2-8).
//!
//! # Architectural Boundary (Decoupling Invariant)
//! This module establishes a pristine decoupling boundary between the runtime compute plane
//! and the trust bootstrapping process (`lokai-enroll`). `AuthorizedComputeTarget` retains
//! zero direct dependency on concrete enrollment keypair structs, consuming only pre-verified
//! abstract network targets and raw TLS certificate bytes.

use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use lokai_domain::WorkerTrust;

/// Audited trust assignment (M5-3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustAssignmentRecord {
    pub worker_id: String,
    pub trust: WorkerTrust,
    pub policy_epoch: u64,
    pub assigned_at_ms: u64,
}

/// Enrolled worker the inference scheduler may place on (not raw enrollment rows).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorizedComputeTarget {
    pub id: String,
    pub label: String,
    pub ip: IpAddr,
    pub fabric_port: u16,
    pub fabric_tls_cert: Arc<[u8]>,
    /// Coordinator-assigned trust tier (M5-3).
    pub worker_trust: WorkerTrust,
}

/// In-memory registry populated at daemon bootstrap from enrollment records.
pub struct ComputeTargetRegistry {
    targets: RwLock<Vec<AuthorizedComputeTarget>>,
    policy_epoch: Arc<AtomicU64>,
    trust_audit: RwLock<Vec<TrustAssignmentRecord>>,
}

impl ComputeTargetRegistry {
    pub fn new() -> Self {
        Self {
            targets: RwLock::new(Vec::new()),
            policy_epoch: Arc::new(AtomicU64::new(0)),
            trust_audit: RwLock::new(Vec::new()),
        }
    }

    pub fn policy_epoch(&self) -> Arc<AtomicU64> {
        self.policy_epoch.clone()
    }

    pub fn bump_epoch(&self, epoch: u64) {
        self.policy_epoch.fetch_max(epoch, Ordering::Relaxed);
    }

    pub fn current_epoch(&self) -> u64 {
        self.policy_epoch.load(Ordering::Relaxed)
    }

    pub fn replace_targets(&self, targets: Vec<AuthorizedComputeTarget>) {
        *self.targets.write().unwrap() = targets;
    }

    pub fn list(&self) -> Vec<AuthorizedComputeTarget> {
        self.targets.read().unwrap().clone()
    }

    pub fn trust_for_worker(&self, id: &str) -> Option<WorkerTrust> {
        self.targets
            .read()
            .unwrap()
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.worker_trust)
    }

    pub fn set_worker_trust(&self, id: &str, trust: WorkerTrust) -> bool {
        let mut g = self.targets.write().unwrap();
        if let Some(t) = g.iter_mut().find(|t| t.id == id) {
            t.worker_trust = trust;
            let epoch = self.bump_epoch_internal();
            self.record_trust_assignment(id, trust, epoch);
            true
        } else {
            false
        }
    }

    fn bump_epoch_internal(&self) -> u64 {
        let epoch = self.current_epoch().saturating_add(1);
        self.bump_epoch(epoch);
        epoch
    }

    fn record_trust_assignment(&self, worker_id: &str, trust: WorkerTrust, epoch: u64) {
        let assigned_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        self.trust_audit
            .write()
            .unwrap()
            .push(TrustAssignmentRecord {
                worker_id: worker_id.to_string(),
                trust,
                policy_epoch: epoch,
                assigned_at_ms,
            });
    }

    pub fn trust_audit_log(&self) -> Vec<TrustAssignmentRecord> {
        self.trust_audit.read().unwrap().clone()
    }

    pub fn remove(&self, id: &str) -> bool {
        let mut g = self.targets.write().unwrap();
        let before = g.len();
        g.retain(|t| t.id != id);
        g.len() < before
    }

    pub fn len(&self) -> usize {
        self.targets.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.targets.read().unwrap().is_empty()
    }
}

impl Default for ComputeTargetRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_and_epoch_bump() {
        let reg = ComputeTargetRegistry::new();
        reg.replace_targets(vec![AuthorizedComputeTarget {
            id: "w1".into(),
            label: "box".into(),
            ip: "127.0.0.1".parse().unwrap(),
            fabric_port: 9443,
            fabric_tls_cert: Arc::from(b"cert".as_slice()),
            worker_trust: WorkerTrust::OwnerControlledEstate,
        }]);
        assert_eq!(reg.len(), 1);
        assert!(reg.remove("w1"));
        assert_eq!(reg.len(), 0);
        reg.bump_epoch(42);
        assert_eq!(reg.current_epoch(), 42);
    }

    #[test]
    fn trust_assignment_is_audited() {
        let reg = ComputeTargetRegistry::new();
        reg.replace_targets(vec![AuthorizedComputeTarget {
            id: "w1".into(),
            label: "box".into(),
            ip: "127.0.0.1".parse().unwrap(),
            fabric_port: 9443,
            fabric_tls_cert: Arc::from(b"cert".as_slice()),
            worker_trust: WorkerTrust::OwnerControlledEstate,
        }]);
        assert!(reg.set_worker_trust("w1", WorkerTrust::ExternalUntrusted));
        let log = reg.trust_audit_log();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].worker_id, "w1");
        assert_eq!(log[0].trust, WorkerTrust::ExternalUntrusted);
        assert_eq!(log[0].policy_epoch, 1);
    }

    #[test]
    fn trust_downgrade_bumps_policy_epoch() {
        let reg = ComputeTargetRegistry::new();
        reg.replace_targets(vec![AuthorizedComputeTarget {
            id: "w1".into(),
            label: "box".into(),
            ip: "127.0.0.1".parse().unwrap(),
            fabric_port: 9443,
            fabric_tls_cert: Arc::from(b"cert".as_slice()),
            worker_trust: WorkerTrust::OwnerControlledEstate,
        }]);
        assert_eq!(reg.current_epoch(), 0);
        assert!(reg.set_worker_trust("w1", WorkerTrust::ExternalUntrusted));
        assert_eq!(reg.current_epoch(), 1);
        assert_eq!(
            reg.trust_for_worker("w1"),
            Some(WorkerTrust::ExternalUntrusted)
        );
    }
}

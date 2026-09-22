//! Worker eligibility combining trust policy and capability state (M5-3).

use chrono::{DateTime, Utc};
use lokai_domain::{DataClass, PlacementReason, ProjectPlacementPolicy, WorkerTrust};
use lokai_fabric_protocol::{
    model_inventory_matches, CapabilityRegistry, ModelSelection, WorkerSchedulingState,
};
use lokai_policy::{placement_to_dispatch_local_only, trust_permits_data_class};

/// Inputs for dispatch-time worker eligibility (trust + capability).
pub struct WorkerEligibilityInput<'a> {
    pub worker_id: &'a str,
    pub trust: WorkerTrust,
    pub data_class: DataClass,
    pub project_policy: &'a ProjectPlacementPolicy,
    pub model: &'a ModelSelection,
    pub coordinator_policy_epoch: u64,
}

/// Whether a placement reason should route to local-only vs hard deny at dispatch.
pub fn placement_reason_is_local_only(reason: &PlacementReason) -> bool {
    placement_to_dispatch_local_only(reason.clone())
}

/// Evaluate whether a worker may receive a remote job with this classification.
///
/// Trust and capability are evaluated independently; both must pass. Unknown
/// or not-yet-probed workers fail closed until a fresh capability document is cached.
pub fn evaluate_worker_eligibility(
    registry: &CapabilityRegistry,
    input: &WorkerEligibilityInput<'_>,
    now: DateTime<Utc>,
) -> Result<(), PlacementReason> {
    if input.data_class == DataClass::Secret {
        return Err(PlacementReason::SecretLocalOnly);
    }

    trust_permits_data_class(input.trust, input.data_class, input.project_policy)?;

    if registry.get(input.worker_id).is_none() {
        return Err(PlacementReason::CapabilityUnavailable);
    }

    match registry.scheduling_state(input.worker_id, now) {
        WorkerSchedulingState::NotCached => {
            return Err(PlacementReason::CapabilityUnavailable);
        }
        WorkerSchedulingState::Quarantined => return Err(PlacementReason::WorkerQuarantined),
        WorkerSchedulingState::Expired => return Err(PlacementReason::CapabilityStale),
        WorkerSchedulingState::Draining => return Err(PlacementReason::CapabilityUnavailable),
        WorkerSchedulingState::Degraded | WorkerSchedulingState::Eligible => {}
    }

    let caps = registry
        .schedulable(input.worker_id, now, false)
        .ok_or(PlacementReason::CapabilityStale)?;

    if caps.revocation_epoch < input.coordinator_policy_epoch {
        return Err(PlacementReason::WorkerRevoked);
    }

    if !model_inventory_matches(&caps.model_inventory, input.model) {
        return Err(PlacementReason::CapabilityUnavailable);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use lokai_domain::ids::WorkerId;
    use lokai_fabric_protocol::WorkerCapabilities;

    use super::*;

    fn seed_registry(reg: &mut CapabilityRegistry, worker_id: &str, revocation_epoch: u64) {
        let wid = WorkerId::new(worker_id);
        let now = Utc::now();
        let mut caps = WorkerCapabilities::legacy_infer_profile(
            wid.clone(),
            "boot_a",
            1,
            revocation_epoch,
            &["qwen:7b".into()],
            &["qwen:7b".into()],
            8192,
            4096,
            0,
            0,
            2,
        );
        caps.generated_at = now;
        caps.valid_until = now + Duration::hours(1);
        reg.upsert_validated(caps, &wid, 0, now).unwrap();
    }

    fn input<'a>(
        worker_id: &'a str,
        trust: WorkerTrust,
        class: DataClass,
        model: &'a ModelSelection,
        epoch: u64,
        project: &'a ProjectPlacementPolicy,
    ) -> WorkerEligibilityInput<'a> {
        WorkerEligibilityInput {
            worker_id,
            trust,
            data_class: class,
            project_policy: project,
            model,
            coordinator_policy_epoch: epoch,
        }
    }

    #[test]
    fn secret_forces_local_only() {
        let reg = CapabilityRegistry::new();
        let model = ModelSelection::from_request("qwen:7b", None);
        let project = ProjectPlacementPolicy::default();
        assert_eq!(
            evaluate_worker_eligibility(
                &reg,
                &input(
                    "w1",
                    WorkerTrust::OwnerControlledEstate,
                    DataClass::Secret,
                    &model,
                    0,
                    &project
                ),
                Utc::now()
            )
            .unwrap_err(),
            PlacementReason::SecretLocalOnly
        );
    }

    #[test]
    fn external_untrusted_denies_repository_source() {
        let mut reg = CapabilityRegistry::new();
        seed_registry(&mut reg, "w1", 0);
        let model = ModelSelection::from_request("qwen:7b", None);
        let project = ProjectPlacementPolicy::default();
        assert_eq!(
            evaluate_worker_eligibility(
                &reg,
                &input(
                    "w1",
                    WorkerTrust::ExternalUntrusted,
                    DataClass::RepositorySource,
                    &model,
                    0,
                    &project
                ),
                Utc::now()
            )
            .unwrap_err(),
            PlacementReason::WorkerTrustInsufficient
        );
    }

    #[test]
    fn expired_capability_makes_worker_ineligible() {
        let mut reg = CapabilityRegistry::new();
        seed_registry(&mut reg, "w1", 0);
        let model = ModelSelection::from_request("qwen:7b", None);
        let project = ProjectPlacementPolicy::default();
        let future = Utc::now() + Duration::hours(2);
        assert_eq!(
            evaluate_worker_eligibility(
                &reg,
                &input(
                    "w1",
                    WorkerTrust::OwnerControlledEstate,
                    DataClass::RepositorySource,
                    &model,
                    0,
                    &project
                ),
                future
            )
            .unwrap_err(),
            PlacementReason::CapabilityStale
        );
    }

    #[test]
    fn stale_revocation_epoch_blocks_dispatch() {
        let mut reg = CapabilityRegistry::new();
        seed_registry(&mut reg, "w1", 2);
        let model = ModelSelection::from_request("qwen:7b", None);
        let project = ProjectPlacementPolicy::default();
        assert_eq!(
            evaluate_worker_eligibility(
                &reg,
                &input(
                    "w1",
                    WorkerTrust::OwnerControlledEstate,
                    DataClass::RepositorySource,
                    &model,
                    5,
                    &project
                ),
                Utc::now()
            )
            .unwrap_err(),
            PlacementReason::WorkerRevoked
        );
    }

    #[test]
    fn uncached_worker_fails_closed_until_capabilities_are_probed() {
        let reg = CapabilityRegistry::new();
        let model = ModelSelection::from_request("qwen:7b", None);
        let project = ProjectPlacementPolicy::default();
        assert_eq!(
            evaluate_worker_eligibility(
                &reg,
                &input(
                    "w1",
                    WorkerTrust::OwnerControlledEstate,
                    DataClass::RepositorySource,
                    &model,
                    0,
                    &project
                ),
                Utc::now()
            )
            .unwrap_err(),
            PlacementReason::CapabilityUnavailable
        );
    }

    #[test]
    fn owner_estate_with_fresh_caps_is_eligible() {
        let mut reg = CapabilityRegistry::new();
        seed_registry(&mut reg, "w1", 0);
        let model = ModelSelection::from_request("qwen:7b", None);
        let project = ProjectPlacementPolicy::default();
        assert!(evaluate_worker_eligibility(
            &reg,
            &input(
                "w1",
                WorkerTrust::OwnerControlledEstate,
                DataClass::RepositorySource,
                &model,
                0,
                &project
            ),
            Utc::now()
        )
        .is_ok());
    }

    #[test]
    fn quarantined_worker_is_worker_quarantined_not_revoked() {
        let mut reg = CapabilityRegistry::new();
        seed_registry(&mut reg, "w1", 0);
        assert!(reg.force_quarantine("w1"));
        let model = ModelSelection::from_request("qwen:7b", None);
        let project = ProjectPlacementPolicy::default();
        assert_eq!(
            evaluate_worker_eligibility(
                &reg,
                &input(
                    "w1",
                    WorkerTrust::OwnerControlledEstate,
                    DataClass::RepositorySource,
                    &model,
                    0,
                    &project
                ),
                Utc::now()
            )
            .unwrap_err(),
            PlacementReason::WorkerQuarantined
        );
    }
}

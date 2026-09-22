//! Trust × data-class placement matrix (M5-3).

use lokai_domain::placement::{
    PlacementDecision, PlacementReason, ProjectPlacementPolicy, VerificationRequirement,
};
use lokai_domain::{DataClass, WorkerTrust};

/// Default V2 placement matrix. `Secret → LocalOnly` is enforced before this runs.
pub fn trust_permits_data_class(
    trust: WorkerTrust,
    class: DataClass,
    project: &ProjectPlacementPolicy,
) -> Result<(), PlacementReason> {
    if class == DataClass::Secret {
        return Err(PlacementReason::SecretLocalOnly);
    }

    match (class, trust) {
        (DataClass::Public, _) => Ok(()),

        (DataClass::RepositorySource, WorkerTrust::LocalMachine) => Ok(()),
        (DataClass::RepositorySource, WorkerTrust::OwnerControlledEstate) => Ok(()),
        (DataClass::RepositorySource, WorkerTrust::AdministrativelyManaged) => {
            if project.allow_repository_to_admin_managed {
                Ok(())
            } else {
                Err(PlacementReason::ProjectPolicyDenied)
            }
        }
        (DataClass::RepositorySource, WorkerTrust::ExternalUntrusted) => {
            Err(PlacementReason::WorkerTrustInsufficient)
        }

        (DataClass::SensitiveSource, WorkerTrust::LocalMachine) => Ok(()),
        (DataClass::SensitiveSource, WorkerTrust::OwnerControlledEstate) => {
            if project.allow_sensitive_to_owner_estate {
                Ok(())
            } else {
                Err(PlacementReason::ProjectPolicyDenied)
            }
        }
        (DataClass::SensitiveSource, WorkerTrust::AdministrativelyManaged) => {
            Err(PlacementReason::WorkerTrustInsufficient)
        }
        (DataClass::SensitiveSource, WorkerTrust::ExternalUntrusted) => {
            Err(PlacementReason::WorkerTrustInsufficient)
        }

        (DataClass::Secret, _) => Err(PlacementReason::SecretLocalOnly),
    }
}

/// Map trust matrix failure to dispatch-local-only vs hard deny.
pub fn placement_to_dispatch_local_only(reason: PlacementReason) -> bool {
    matches!(
        reason,
        PlacementReason::SecretLocalOnly
            | PlacementReason::WorkerTrustInsufficient
            | PlacementReason::ProjectPolicyDenied
            | PlacementReason::RedactionRequired
            | PlacementReason::RedactionFailed
    )
}

pub fn evaluate_trust_placement(
    trust: WorkerTrust,
    class: DataClass,
    project: &ProjectPlacementPolicy,
) -> PlacementDecision {
    if class == DataClass::Secret {
        return PlacementDecision::LocalOnly {
            reason: PlacementReason::SecretLocalOnly,
        };
    }
    match trust_permits_data_class(trust, class, project) {
        Ok(()) => PlacementDecision::Eligible {
            targets: vec![],
            required_verification: VerificationRequirement::None,
        },
        Err(reason) if placement_to_dispatch_local_only(reason.clone()) => {
            PlacementDecision::LocalOnly { reason }
        }
        Err(reason) => PlacementDecision::Denied { reason },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lokai_domain::DataClass;

    fn project_default() -> ProjectPlacementPolicy {
        ProjectPlacementPolicy::default()
    }

    #[test]
    fn secret_always_local_only() {
        assert!(matches!(
            evaluate_trust_placement(
                WorkerTrust::OwnerControlledEstate,
                DataClass::Secret,
                &project_default()
            ),
            PlacementDecision::LocalOnly {
                reason: PlacementReason::SecretLocalOnly
            }
        ));
    }

    #[test]
    fn sensitive_source_to_owner_controlled_estate() {
        assert!(trust_permits_data_class(
            WorkerTrust::OwnerControlledEstate,
            DataClass::SensitiveSource,
            &project_default()
        )
        .is_ok());
    }

    #[test]
    fn sensitive_source_to_external_worker_denied() {
        assert_eq!(
            trust_permits_data_class(
                WorkerTrust::ExternalUntrusted,
                DataClass::SensitiveSource,
                &project_default()
            )
            .unwrap_err(),
            PlacementReason::WorkerTrustInsufficient
        );
    }

    #[test]
    fn repository_source_under_default_policy_external_denied() {
        assert_eq!(
            trust_permits_data_class(
                WorkerTrust::ExternalUntrusted,
                DataClass::RepositorySource,
                &project_default()
            )
            .unwrap_err(),
            PlacementReason::WorkerTrustInsufficient
        );
    }

    #[test]
    fn repository_source_owner_estate_allowed() {
        assert!(trust_permits_data_class(
            WorkerTrust::OwnerControlledEstate,
            DataClass::RepositorySource,
            &project_default()
        )
        .is_ok());
    }

    #[test]
    fn project_policy_cannot_override_secret() {
        let mut project = project_default();
        project.allow_sensitive_to_owner_estate = true;
        let d =
            evaluate_trust_placement(WorkerTrust::ExternalUntrusted, DataClass::Secret, &project);
        assert!(matches!(
            d,
            PlacementDecision::LocalOnly {
                reason: PlacementReason::SecretLocalOnly
            }
        ));
    }

    #[test]
    fn sensitive_denied_when_project_disallows_owner() {
        let project = ProjectPlacementPolicy {
            allow_sensitive_to_owner_estate: false,
            ..Default::default()
        };
        assert_eq!(
            trust_permits_data_class(
                WorkerTrust::OwnerControlledEstate,
                DataClass::SensitiveSource,
                &project
            )
            .unwrap_err(),
            PlacementReason::ProjectPolicyDenied
        );
    }

    #[test]
    fn worker_trust_downgrade_blocks_sensitive_dispatch() {
        let project = project_default();
        assert!(trust_permits_data_class(
            WorkerTrust::OwnerControlledEstate,
            DataClass::SensitiveSource,
            &project
        )
        .is_ok());
        assert_eq!(
            trust_permits_data_class(
                WorkerTrust::ExternalUntrusted,
                DataClass::SensitiveSource,
                &project
            )
            .unwrap_err(),
            PlacementReason::WorkerTrustInsufficient
        );
    }

    #[test]
    fn ui_summary_for_secret_local_only() {
        let d = evaluate_trust_placement(
            WorkerTrust::ExternalUntrusted,
            DataClass::Secret,
            &project_default(),
        );
        let summary = d.ui_summary(
            Some(DataClass::Secret),
            Some(WorkerTrust::ExternalUntrusted),
        );
        assert_eq!(summary.decision, "local_only");
        assert_eq!(summary.reason_code, "secret_local_only");
    }
}

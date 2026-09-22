//! Unified policy engine (D1) — remote inference, data class, tool gates.
//!
//! Homelab uses [`PolicyMode::EstateStub`]: owner estate workers OK; circle jobs
//! denied until [`PolicyMode::Full`] ships with Circle (N2).

mod classify;
mod dispatch;
mod engine;
mod fabric;
mod hosted;
mod mode;
mod placement;
mod shell;
pub use hosted::HostedInferencePolicy;

pub use classify::{
    apply_data_class_floor, classification_from_data_class, classify_message_payloads,
    classify_session, classify_text_content, combine_classifications, data_class_name,
    data_class_rank, default_disclosure_tier, normalize_data_class_name, parse_data_class,
    policy_version, restrict_data_class,
};
pub use dispatch::{classification_from_session_data_class, PolicyDispatchGuard};
pub use engine::{PolicyEngine, PolicySettings};
pub use fabric::{Destination, FabricJobDraft, PolicyContext};
pub use mode::PolicyMode;
pub use placement::{
    evaluate_trust_placement, placement_to_dispatch_local_only, trust_permits_data_class,
};

#[cfg(test)]
mod integration_tests {
    use super::*;
    use tetonic_domain::DataClass;

    #[test]
    fn data_class_floor_coerces_looser_override() {
        let floor = classify_session(std::path::Path::new("/tmp/secrets"), None);
        assert_eq!(floor.class, DataClass::Secret);
        assert_eq!(
            apply_data_class_floor(floor.class, Some(DataClass::RepositorySource)),
            DataClass::Secret
        );
        assert_eq!(
            apply_data_class_floor(
                DataClass::RepositorySource,
                Some(DataClass::SensitiveSource)
            ),
            DataClass::SensitiveSource
        );
        assert_eq!(
            apply_data_class_floor(DataClass::RepositorySource, Some(DataClass::Public)),
            DataClass::RepositorySource
        );
    }

    #[test]
    fn remote_policy_callback_matches_engine() {
        let engine = PolicyEngine::default();
        let guard = PolicyDispatchGuard::from_engine(engine);
        use tetonic_domain::{
            Classification, ClassificationSource, DispatchDestination, DispatchGuard,
            DispatchRequest,
        };
        let session =
            Classification::new(DataClass::Secret, vec![ClassificationSource::SessionFloor]);
        let req = DispatchRequest {
            payload: None,
            session: Some(session),
            destination: DispatchDestination::RemoteWorker {
                worker_id: "gpu-box".into(),
            },
            post_redaction: false,
            worker_trust: Some(tetonic_domain::WorkerTrust::OwnerControlledEstate),
            project_policy: tetonic_domain::ProjectPlacementPolicy::default(),
        };
        assert_eq!(
            guard.evaluate(&req).unwrap(),
            tetonic_domain::DispatchDecision::LocalOnly
        );

        let session = Classification::new(
            DataClass::RepositorySource,
            vec![ClassificationSource::SessionFloor],
        );
        let req = DispatchRequest {
            payload: None,
            session: Some(session),
            destination: DispatchDestination::RemoteWorker {
                worker_id: "gpu-box".into(),
            },
            post_redaction: false,
            worker_trust: Some(tetonic_domain::WorkerTrust::OwnerControlledEstate),
            project_policy: tetonic_domain::ProjectPlacementPolicy::default(),
        };
        assert!(guard.evaluate(&req).unwrap().allows_remote());
    }
}

//! Common dispatch guard implementation (M2-2).

use std::sync::Arc;

use lokai_domain::{
    ClassificationSource, DataClass, DispatchDecision, DispatchDenied, DispatchDestination,
    DispatchGuard, DispatchRequest,
};

use crate::classify::{classification_from_data_class, restrict_data_class};
use crate::engine::PolicyEngine;
use crate::fabric::{Destination, FabricJobDraft, PolicyContext};

/// Policy-backed dispatch guard — single gate for remote inference placement.
pub struct PolicyDispatchGuard {
    engine: Arc<PolicyEngine>,
}

impl PolicyDispatchGuard {
    pub fn new(engine: Arc<PolicyEngine>) -> Self {
        Self { engine }
    }

    pub fn from_engine(engine: PolicyEngine) -> Self {
        Self {
            engine: Arc::new(engine),
        }
    }

    fn effective_class(request: &DispatchRequest) -> Option<DataClass> {
        let payload = request.payload.as_ref().map(|c| c.class);
        let session = request.session.as_ref().map(|c| c.class);
        match (payload, session) {
            (None, None) => None,
            (Some(p), None) => Some(p),
            (None, Some(s)) => Some(s),
            (Some(p), Some(s)) => Some(restrict_data_class(s, p)),
        }
    }

    fn map_destination(dest: &DispatchDestination) -> Destination {
        match dest {
            DispatchDestination::Local => Destination::Local,
            DispatchDestination::RemoteWorker { worker_id } => Destination::EstateWorker {
                id: worker_id.clone(),
            },
            DispatchDestination::CirclePeer { circle_id, peer_id } => Destination::CirclePeer {
                circle_id: circle_id.clone(),
                peer_id: peer_id.clone(),
            },
        }
    }
}

impl DispatchGuard for PolicyDispatchGuard {
    fn project_placement_policy(&self) -> lokai_domain::ProjectPlacementPolicy {
        self.engine.project_placement_policy()
    }

    fn evaluate(&self, request: &DispatchRequest) -> Result<DispatchDecision, DispatchDenied> {
        let Some(class) = Self::effective_class(request) else {
            return Err(DispatchDenied::new(
                "missing_classification",
                "missing or unknown classification — remote dispatch denied",
            ));
        };

        if class == DataClass::Secret {
            return Ok(DispatchDecision::LocalOnly);
        }

        if matches!(
            request.destination,
            DispatchDestination::RemoteWorker { .. }
        ) {
            let Some(trust) = request.worker_trust else {
                return Err(DispatchDenied::new(
                    "worker_trust_missing",
                    "missing worker trust — remote dispatch denied",
                ));
            };
            let project = self.project_placement_policy();
            if let Err(reason) = crate::placement::trust_permits_data_class(trust, class, &project)
            {
                if crate::placement::placement_to_dispatch_local_only(reason) {
                    return Ok(DispatchDecision::LocalOnly);
                }
                return Err(DispatchDenied::new(
                    "worker_trust_insufficient",
                    "worker trust tier does not permit this data class",
                ));
            }
        }

        let dest = Self::map_destination(&request.destination);
        if matches!(dest, Destination::Local) {
            return Ok(DispatchDecision::RemoteAllowed);
        }

        let disclosure = lokai_domain::DisclosureTier::Auditable;
        let ctx = PolicyContext {
            mode: self.engine.mode(),
            session_data_class: request.session.as_ref().map(|c| c.class).unwrap_or(class),
        };
        let draft = FabricJobDraft {
            data_class: class,
            disclosure_tier: disclosure,
            destination: dest.clone(),
            is_circle_job: matches!(dest, Destination::CirclePeer { .. }),
        };
        let decision = self.engine.check_remote_inference(&ctx, &draft);
        if !decision.allowed() {
            let reason = match &decision {
                lokai_domain::PolicyDecision::Deny { reason } => reason.as_str(),
                _ => "remote dispatch denied by policy",
            };
            return if class == DataClass::Secret {
                Ok(DispatchDecision::LocalOnly)
            } else {
                Err(DispatchDenied::new("policy_denied", reason))
            };
        }

        if request.post_redaction {
            Ok(DispatchDecision::RemoteAllowedWithRedaction)
        } else {
            Ok(DispatchDecision::RemoteAllowed)
        }
    }
}

/// Build session classification from persisted data class string.
pub fn classification_from_session_data_class(class: DataClass) -> lokai_domain::Classification {
    classification_from_data_class(class, ClassificationSource::SessionFloor)
}

#[cfg(test)]
mod adversarial_tests {
    use std::sync::Arc;

    use super::*;
    use lokai_domain::{Classification, ClassificationSource, DataClass, ProjectPlacementPolicy};

    fn guard() -> PolicyDispatchGuard {
        PolicyDispatchGuard::from_engine(PolicyEngine::default())
    }

    fn worker_req(
        payload: Option<Classification>,
        session: Option<Classification>,
    ) -> DispatchRequest {
        DispatchRequest {
            payload,
            session,
            destination: DispatchDestination::RemoteWorker {
                worker_id: "gpu-box".into(),
            },
            post_redaction: false,
            worker_trust: Some(lokai_domain::WorkerTrust::OwnerControlledEstate),
            project_policy: lokai_domain::ProjectPlacementPolicy::default(),
        }
    }

    #[test]
    fn project_policy_disabling_sensitive_blocks_owner_estate() {
        let engine = PolicyEngine::default();
        let mut placement = engine.project_placement_policy();
        placement.allow_sensitive_to_owner_estate = false;
        engine.set_project_placement_policy(placement);
        let guard = PolicyDispatchGuard::new(Arc::new(engine));
        let session = Classification::new(
            DataClass::SensitiveSource,
            vec![ClassificationSource::SessionFloor],
        );
        let d = guard
            .evaluate(&DispatchRequest {
                payload: None,
                session: Some(session),
                destination: DispatchDestination::RemoteWorker {
                    worker_id: "gpu-box".into(),
                },
                post_redaction: false,
                worker_trust: Some(lokai_domain::WorkerTrust::OwnerControlledEstate),
                project_policy: ProjectPlacementPolicy::default(),
            })
            .unwrap();
        assert_eq!(d, DispatchDecision::LocalOnly);
    }

    #[test]
    fn secret_only_in_earlier_message_blocks_remote() {
        let payload = Classification::new(
            DataClass::Secret,
            vec![ClassificationSource::ContentHeuristic],
        );
        let session = Classification::new(
            DataClass::RepositorySource,
            vec![ClassificationSource::DefaultRule],
        );
        let d = guard()
            .evaluate(&worker_req(Some(payload), Some(session)))
            .unwrap();
        assert_eq!(d, DispatchDecision::LocalOnly);
    }

    #[test]
    fn secret_from_tool_result_blocks_remote() {
        let payload = Classification::new(
            DataClass::Secret,
            vec![
                ClassificationSource::SecretDetector,
                ClassificationSource::DerivedFromInput,
            ],
        );
        let d = guard().evaluate(&worker_req(Some(payload), None)).unwrap();
        assert_eq!(d, DispatchDecision::LocalOnly);
    }

    #[test]
    fn secret_combined_with_public_stays_local() {
        let payload = Classification::new(
            DataClass::Secret,
            vec![ClassificationSource::AggregatedPayload],
        );
        let session =
            Classification::new(DataClass::Public, vec![ClassificationSource::DefaultRule]);
        let d = guard()
            .evaluate(&worker_req(Some(payload), Some(session)))
            .unwrap();
        assert_eq!(d, DispatchDecision::LocalOnly);
    }

    #[test]
    fn missing_worker_trust_does_not_allow_remote_sensitive() {
        let session = Classification::new(
            DataClass::SensitiveSource,
            vec![ClassificationSource::SessionFloor],
        );
        let outcome = guard().evaluate(&DispatchRequest {
            payload: None,
            session: Some(session),
            destination: DispatchDestination::RemoteWorker {
                worker_id: "gpu-box".into(),
            },
            post_redaction: false,
            worker_trust: None,
            project_policy: ProjectPlacementPolicy::default(),
        });
        let allows_remote = match &outcome {
            Ok(d) => d.allows_remote(),
            Err(_) => false,
        };
        assert!(!allows_remote);
    }

    #[test]
    fn missing_classification_denies_remote() {
        let err = guard().evaluate(&worker_req(None, None)).unwrap_err();
        assert_eq!(err.reason_code, "missing_classification");
    }

    #[test]
    fn repository_source_allows_estate_worker() {
        let session = Classification::new(
            DataClass::RepositorySource,
            vec![ClassificationSource::SessionFloor],
        );
        let d = guard().evaluate(&worker_req(None, Some(session))).unwrap();
        assert_eq!(d, DispatchDecision::RemoteAllowed);
    }

    #[test]
    fn manual_override_cannot_dispatch_secret() {
        let session = Classification::new(
            DataClass::Secret,
            vec![ClassificationSource::UserDesignation],
        );
        let payload = Classification::new(
            DataClass::Public,
            vec![ClassificationSource::ExplicitReclassification],
        );
        let d = guard()
            .evaluate(&worker_req(Some(payload), Some(session)))
            .unwrap();
        assert_eq!(d, DispatchDecision::LocalOnly);
    }

    #[test]
    fn external_untrusted_blocks_repository_source() {
        let req = DispatchRequest {
            payload: None,
            session: Some(Classification::new(
                DataClass::RepositorySource,
                vec![ClassificationSource::SessionFloor],
            )),
            destination: DispatchDestination::RemoteWorker {
                worker_id: "external".into(),
            },
            post_redaction: false,
            worker_trust: Some(lokai_domain::WorkerTrust::ExternalUntrusted),
            project_policy: lokai_domain::ProjectPlacementPolicy::default(),
        };
        let d = guard().evaluate(&req).unwrap();
        assert_eq!(d, DispatchDecision::LocalOnly);
    }
}

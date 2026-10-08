//! Application service implementations: initialization, run management,
//! policy, estate and capacity. `RunSupervisor` is the sole authority for
//! run/task/attempt transitions.

pub use crate::execution::{
    DefaultRunService, FinalizationEffectDriver, FinalizationPolicy, RunService,
};

use crate::{commands::*, errors::AppError, events::*};
use async_trait::async_trait;
use std::sync::Arc;

pub use crate::host::initialization::{DefaultInitializationService, InitializationService};

// ---------------------------------------------------------------------------
// Policy services
// ---------------------------------------------------------------------------

#[async_trait]
pub trait PolicyService: Send + Sync {
    async fn get_policy(&self, cmd: GetPolicyCommand) -> Result<GetPolicyResultPayload, AppError>;
    async fn set_policy(&self, cmd: SetPolicyCommand) -> Result<(), AppError>;
    async fn reclassify_session(
        &self,
        cmd: ReclassifySessionCommand,
    ) -> Result<ReclassifySessionResultPayload, AppError>;
}

pub struct DefaultPolicyService {
    policy: Arc<tetonic_policy::PolicyEngine>,
    store: Option<tetonic_memory::SharedStore>,
    events: Arc<dyn ApplicationEventSink>,
}

impl DefaultPolicyService {
    pub fn new(
        policy: Arc<tetonic_policy::PolicyEngine>,
        store: Option<tetonic_memory::SharedStore>,
        events: Arc<dyn ApplicationEventSink>,
    ) -> Self {
        Self {
            policy,
            store,
            events,
        }
    }
}

#[async_trait]
impl PolicyService for DefaultPolicyService {
    async fn get_policy(&self, cmd: GetPolicyCommand) -> Result<GetPolicyResultPayload, AppError> {
        let floor =
            tetonic_policy::classify_session(std::path::Path::new(&cmd.workspace_root), None);
        Ok(GetPolicyResultPayload {
            mode: self.policy.mode().as_str().to_string(),
            default_data_class: tetonic_policy::data_class_name(floor.class).to_string(),
            verify_allowed: self.policy.verify_allowed(),
            mutations_allowed: self.policy.mutations_allowed(),
            allow_sensitive_to_owner_estate: self
                .policy
                .project_placement_policy()
                .allow_sensitive_to_owner_estate,
            allow_repository_to_admin_managed: self
                .policy
                .project_placement_policy()
                .allow_repository_to_admin_managed,
        })
    }

    async fn set_policy(&self, cmd: SetPolicyCommand) -> Result<(), AppError> {
        let mut changed = false;
        let mode_for_event = cmd.mode.clone();

        if let Some(mode_str) = cmd.mode {
            let mode = tetonic_policy::PolicyMode::parse(&mode_str).ok_or_else(|| {
                AppError::InvalidRequest(format!("invalid policy mode: {mode_str}"))
            })?;
            self.policy.set_mode(mode);
            if let Some(s) = &self.store {
                s.write(move |db| db.set_policy_mode(mode.as_str()))
                    .await
                    .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?
                    .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?;
            }
            changed = true;
        }

        if let Some(verify_allowed) = cmd.verify_allowed {
            self.policy.set_verify_allowed(verify_allowed);
            if let Some(s) = &self.store {
                s.write(move |db| db.set_policy_verify_allowed(verify_allowed))
                    .await
                    .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?
                    .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?;
            }
            changed = true;
        }

        if let Some(mutations_allowed) = cmd.mutations_allowed {
            self.policy.set_mutations_allowed(mutations_allowed);
            if let Some(s) = &self.store {
                s.write(move |db| db.set_policy_mutations_allowed(mutations_allowed))
                    .await
                    .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?
                    .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?;
            }
            changed = true;
        }

        if cmd.allow_sensitive_to_owner_estate.is_some()
            || cmd.allow_repository_to_admin_managed.is_some()
        {
            let mut placement = self.policy.project_placement_policy();
            if let Some(v) = cmd.allow_sensitive_to_owner_estate {
                placement.allow_sensitive_to_owner_estate = v;
            }
            if let Some(v) = cmd.allow_repository_to_admin_managed {
                placement.allow_repository_to_admin_managed = v;
            }
            self.policy.set_project_placement_policy(placement.clone());
            if let Some(s) = &self.store {
                s.write(move |db| db.set_project_placement_policy(&placement))
                    .await
                    .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?
                    .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?;
            }
            changed = true;
        }

        if !changed {
            return Err(AppError::InvalidRequest(
                "policy/set requires at least one of: mode, verify_allowed, mutations_allowed, allow_sensitive_to_owner_estate, allow_repository_to_admin_managed"
                    .into(),
            ));
        }

        emit(
            &self.events,
            ApplicationEvent::PolicyUpdated {
                mode: mode_for_event,
            },
        );

        Ok(())
    }

    async fn reclassify_session(
        &self,
        cmd: ReclassifySessionCommand,
    ) -> Result<ReclassifySessionResultPayload, AppError> {
        let Some(class) = tetonic_policy::parse_data_class(&cmd.data_class) else {
            return Err(AppError::InvalidRequest(format!(
                "invalid data_class: {}",
                cmd.data_class
            )));
        };
        if cmd.reason.trim().is_empty() {
            return Err(AppError::InvalidRequest(
                "reclassification reason is required".into(),
            ));
        }
        let previous = if let Some(s) = &self.store {
            let session_id = cmd.session_id.clone();
            let reason = cmd.reason.clone();
            s.write(move |db| {
                let prev = db.session_data_class(&session_id).ok().flatten();
                let _canonical = db
                    .reclassify_session(
                        &session_id,
                        tetonic_policy::data_class_name(class),
                        &reason,
                        "explicit_reclassification",
                    )
                    .map_err(|e| AppError::PersistenceFailed(format!("reclassify: {e}")))?;
                Ok::<_, AppError>(prev)
            })
            .await
            .map_err(|e| AppError::PersistenceFailed(format!("reclassify: {e}")))??
        } else {
            None
        };
        emit(
            &self.events,
            ApplicationEvent::LogDiagnostic {
                session_id: Some(cmd.session_id.clone()),
                agent_id: None,
                message: format!(
                    "classification reclassified to {} (reason: {})",
                    tetonic_policy::data_class_name(class),
                    cmd.reason
                ),
            },
        );
        Ok(ReclassifySessionResultPayload {
            data_class: tetonic_policy::data_class_name(class).to_string(),
            previous_data_class: previous,
        })
    }
}
// ---------------------------------------------------------------------------
// Slice 6 — Estate
// ---------------------------------------------------------------------------

#[async_trait]
pub trait EstateService: Send + Sync {
    fn get_estate_status(
        &self,
        cmd: GetEstateStatusCommand,
    ) -> Result<EstateStatusResultPayload, AppError>;
    async fn enroll_worker(
        &self,
        cmd: EnrollWorkerCommand,
    ) -> Result<EnrollWorkerResultPayload, AppError>;
    async fn remove_worker(
        &self,
        cmd: RemoveWorkerCommand,
    ) -> Result<RemoveWorkerResultPayload, AppError>;
}

pub struct DefaultEstateService {
    store: Option<tetonic_memory::SharedStore>,
    policy: Option<Arc<tetonic_policy::PolicyEngine>>,
}

impl DefaultEstateService {
    pub fn new(
        store: Option<tetonic_memory::SharedStore>,
        policy: Option<Arc<tetonic_policy::PolicyEngine>>,
    ) -> Self {
        Self { store, policy }
    }
}

#[async_trait]
impl EstateService for DefaultEstateService {
    fn get_estate_status(
        &self,
        cmd: GetEstateStatusCommand,
    ) -> Result<EstateStatusResultPayload, AppError> {
        let workers = self
            .store
            .as_ref()
            .map(|s| {
                s.read_sync(|db| db.list_worker_enrollments())
                    .unwrap_or_else(|_| Ok(vec![]))
                    .unwrap_or_default()
                    .len() as u32
            })
            .unwrap_or(0);
        let policy_mode = self
            .policy
            .as_ref()
            .map(|p| p.mode().as_str().to_string())
            .unwrap_or_default();
        Ok(EstateStatusResultPayload {
            policy_mode,
            workers_enrolled: workers,
            fabric_pooled: cmd.fabric_pooled,
        })
    }

    async fn enroll_worker(
        &self,
        cmd: EnrollWorkerCommand,
    ) -> Result<EnrollWorkerResultPayload, AppError> {
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| AppError::InvalidRequest("enrollment requires lokai.db store".into()))?;
        crate::estate_enrollment::enroll_worker(store, cmd).await
    }

    async fn remove_worker(
        &self,
        cmd: RemoveWorkerCommand,
    ) -> Result<RemoveWorkerResultPayload, AppError> {
        let store = self.store.as_ref().ok_or_else(|| {
            AppError::InvalidRequest("worker removal requires lokai.db store".into())
        })?;
        crate::estate_enrollment::remove_worker(store, cmd).await
    }
}

pub use crate::capacity_service::{CapacityService, DefaultCapacityService};

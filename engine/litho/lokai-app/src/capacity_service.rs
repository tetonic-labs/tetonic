//! Capacity profile and optimize service (Slice 6).

use crate::commands::*;
use crate::errors::AppError;
use crate::events::{emit, ApplicationEvent, ApplicationEventSink};
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub trait CapacityService: Send + Sync {
    fn begin_optimize(
        &self,
        cmd: BeginOptimizeCommand,
    ) -> Result<BeginOptimizeResultPayload, AppError>;
    fn cancel_optimize(&self, cmd: CancelOptimizeCommand) -> Result<bool, AppError>;
    async fn get_capacity_status(
        &self,
        cmd: CapacityStatusCommand,
    ) -> Result<CapacityStatusResultPayload, AppError>;
    async fn get_capacity_doctor(
        &self,
        cmd: CapacityDoctorCommand,
    ) -> Result<CapacityDoctorResultPayload, AppError>;
    fn list_profiles(
        &self,
        cmd: ListProfilesCommand,
    ) -> Result<Vec<lokai_capacity::ProfileSummary>, AppError>;
    fn activate_profile(
        &self,
        cmd: ActivateProfileCommand,
    ) -> Result<ActivateProfileResultPayload, AppError>;
    fn rollback_profile(
        &self,
        cmd: RollbackProfileCommand,
    ) -> Result<RollbackProfileResultPayload, AppError>;
    fn export_profile(
        &self,
        cmd: ExportProfileCommand,
    ) -> Result<lokai_capacity::RuntimeProfile, AppError>;
    fn import_profile(
        &self,
        cmd: ImportProfileCommand,
    ) -> Result<ImportProfileResultPayload, AppError>;
    async fn run_optimize(
        &self,
        cmd: RunOptimizeCommand,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<lokai_capacity::OptimizeOutcome, AppError>;
    fn get_capacity_job(
        &self,
        cmd: GetCapacityJobCommand,
    ) -> Result<GetCapacityJobResultPayload, AppError>;
    /// Store-backed doctor snapshot for turn admission (H2-1). No live Ollama probe.
    fn snapshot_admission_status(&self, node_id: &str) -> Option<lokai_capacity::CapacityStatus>;
}

pub struct DefaultCapacityService {
    store: Option<lokai_memory::SharedStore>,
    events: Arc<dyn ApplicationEventSink>,
}

impl DefaultCapacityService {
    pub fn new(
        store: Option<lokai_memory::SharedStore>,
        events: Arc<dyn ApplicationEventSink>,
    ) -> Self {
        Self { store, events }
    }
}

#[async_trait]
impl CapacityService for DefaultCapacityService {
    fn begin_optimize(
        &self,
        cmd: BeginOptimizeCommand,
    ) -> Result<BeginOptimizeResultPayload, AppError> {
        if cmd.sessions_busy {
            return Err(AppError::InvalidRequest(
                "capacity optimize blocked while session turns are active".into(),
            ));
        }
        if cmd.capacity_busy {
            return Err(AppError::InvalidRequest(
                "capacity optimize already running".into(),
            ));
        }
        let job_id = lokai_memory::new_id("capjob");
        emit(
            &self.events,
            ApplicationEvent::CapacityProgress {
                job_id: job_id.clone(),
                progress_pct: 0,
                message: "queued".into(),
            },
        );
        Ok(BeginOptimizeResultPayload {
            job_id,
            depth: lokai_capacity::OptimizeDepth::parse(&cmd.depth),
            auto_apply: cmd.auto_apply,
        })
    }

    fn cancel_optimize(&self, cmd: CancelOptimizeCommand) -> Result<bool, AppError> {
        if !cmd.capacity_busy {
            return Ok(false);
        }
        if let Some(ref want) = cmd.requested_job_id {
            if cmd.active_job_id.as_deref() != Some(want.as_str()) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    async fn get_capacity_status(
        &self,
        cmd: CapacityStatusCommand,
    ) -> Result<CapacityStatusResultPayload, AppError> {
        let store = self.store.as_ref().ok_or_else(|| {
            AppError::InvalidRequest("capacity status requires lokai.db store".into())
        })?;
        let (status, _) = lokai_capacity::capacity_status_for_store(
            store,
            &cmd.node_id,
            &cmd.client,
            cmd.ollama_version,
        )
        .await;
        Ok(CapacityStatusResultPayload { status })
    }

    async fn get_capacity_doctor(
        &self,
        cmd: CapacityDoctorCommand,
    ) -> Result<CapacityDoctorResultPayload, AppError> {
        let store = self.store.as_ref().ok_or_else(|| {
            AppError::InvalidRequest("capacity doctor requires lokai.db store".into())
        })?;
        let (status, diagnosis) = lokai_capacity::capacity_status_for_store(
            store,
            &cmd.node_id,
            &cmd.client,
            cmd.ollama_version,
        )
        .await;
        Ok(CapacityDoctorResultPayload { status, diagnosis })
    }

    fn list_profiles(
        &self,
        cmd: ListProfilesCommand,
    ) -> Result<Vec<lokai_capacity::ProfileSummary>, AppError> {
        let store = self.store.as_ref().ok_or_else(|| {
            AppError::InvalidRequest("capacity profiles require lokai.db store".into())
        })?;
        let role = lokai_capacity::parse_tier_role(&cmd.role)
            .ok_or_else(|| AppError::InvalidRequest(format!("unknown role: {}", cmd.role)))?;
        store
            .read_sync({
                let node_id = cmd.node_id.clone();
                move |db| {
                    lokai_capacity::ProfileStore::new(db)
                        .list_summaries(&node_id, role)
                        .map_err(|e| AppError::PersistenceFailed(format!("list profiles: {e}")))
                }
            })
            .map_err(|e| AppError::PersistenceFailed(format!("list profiles: {e}")))?
    }

    fn activate_profile(
        &self,
        cmd: ActivateProfileCommand,
    ) -> Result<ActivateProfileResultPayload, AppError> {
        let store = self.store.as_ref().ok_or_else(|| {
            AppError::InvalidRequest("capacity profiles require lokai.db store".into())
        })?;
        let role = lokai_capacity::parse_tier_role(&cmd.role)
            .ok_or_else(|| AppError::InvalidRequest(format!("unknown role: {}", cmd.role)))?;
        let defaults = store
            .write_sync({
                let node_id = cmd.node_id.clone();
                let profile_id = cmd.profile_id.clone();
                move |db| {
                    lokai_capacity::ProfileStore::new(db)
                        .activate(&node_id, role, &profile_id)
                        .map_err(|e| AppError::InvalidRequest(format!("activate: {e}")))?;
                    Ok::<_, AppError>(lokai_capacity::load_inference_defaults(db, &node_id))
                }
            })
            .map_err(|e| AppError::PersistenceFailed(format!("activate: {e}")))??;
        Ok(ActivateProfileResultPayload {
            profile_id: cmd.profile_id,
            model_fast: defaults.model_fast,
            num_ctx: defaults.num_ctx,
        })
    }

    fn rollback_profile(
        &self,
        cmd: RollbackProfileCommand,
    ) -> Result<RollbackProfileResultPayload, AppError> {
        let store = self.store.as_ref().ok_or_else(|| {
            AppError::InvalidRequest("capacity profiles require lokai.db store".into())
        })?;
        let role = lokai_capacity::parse_tier_role(&cmd.role)
            .ok_or_else(|| AppError::InvalidRequest(format!("unknown role: {}", cmd.role)))?;
        let (prev, defaults) = store
            .write_sync({
                let node_id = cmd.node_id.clone();
                move |db| {
                    let prev = lokai_capacity::ProfileStore::new(db)
                        .rollback(&node_id, role)
                        .map_err(|e| AppError::InvalidRequest(format!("rollback: {e}")))?;
                    let defaults = lokai_capacity::load_inference_defaults(db, &node_id);
                    Ok::<_, AppError>((prev, defaults))
                }
            })
            .map_err(|e| AppError::PersistenceFailed(format!("rollback: {e}")))??;
        Ok(RollbackProfileResultPayload {
            profile_id: prev.id,
            model_fast: defaults.model_fast,
            num_ctx: defaults.num_ctx,
        })
    }

    fn export_profile(
        &self,
        cmd: ExportProfileCommand,
    ) -> Result<lokai_capacity::RuntimeProfile, AppError> {
        let store = self.store.as_ref().ok_or_else(|| {
            AppError::InvalidRequest("capacity profiles require lokai.db store".into())
        })?;
        store
            .read_sync({
                let profile_id = cmd.profile_id.clone();
                move |db| {
                    lokai_capacity::ProfileStore::new(db)
                        .get(&profile_id)
                        .map_err(|e| AppError::PersistenceFailed(format!("export: {e}")))
                }
            })
            .map_err(|e| AppError::PersistenceFailed(format!("export: {e}")))??
            .ok_or_else(|| {
                AppError::InvalidRequest(format!("unknown profile id `{}`", cmd.profile_id))
            })
    }

    fn import_profile(
        &self,
        cmd: ImportProfileCommand,
    ) -> Result<ImportProfileResultPayload, AppError> {
        let store = self.store.as_ref().ok_or_else(|| {
            AppError::InvalidRequest("capacity profiles require lokai.db store".into())
        })?;
        let mut profile: lokai_capacity::RuntimeProfile = serde_json::from_str(&cmd.json)
            .map_err(|e| AppError::InvalidRequest(format!("parse profile JSON: {e}")))?;
        profile
            .validate_schema()
            .map_err(|e| AppError::InvalidRequest(format!("invalid profile: {e}")))?;
        if cmd.refresh_fingerprint {
            profile.hardware =
                lokai_capacity::detect_hardware(profile.hardware.ollama_version.clone());
        }
        let mut activated = false;
        store
            .write_sync({
                let profile = profile.clone();
                let activate = cmd.activate;
                move |db| {
                    let ps = lokai_capacity::ProfileStore::new(db);
                    ps.append(&profile)
                        .map_err(|e| AppError::PersistenceFailed(format!("store profile: {e}")))?;
                    if activate {
                        ps.activate(lokai_capacity::LOCAL_NODE_ID, profile.role, &profile.id)
                            .map_err(|e| AppError::InvalidRequest(format!("activate: {e}")))?;
                    }
                    Ok::<_, AppError>(())
                }
            })
            .map_err(|e| AppError::PersistenceFailed(format!("store profile: {e}")))??;
        if cmd.activate {
            activated = true;
        }
        Ok(ImportProfileResultPayload {
            profile_id: profile.id.clone(),
            activated,
            model: profile.recipe.estate_model.clone(),
            num_ctx: profile.recipe.num_ctx,
        })
    }

    async fn run_optimize(
        &self,
        cmd: RunOptimizeCommand,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<lokai_capacity::OptimizeOutcome, AppError> {
        let begin = if let Some(b) = cmd.prebegin {
            b
        } else {
            self.begin_optimize(BeginOptimizeCommand {
                sessions_busy: cmd.sessions_busy,
                capacity_busy: cmd.capacity_busy,
                depth: cmd.depth.clone(),
                auto_apply: cmd.auto_apply,
            })?
        };
        let store = self.store.as_ref().ok_or_else(|| {
            AppError::InvalidRequest("capacity optimize requires lokai.db store".into())
        })?;
        let job_id = begin.job_id.clone();
        let opts = lokai_capacity::OptimizeOptions {
            depth: begin.depth,
            auto_apply: begin.auto_apply,
            base_models: vec![],
        };
        let job_json = serde_json::to_string(&opts)
            .map_err(|e| AppError::InvalidRequest(format!("serialize job: {e}")))?;
        store
            .write({
                let job_id = job_id.clone();
                let job_json = job_json.clone();
                move |db| {
                    db.upsert_capacity_job(&job_id, "running", &job_json)
                        .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))
                }
            })
            .await
            .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))??;

        let events = self.events.clone();
        let job_id_cb = job_id.clone();
        let outcome =
            match lokai_capacity::run_optimize(cmd.client, store, &job_id, opts, cancel, move |p| {
                emit(
                    &events,
                    ApplicationEvent::CapacityProgress {
                        job_id: job_id_cb.clone(),
                        progress_pct: p.percent.min(100) as u8,
                        message: format!("{} — {}", p.phase, p.message),
                    },
                );
            })
            .await
            {
                Ok(o) => o,
                Err(e) => lokai_capacity::OptimizeOutcome {
                    job_id: job_id.clone(),
                    state: if matches!(e, lokai_capacity::OptimizeError::Cancelled) {
                        lokai_capacity::JobState::Cancelled
                    } else {
                        lokai_capacity::JobState::Failed
                    },
                    profile_ids: vec![],
                    applied_profile_id: None,
                    error: Some(e.to_string()),
                },
            };

        let state_str = match outcome.state {
            lokai_capacity::JobState::Succeeded => "succeeded",
            lokai_capacity::JobState::Cancelled => "cancelled",
            _ => "failed",
        };
        if let Ok(json) = serde_json::to_string(&outcome) {
            let job_id = job_id.clone();
            let state_str = state_str.to_string();
            let _ = store
                .write(move |db| db.upsert_capacity_job(&job_id, &state_str, &json))
                .await;
        }
        Ok(outcome)
    }

    fn get_capacity_job(
        &self,
        cmd: GetCapacityJobCommand,
    ) -> Result<GetCapacityJobResultPayload, AppError> {
        let store = self.store.as_ref().ok_or_else(|| {
            AppError::InvalidRequest("capacity jobs require lokai.db store".into())
        })?;
        let row = store
            .read_sync({
                let job_id = cmd.job_id.clone();
                move |db| {
                    db.get_capacity_job(&job_id)
                        .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))
                }
            })
            .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))??
            .ok_or_else(|| AppError::InvalidRequest(format!("unknown job id `{}`", cmd.job_id)))?;
        let json = serde_json::from_str::<serde_json::Value>(&row.1).ok();
        Ok(GetCapacityJobResultPayload {
            job_id: cmd.job_id,
            state: row.0,
            json,
        })
    }

    fn snapshot_admission_status(&self, node_id: &str) -> Option<lokai_capacity::CapacityStatus> {
        let store = self.store.as_ref()?;
        store
            .read_sync({
                let node_id = node_id.to_string();
                move |db| Some(lokai_capacity::admission_status_from_store(db, &node_id))
            })
            .ok()
            .flatten()
    }
}

use super::prelude::*;

impl Daemon {
    pub(in crate::daemon) async fn estate_capacity_status(&self) -> Result<Value, RpcError> {
        let services = self.services()?;
        let status = services
            .app
            .daemon_capacity_status()
            .await
            .map_err(|e| RpcError::new(ErrorCode::InternalError, format!("app: {e}")))?;
        Ok(to_value(to_capacity_summary(status)))
    }

    pub(in crate::daemon) async fn estate_capacity_doctor(&self) -> Result<Value, RpcError> {
        let services = self.services()?;
        let (status, diagnosis) = services
            .app
            .daemon_capacity_doctor()
            .await
            .map_err(|e| RpcError::new(ErrorCode::InternalError, format!("app: {e}")))?;
        Ok(to_value(CapacityDoctorResult {
            status: to_capacity_summary(status),
            summary: diagnosis.summary,
            codes: diagnosis
                .codes
                .into_iter()
                .map(diagnosis_code_str)
                .collect(),
            recommendations: diagnosis.recommendations,
        }))
    }

    pub(in crate::daemon) async fn estate_capacity_optimize(
        &mut self,
        params: Value,
    ) -> Result<Value, RpcError> {
        if !rpc_control_plane_mutations_allowed() {
            return Err(RpcError::new(
                ErrorCode::InvalidRequest,
                "estate/capacity/optimize is disabled when LOKAI_STRICT_RPC=1",
            ));
        }
        let p: CapacityOptimizeParams = parse(params)?;
        let services = self.services()?;
        let begin = services
            .app
            .capacity
            .begin_optimize(tetonic_app::commands::BeginOptimizeCommand {
                sessions_busy: self.sessions_busy(),
                capacity_busy: self.capacity.busy.load(Ordering::Relaxed),
                depth: p.depth.as_deref().unwrap_or("quick").to_string(),
                auto_apply: p.auto_apply.unwrap_or(false),
            })
            .map_err(|e| match &e {
                tetonic_app::errors::AppError::InvalidRequest(_) => {
                    RpcError::new(ErrorCode::InvalidRequest, format!("{e}"))
                }
                _ => RpcError::new(ErrorCode::InternalError, format!("{e}")),
            })?;
        let job_id = begin.job_id.clone();
        let depth = begin.depth;
        let auto_apply = begin.auto_apply;

        self.capacity.cancel.store(false, Ordering::Relaxed);
        self.capacity.busy.store(true, Ordering::Relaxed);
        *self.capacity.job_id.lock().unwrap() = Some(job_id.clone());

        let busy = self.capacity.busy.clone();
        let cancel = self.capacity.cancel.clone();
        let pending_reload = self.capacity.pending_reload.clone();
        let notifier = self.notifier.clone();
        let job_id_spawn = job_id.clone();
        let depth_str = match depth {
            OptimizeDepth::Quick => "quick",
            OptimizeDepth::Full => "full",
        }
        .to_string();
        let app = services.app.clone();
        let prebegin = begin;

        tokio::spawn(async move {
            let outcome = app
                .daemon_run_capacity_optimize(depth_str, auto_apply, Some(prebegin), &cancel)
                .await;

            let outcome = match outcome {
                Ok(out) => out,
                Err(e) => OptimizeOutcome {
                    job_id: job_id_spawn.clone(),
                    state: JobState::Failed,
                    profile_ids: vec![],
                    applied_profile_id: None,
                    error: Some(e.to_string()),
                },
            };

            if outcome.applied_profile_id.is_some() {
                *pending_reload.lock().unwrap() = app.reload_inference_services();
            }

            busy.store(false, Ordering::Relaxed);
            notifier.notify(
                CAPACITY_SESSION,
                CAPACITY_AGENT,
                events::CAPACITY_PROGRESS,
                json!({
                    "job_id": job_id_spawn,
                    "phase": "done",
                    "percent": 100,
                    "message": outcome.error.clone().unwrap_or_else(|| "complete".into()),
                }),
            );
        });

        Ok(to_value(CapacityOptimizeResult {
            job_id: job_id.clone(),
            accepted: true,
        }))
    }

    pub(in crate::daemon) fn estate_capacity_cancel(
        &mut self,
        params: Value,
    ) -> Result<Value, RpcError> {
        let p: CapacityCancelParams = parse(params)?;
        let services = self.services()?;
        let active_job_id = self.capacity.job_id.lock().unwrap().clone();
        let accepted = services
            .app
            .capacity
            .cancel_optimize(tetonic_app::commands::CancelOptimizeCommand {
                requested_job_id: p.job_id.clone(),
                active_job_id,
                capacity_busy: self.capacity.busy.load(Ordering::Relaxed),
            })
            .map_err(|e| RpcError::new(ErrorCode::InternalError, format!("app: {e}")))?;
        if accepted {
            self.capacity.cancel.store(true, Ordering::Relaxed);
        }
        Ok(to_value(CapacityCancelResult {
            cancelled: accepted,
        }))
    }

    pub(in crate::daemon) fn apply_pending_capacity_reload(&mut self) {
        let pending = self.capacity.pending_reload.lock().unwrap().take();
        if pending.is_some() {
            self.reload_inference_services();
        }
    }

    pub(in crate::daemon) fn reload_inference_services(&mut self) {
        let Some(services) = self.services.as_mut() else {
            return;
        };
        let Some(defaults) = services.app.reload_inference_services() else {
            return;
        };
        services.model = defaults.model_fast.clone();
        services.model_hard = defaults.model_hard.clone();
        services.num_ctx = defaults.num_ctx;
        tracing::info!(
            model = %services.model,
            num_ctx = services.num_ctx,
            profile = ?defaults.profile_label,
            "reloaded inference defaults from capacity profile"
        );
    }

    pub(in crate::daemon) fn estate_capacity_profiles_list(
        &self,
        params: Value,
    ) -> Result<Value, RpcError> {
        let p: CapacityProfilesListParams = parse(params)?;
        let services = self.services()?;
        let node_id = p.node_id.as_deref().unwrap_or(LOCAL_NODE_ID);
        let role_name = p.role.as_deref().unwrap_or("coder").to_string();
        let _role = parse_tier_role(&role_name)
            .ok_or_else(|| RpcError::new(ErrorCode::InvalidParams, "unknown role"))?;
        let summaries = services
            .app
            .capacity
            .list_profiles(tetonic_app::commands::ListProfilesCommand {
                node_id: node_id.to_string(),
                role: role_name,
            })
            .map_err(crate::daemon::rpc::map::map_app_error)?;
        let profiles = summaries
            .into_iter()
            .map(|s| CapacityProfileListItem {
                id: s.id,
                label: s.label,
                created_at: s.created_at,
                gates_passed: s.gates_passed,
                estate_model: s.estate_model,
                num_ctx: s.num_ctx,
                active: s.active,
            })
            .collect();
        Ok(to_value(CapacityProfilesListResult { profiles }))
    }

    pub(in crate::daemon) async fn estate_capacity_profiles_activate(
        &mut self,
        params: Value,
    ) -> Result<Value, RpcError> {
        if !rpc_control_plane_mutations_allowed() {
            return Err(RpcError::new(
                ErrorCode::InvalidRequest,
                "estate/capacity/profiles/activate is disabled when LOKAI_STRICT_RPC=1",
            ));
        }
        let p: CapacityProfilesActivateParams = parse(params)?;
        let services = self.services()?;
        let node_id = p.node_id.as_deref().unwrap_or(LOCAL_NODE_ID);
        let role_name = p.role.as_deref().unwrap_or("coder").to_string();
        let _role = parse_tier_role(&role_name)
            .ok_or_else(|| RpcError::new(ErrorCode::InvalidParams, "unknown role"))?;
        services
            .app
            .capacity
            .activate_profile(tetonic_app::commands::ActivateProfileCommand {
                node_id: node_id.to_string(),
                role: role_name,
                profile_id: p.profile_id.clone(),
            })
            .map_err(crate::daemon::rpc::map::map_app_error)?;
        self.reload_inference_services();
        let status = self
            .services()?
            .app
            .daemon_capacity_status()
            .await
            .map_err(|e| RpcError::new(ErrorCode::InternalError, format!("app: {e}")))?;
        let status = to_capacity_summary(status);
        Ok(to_value(CapacityProfilesActivateResult {
            ok: true,
            profile_id: p.profile_id,
            status,
        }))
    }

    pub(in crate::daemon) async fn estate_capacity_profiles_rollback(
        &mut self,
        params: Value,
    ) -> Result<Value, RpcError> {
        if !rpc_control_plane_mutations_allowed() {
            return Err(RpcError::new(
                ErrorCode::InvalidRequest,
                "estate/capacity/profiles/rollback is disabled when LOKAI_STRICT_RPC=1",
            ));
        }
        let p: CapacityProfilesRollbackParams = parse(params)?;
        let services = self.services()?;
        let node_id = p.node_id.as_deref().unwrap_or(LOCAL_NODE_ID);
        let role_name = p.role.as_deref().unwrap_or("coder").to_string();
        let _role = parse_tier_role(&role_name)
            .ok_or_else(|| RpcError::new(ErrorCode::InvalidParams, "unknown role"))?;
        let prev = services
            .app
            .capacity
            .rollback_profile(tetonic_app::commands::RollbackProfileCommand {
                node_id: node_id.to_string(),
                role: role_name,
            })
            .map_err(crate::daemon::rpc::map::map_app_error)?;
        self.reload_inference_services();
        let status = self
            .services()?
            .app
            .daemon_capacity_status()
            .await
            .map_err(|e| RpcError::new(ErrorCode::InternalError, format!("app: {e}")))?;
        let status = to_capacity_summary(status);
        Ok(to_value(CapacityProfilesRollbackResult {
            ok: true,
            profile_id: prev.profile_id,
            status,
        }))
    }

    pub(in crate::daemon) fn estate_capacity_profiles_export(
        &self,
        params: Value,
    ) -> Result<Value, RpcError> {
        let p: CapacityProfilesExportParams = parse(params)?;
        let services = self.services()?;
        let profile = services
            .app
            .capacity
            .export_profile(tetonic_app::commands::ExportProfileCommand {
                profile_id: p.profile_id.clone(),
            })
            .map_err(crate::daemon::rpc::map::map_app_error)?;
        Ok(to_value(CapacityProfilesExportResult {
            profile: to_value(profile),
        }))
    }

    pub(in crate::daemon) fn estate_capacity_jobs_get(
        &self,
        params: Value,
    ) -> Result<Value, RpcError> {
        let p: CapacityJobsGetParams = parse(params)?;
        let services = self.services()?;
        let row = services
            .app
            .capacity
            .get_capacity_job(tetonic_app::commands::GetCapacityJobCommand {
                job_id: p.job_id.clone(),
            })
            .map_err(crate::daemon::rpc::map::map_app_error)?;
        Ok(to_value(CapacityJobsGetResult {
            job_id: row.job_id,
            state: row.state,
            json: row.json,
        }))
    }

    // ---- fabric/status -----------------------------------------------------
}

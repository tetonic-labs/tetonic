use super::prelude::*;

impl Daemon {
    pub(in crate::daemon) fn session_models(&mut self, params: Value) -> Result<Value, RpcError> {
        let p: tetonic_rpc::SessionInferenceParams = parse(params)?;
        let catalog = self
            .services()?
            .app
            .model_catalog(&p.session_id)
            .map_err(crate::daemon::rpc::map::map_session_error)?;
        Ok(to_value(tetonic_rpc::SessionModelsResult {
            revision: catalog.revision,
            models: catalog
                .models
                .into_iter()
                .map(|m| tetonic_rpc::ModelChoice {
                    id: m.id,
                    name: m.name,
                    provider_label: m.provider_label,
                    availability: m.availability,
                    current: m.current,
                    requires_auth: m.requires_auth,
                    provider_id: m.provider_id,
                })
                .collect(),
        }))
    }

    pub(in crate::daemon) fn session_select_model(
        &mut self,
        params: Value,
    ) -> Result<Value, RpcError> {
        let p: tetonic_rpc::SessionSelectModelParams = parse(params)?;
        let services = self.services()?;
        let selection = services
            .app
            .select_session_model(&p.session_id, &p.selection_id, p.expected_revision)
            .map_err(crate::daemon::rpc::map::map_session_error)?;
        Ok(to_value(tetonic_rpc::SessionInferenceResult {
            profile: selection.profile,
            model_fast: selection.model_fast,
            model_hard: selection.model_hard,
            revision: selection.revision,
            available_profiles: services.app.inference_profiles(),
        }))
    }

    pub(in crate::daemon) fn session_inference(
        &mut self,
        params: Value,
        change: bool,
    ) -> Result<Value, RpcError> {
        let services = self.services()?;
        let selection = if change {
            let p: tetonic_rpc::SessionSetInferenceParams = parse(params)?;
            services.app.change_session_inference(
                tetonic_app::inference_selection::ChangeInferenceCommand {
                    session_id: p.session_id,
                    profile: p.profile,
                    model_fast: p.model_fast,
                    model_hard: p.model_hard,
                    expected_revision: p.expected_revision,
                },
            )
        } else {
            let p: tetonic_rpc::SessionInferenceParams = parse(params)?;
            services.app.session_inference(&p.session_id)
        }
        .map_err(crate::daemon::rpc::map::map_session_error)?;
        Ok(to_value(tetonic_rpc::SessionInferenceResult {
            profile: selection.profile,
            model_fast: selection.model_fast,
            model_hard: selection.model_hard,
            revision: selection.revision,
            available_profiles: services.app.inference_profiles(),
        }))
    }
    pub(in crate::daemon) async fn session_start(
        &mut self,
        params: Value,
    ) -> Result<Value, RpcError> {
        let p: SessionStartParams = parse(params)?;
        let services = self.services()?;

        if self.draining.load(Ordering::Relaxed) {
            return Err(RpcError::new(
                ErrorCode::InvalidRequest,
                "daemon is shutting down",
            ));
        }

        let model = match p.model_tier.as_deref() {
            Some("hard") => services.model_hard.clone(),
            _ => services.model.clone(),
        };
        let cmd = tetonic_app::commands::StartSessionCommand {
            workspace_root: services.workspace_root.clone(),
            resume: p.resume,
            model_tier: p.model_tier.clone(),
            session_id: p.session_id.clone(),
            goal: p.goal.clone(),
            data_class: p.data_class.clone(),
            verify_cmd: p.verify_cmd.clone(),
            briefing: p.briefing,
            orchestration: p.orchestration.clone(),
            critic: p.critic,
            llm_router: p.llm_router,
            model_fast: Some(model),
            model_hard: Some(services.model_hard.clone()),
            session_max_steps: None,
            allow_shell: p.allow_shell,
            force_explain: None,
            auto_grant_approvals: None,
        };

        let result = services
            .app
            .sessions
            .start_session(cmd)
            .await
            .map_err(|e| match &e {
                tetonic_app::errors::AppError::InvalidRequest(_) => {
                    RpcError::new(ErrorCode::InvalidRequest, e.employee_message())
                }
                tetonic_app::errors::AppError::SessionNotFound(_)
                | tetonic_app::errors::AppError::SessionConflict => {
                    RpcError::new(ErrorCode::UnknownSession, "unknown session_id")
                }
                _other => {
                    tracing::warn!("session request failed");
                    RpcError::new(ErrorCode::InternalError, "request failed")
                }
            })?;

        if let Some(ref cap) = services.capacity {
            if cap.doctor == CapacityDoctorStatus::Degraded {
                tracing::warn!("starting session with degraded capacity profile — run `lokai estate capacity doctor`");
            } else if !cap.completed {
                tracing::warn!("no capacity profile — inference defaults may be suboptimal");
            }
        }

        Ok(to_value(SessionStartResult {
            session_id: result.session_id,
            data_class: result.data_class,
            resumed: result.resumed,
            messages_loaded: result.messages_loaded,
            resume_state: result.resume_state,
        }))
    }

    pub(in crate::daemon) fn session_end(&mut self, params: Value) -> Result<Value, RpcError> {
        tetonic_app::tetonic_telemetry::fault::inject_fault("during_session_shutdown");
        let p: SessionEndParams = parse(params)?;
        let services = self.services()?;
        let workspace_root = services.workspace_root.clone();
        let app = services.app.clone();
        app.sessions
            .end_session(tetonic_app::commands::EndSessionCommand {
                session_id: p.session_id.clone(),
                workspace_root,
                status: p.status.clone(),
                error: p.error.clone(),
            })
            .map_err(crate::daemon::rpc::map::map_app_error)?;
        Ok(to_value(SessionEndResult { ok: true }))
    }

    pub(in crate::daemon) async fn session_reclassify(
        &mut self,
        params: Value,
    ) -> Result<Value, RpcError> {
        let p: SessionReclassifyParams = parse(params)?;
        let services = self.services()?;
        let result = services
            .app
            .policies
            .reclassify_session(tetonic_app::commands::ReclassifySessionCommand {
                session_id: p.session_id.clone(),
                data_class: p.data_class.clone(),
                reason: p.reason,
            })
            .await
            .map_err(|e| match &e {
                tetonic_app::errors::AppError::InvalidRequest(_) => {
                    RpcError::new(ErrorCode::InvalidParams, e.employee_message())
                }
                _other => {
                    tracing::warn!("session reclassify failed");
                    RpcError::new(ErrorCode::InternalError, "request failed")
                }
            })?;
        services
            .app
            .reclassify_session_live(&p.session_id, &result.data_class);
        Ok(to_value(SessionReclassifyResult {
            ok: true,
            data_class: result.data_class,
            previous_data_class: result.previous_data_class,
        }))
    }
}

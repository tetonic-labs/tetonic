use super::prelude::*;

impl Daemon {
    pub(in crate::daemon) async fn session_cancel(
        &mut self,
        params: Value,
    ) -> Result<Value, RpcError> {
        tetonic_app::tetonic_telemetry::fault::inject_fault("during_cancellation");
        let p: SessionCancelParams = parse(params)?;
        let services = self.services()?;
        services.app.cancel_session_broker_jobs(&p.session_id);
        services
            .app
            .sessions
            .cancel_session(tetonic_app::commands::CancelRunCommand {
                session_id: p.session_id.clone(),
                pooled_cancel: services.fabric_pooled,
            })
            .await
            .map_err(|e| match &e {
                tetonic_app::errors::AppError::SessionNotFound(_) => {
                    RpcError::new(ErrorCode::UnknownSession, "unknown session_id")
                }
                _ => RpcError::new(ErrorCode::InternalError, format!("app: {e}")),
            })?;
        Ok(to_value(Canceled { canceled: true }))
    }

    pub(in crate::daemon) fn project_consolidate(
        &mut self,
        params: Value,
    ) -> Result<Value, RpcError> {
        let p: ProjectConsolidateParams = parse(params)?;
        let services = self.services()?;
        if !services.app.sessions.has_live(&p.session_id) {
            return Err(RpcError::new(
                ErrorCode::UnknownSession,
                "unknown session_id",
            ));
        }
        let result = services
            .app
            .sessions
            .consolidate_session(tetonic_app::commands::ConsolidateSessionCommand {
                session_id: p.session_id.clone(),
                workspace_root: services.workspace_root.clone(),
            })
            .map_err(crate::daemon::rpc::map::map_app_error)?;
        Ok(to_value(ProjectConsolidateResult {
            ok: true,
            digest_chars: result.digest_chars.map(|n| n as usize),
        }))
    }

    pub(in crate::daemon) fn approval_respond(&mut self, params: Value) -> Result<Value, RpcError> {
        let p: ApprovalRespondParams = parse(params)?;
        let services = self.services()?;
        let allowed = p.decision == ApprovalDecision::Allow;
        let ok = services
            .app
            .approvals
            .respond(tetonic_app::commands::ApprovalResponseCommand {
                session_id: p.session_id.clone(),
                approval_id: p.approval_id.clone(),
                approved: allowed,
                remember: p.remember,
                kind: String::new(),
                detail: String::new(),
                channel_delivered: true,
                attempt_id: None,
            })
            .map_err(|e| RpcError::new(ErrorCode::InternalError, format!("app: {e}")))?;
        Ok(to_value(Ack { ok }))
    }

    pub(in crate::daemon) fn model_list(&mut self) -> Result<Value, RpcError> {
        let services = self.services()?;
        Ok(to_value(ModelListResult {
            models: services.models.clone(),
            tool_capable: services.tool_capable,
            default_model: services.model.clone(),
            hard_model: services.model_hard.clone(),
        }))
    }
}

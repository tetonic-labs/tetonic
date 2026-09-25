use super::prelude::*;

impl Daemon {
    pub(in crate::daemon) fn chat_send(&mut self, params: Value) -> Result<Value, RpcError> {
        let p: ChatSendParams = parse(params)?;
        let services = self.services()?;
        if self.draining.load(Ordering::Relaxed) {
            return Err(RpcError::new(
                ErrorCode::InvalidRequest,
                "daemon is shutting down",
            ));
        }
        if self.capacity.busy.load(Ordering::Relaxed) {
            return Err(RpcError::new(
                ErrorCode::InvalidRequest,
                "capacity optimize in progress — interactive chat blocked",
            ));
        }
        let _ = services
            .app
            .sessions
            .live(&p.session_id)
            .map_err(|_| RpcError::new(ErrorCode::UnknownSession, "unknown session_id"))?;
        services
            .app
            .submit_chat_turn(tetonic_app::commands::RunTurnCommand {
                session_id: p.session_id,
                user_input: p.text,
                verify_cmd: None,
                llm_router: None,
            })
            .map_err(|e| RpcError::new(ErrorCode::InvalidRequest, format!("{e}")))?;
        Ok(to_value(Accepted { accepted: true }))
    }
}

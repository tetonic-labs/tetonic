use super::prelude::*;

impl Daemon {
    pub(in crate::daemon) fn services(&self) -> Result<&EngineServices, RpcError> {
        self.services
            .as_ref()
            .ok_or_else(|| RpcError::new(ErrorCode::NotReady, "call initialize first"))
    }

    pub(in crate::daemon) fn sessions_busy(&self) -> bool {
        if self.in_flight.load(Ordering::Relaxed) > 0 {
            return true;
        }
        self.services
            .as_ref()
            .is_some_and(|s| s.app.sessions.any_turn_in_flight())
    }
}

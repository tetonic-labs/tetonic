//! Product workflow futures are submitted to the shared manager.
use super::DefaultRunService;
use lokai_memory::RecoverMutex;
impl DefaultRunService {
    pub fn arm_attempt_join(
        &self,
        attempt: &lokai_domain::AttemptId,
    ) -> tokio::sync::oneshot::Receiver<crate::commands::StartIdentityJobResult> {
        self.managed.arm_attempt_join(attempt)
    }
    pub(crate) fn dispatch_chat_turn(
        &self,
        cmd: crate::commands::RunTurnCommand,
        host: crate::turn_execution::TurnExecutionHost,
        mut owned: crate::product_submit::OwnedTurn,
    ) -> Result<(), crate::errors::AppError> {
        let mut runs = self.clone();
        runs.events = owned.delivery.clone();
        let events = runs.events.clone();
        let session_id = cmd.session_id.clone();
        let ticket = self.managed.reserve_dispatch().id;
        self.session_dispatches
            .lock_recover()
            .insert(session_id, ticket.clone());
        self.managed
            .spawn_dispatch(&ticket, async move {
                let mut spawn_serial = owned
                    .live
                    .spawn_serial
                    .load(std::sync::atomic::Ordering::Relaxed);
                let result = crate::turn_execution::execute_turn(
                    &runs,
                    &events,
                    &cmd,
                    &host,
                    owned.convo.as_mut().expect("leased"),
                    &mut spawn_serial,
                )
                .await;
                crate::product_submit::complete_dispatched_turn(owned, &host, result, spawn_serial);
            })
            .map_err(Into::into)
    }

    pub(crate) fn dispatch_spawn(
        &self,
        cmd: crate::commands::SpawnAgentCommand,
        host: crate::turn_execution::TurnExecutionHost,
        mut owned: crate::product_submit::OwnedTurn,
    ) -> Result<(), crate::errors::AppError> {
        let mut runs = self.clone();
        runs.events = owned.delivery.clone();
        let events = runs.events.clone();
        let session_id = cmd.session_id.clone();
        let ticket = self.managed.reserve_dispatch().id;
        self.session_dispatches
            .lock_recover()
            .insert(session_id, ticket.clone());
        self.managed
            .spawn_dispatch(&ticket, async move {
                let mut spawn_serial = owned
                    .live
                    .spawn_serial
                    .load(std::sync::atomic::Ordering::Relaxed);
                let result = crate::turn_execution::execute_spawn(
                    &runs,
                    &events,
                    &cmd,
                    &host,
                    owned.convo.as_mut().expect("leased"),
                    &mut spawn_serial,
                )
                .await;
                crate::product_submit::complete_dispatched_turn(owned, &host, result, spawn_serial);
            })
            .map_err(Into::into)
    }
}

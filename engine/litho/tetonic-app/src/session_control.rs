//! Complete product operations for portals; foundation coordination stays here.
use crate::{
    commands::{CancelRunCommand, EndSessionCommand},
    errors::AppError,
    Application,
};

impl Application {
    pub async fn cancel_chat_turn(&self, session_id: &str) -> Result<(), AppError> {
        self.sessions.live(session_id)?;
        // Send remote cancellation while the compute job registry still has its
        // identities, before cooperative cancellation drops inference futures.
        self.cancel_session_broker_jobs(session_id);
        self.sessions
            .cancel_session(CancelRunCommand {
                session_id: session_id.into(),
                pooled_cancel: false,
            })
            .await
    }

    /// Call while the host runtime is still running. Failed draining preserves
    /// session resources so an outstanding effect cannot race worktree deletion.
    pub async fn close_session(&self, cmd: EndSessionCommand) -> Result<(), AppError> {
        let live = self.sessions.live(&cmd.session_id)?;
        live.begin_close()?;
        if live.turn_in_flight() {
            self.cancel_chat_turn(&cmd.session_id).await?;
            tokio::time::timeout(std::time::Duration::from_secs(30), async {
                while live.turn_in_flight() {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            }).await.map_err(|_| AppError::InvalidRequest(
                "session close timed out waiting for execution; session resources were preserved".into()
            ))?;
        }
        self.sessions.end_session(cmd)
    }
}

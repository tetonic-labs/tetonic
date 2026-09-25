//! Maps OS signals to product cancellation commands.
use std::sync::Arc;
use tetonic_app::Application;

pub struct CancelHandle {
    task: tokio::task::JoinHandle<()>,
}
impl CancelHandle {
    pub fn spawn(app: Arc<Application>, session_id: String) -> Self {
        let task = tokio::spawn(async move {
            while tokio::signal::ctrl_c().await.is_ok() {
                let _ = app.cancel_chat_turn(&session_id).await;
            }
        });
        Self { task }
    }
}
impl Drop for CancelHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

//! Attempt completion joins for managed identity jobs.
use super::DefaultRunService;
impl DefaultRunService {
    pub fn arm_attempt_join(
        &self,
        attempt: &tetonic_domain::AttemptId,
    ) -> tokio::sync::oneshot::Receiver<crate::commands::StartIdentityJobResult> {
        self.managed.arm_attempt_join(attempt)
    }
}

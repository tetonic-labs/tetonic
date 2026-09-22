//! Product completion adapter; lifecycle and effect ordering belong to lokai-run.
use super::*;
impl DefaultRunService {
    pub(crate) async fn finalize_attempt(
        &self,
        attempt_id: &AttemptId,
        session_id: &str,
        outcome: &CandidateOutcome,
        canceled: bool,
        policy: Option<FinalizationPolicy>,
        finish_run: bool,
    ) -> Result<CandidateOutcome, AppError> {
        let outcome = if canceled || self.managed.is_canceled(attempt_id) {
            CandidateOutcome::Canceled {
                reason: "turn execution canceled".into(),
            }
        } else {
            outcome.clone()
        };
        let result = self
            .managed
            .finalize(tetonic_run::FinalizeJob {
                attempt: attempt_id.clone(),
                outcome,
                policy,
                finish_run,
            })
            .await?;
        if finish_run {
            self.session_runs.lock_recover().remove(session_id);
            self.session_dispatches.lock_recover().remove(session_id);
            if let Some(live) = self.live.get(session_id) {
                live.clear_current_run();
            }
        }
        Ok(result)
    }
}

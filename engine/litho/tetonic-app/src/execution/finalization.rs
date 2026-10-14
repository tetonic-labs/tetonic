//! Tool effects for managed finalization; the run manager owns acceptance.
use tetonic_domain::{AttemptId, TaskId};
use tetonic_run::FinalizationEffectDriver;

pub(crate) struct ToolsFinalizationDriver(pub(crate) std::sync::Arc<tetonic_tools::Tools>);

impl FinalizationEffectDriver for ToolsFinalizationDriver {
    fn bind_effect_identity(&self, task_id: &TaskId, attempt_id: &AttemptId) -> Result<(), String> {
        self.0
            .bind_effect_identity(task_id.clone(), attempt_id.clone())
            .map_err(|e| e.to_string())
    }

    fn run_verify(
        &self,
        verify_cmd: &str,
        _cancel: &tetonic_domain::work_scope::CancellationSignal,
    ) -> Result<(), (String, Option<String>)> {
        let (ok, output) = self.0.run_command_cancellable(verify_cmd, Some(_cancel));
        if ok {
            Ok(())
        } else {
            let hint = tetonic_tools::summarize_verify_failure(&output);
            Err((output, hint))
        }
    }

    fn commit_workspace(&self) -> Result<Option<tetonic_domain::CommitResult>, String> {
        self.0.commit_staged_if_any().map_err(|e| e.to_string())
    }

    fn commit_workspace_cancellable(
        &self,
        cancel: &tetonic_domain::work_scope::CancellationSignal,
    ) -> Result<Option<tetonic_domain::CommitResult>, String> {
        let result = self
            .0
            .commit_staged_cancellable(cancel)
            .map_err(|e| e.to_string());
        if result.is_err() {
            // A failed finalization must not lose the draft produced by a
            // completed loop. Recovery-required transactions remain protected.
            if let Err(error) = self.0.abort_preserving_draft() {
                tracing::warn!("could not retain stopped finalization draft: {error}");
            }
        }
        result
    }
}

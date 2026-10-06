//! Opt-in fixture for races between two live finalizers. Only run creation is
//! configured here; admission, leases, claims, persistence and replay still use
//! the real supervisor. Ordinary tests retain the production one-attempt limit.
use std::sync::Arc;

use tetonic_domain::{
    ReplayGap, RunCommand, RunCommandResult, RunEventEnvelope, RunId, RunSnapshot,
    RunSupervisorError, SpeculationConfig,
};
use tetonic_run::RunSupervisor;

pub fn allow_two_attempts(inner: Arc<dyn RunSupervisor>) -> Arc<dyn RunSupervisor> {
    Arc::new(CompetingAttempts(inner))
}

struct CompetingAttempts(Arc<dyn RunSupervisor>);

#[async_trait::async_trait]
impl RunSupervisor for CompetingAttempts {
    async fn handle(
        &self,
        mut command: RunCommand,
    ) -> Result<RunCommandResult, RunSupervisorError> {
        if let RunCommand::CreateRun(create) = &mut command {
            create.speculation = Some(SpeculationConfig {
                allowed: true,
                max_simultaneous_attempts: 2,
                require_result_agreement: false,
            });
        }
        self.0.handle(command).await
    }

    async fn snapshot(&self, run_id: RunId) -> Result<RunSnapshot, RunSupervisorError> {
        self.0.snapshot(run_id).await
    }

    async fn resume_from_sequence(
        &self,
        run_id: RunId,
        after_sequence: u64,
        limit: u32,
    ) -> Result<Result<Vec<RunEventEnvelope>, ReplayGap>, RunSupervisorError> {
        self.0
            .resume_from_sequence(run_id, after_sequence, limit)
            .await
    }

    fn recovery_required(&self) -> bool {
        self.0.recovery_required()
    }

    fn recovery_required_reason(&self) -> Option<String> {
        self.0.recovery_required_reason()
    }

    fn enter_safe_mode(&self, reason: &str) {
        self.0.enter_safe_mode(reason);
    }

    fn is_safe_mode(&self) -> bool {
        self.0.is_safe_mode()
    }
}

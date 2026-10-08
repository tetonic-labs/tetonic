//! Local product adapter: agent composition and readable output, not scheduling.
use super::*;
use crate::team_work_controller::{ProgressReader, TeamWorkHost};

#[async_trait::async_trait(?Send)]
impl TeamWorkHost for LocalWorkspace {
    async fn admit(
        &self,
        receipt: &HuddleExecution,
        parent: &tetonic_run::managed::DelegationParent,
        key: &str,
    ) -> Result<(), AppError> {
        self.admit_plan_assignment(receipt, parent, key).await
    }

    async fn contribution(
        &self,
        receipt: &HuddleExecution,
        key: &str,
    ) -> Result<tetonic_domain::ToolOutcome, AppError> {
        let pin = receipt
            .assignments
            .iter()
            .find(|p| p.assignment_key == key)
            .ok_or(AppError::InferenceUnavailable)?;
        let task = self.task(&pin.work_id).await?;
        self.directed_contribution(receipt, key, &task).await
    }
}

impl LocalWorkspace {
    pub(super) fn controller_reader(&self) -> ProgressReader {
        ProgressReader {
            store: self.keys.store.clone(),
            actor: OWNER.into(),
            org: ORG.into(),
            team: TEAM.into(),
        }
    }

    // Existing product acceptance exercises the same engine delivery reconciler.
    #[cfg(test)]
    pub(super) async fn collect_plan_contributions(
        &self,
        receipt: &HuddleExecution,
        requested: &str,
        outcome: tetonic_domain::ToolOutcome,
        remaining: &Arc<Mutex<HashSet<String>>>,
    ) -> Result<tetonic_domain::ToolOutcome, AppError> {
        crate::team_work_controller::collect_contributions(
            &self.controller_reader(),
            self,
            receipt,
            requested,
            outcome,
            remaining,
        )
        .await
    }
}

//! Allocation-only correction of the durable first proposal; never a new plan.
use super::*;
use crate::resources::work_director::Allocation;

impl WorkService {
    pub(super) async fn director_rebalance(
        &self,
        binding: &DirectorBinding,
        call: &str,
        allocations: Vec<Allocation>,
    ) -> Result<serde_json::Value, AppError> {
        let scope = self.services.authorized_scope().await?;
        let (direction, mut content) = self.services.local.resources()
            .guide_proposal_allocation_basis(&self.services.host.credential,
                scope.organization().into(), scope.team().into(), binding.source.clone(), binding.turn.clone())
            .await.map_err(resource)?
            .ok_or_else(|| AppError::InvalidRequest("No parsed proposal to rebalance in this reply. Repair structural errors with propose.".into()))?;
        let keys: std::collections::HashSet<_> =
            allocations.iter().map(|a| &a.assignment_key).collect();
        let valid = allocations.len() == content.assignments.len()
            && keys.len() == allocations.len()
            && content.assignments.iter().all(|a| keys.contains(&a.key));
        if !valid {
            return self.record_proposal(binding, call, direction, Some(content), None,
                Some("Provide exactly one token allocation for each original assignment key; no added or missing assignments.".into())).await;
        }
        for assignment in &mut content.assignments {
            assignment.token_budget = allocations
                .iter()
                .find(|a| a.assignment_key == assignment.key)
                .unwrap()
                .token_budget;
        }
        self.director_propose_call(binding, call, direction, content)
            .await
    }
}

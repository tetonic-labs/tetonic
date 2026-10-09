//! Proposal and launch checks share the same current allowance policy. These
//! checks reserve nothing; admission still rechecks the reviewed plan.
use super::*;
use tetonic_memory::PlanContent;

impl WorkService {
    pub(super) async fn plan_token_limit(&self) -> Result<Option<i64>, AppError> {
        let scope = self.services.authorized_scope().await?;
        Ok(self
            .services
            .local
            .resources()
            .team_budget_setting(
                &self.services.host.credential,
                scope.organization().into(),
                scope.team().into(),
            )
            .await
            .map_err(resource)?
            .token_limit)
    }

    pub(super) async fn plan_allowance_issues(
        &self,
        content: &PlanContent,
    ) -> Result<Vec<String>, AppError> {
        content
            .validate()
            .map_err(|e| AppError::InvalidRequest(e.to_string()))?;
        let mut issues = vec![];
        let workers: u64 = content.assignments.iter().map(|a| a.token_budget).sum();
        let coordination = content.token_budget - workers;
        let ceiling = self.services.execution.coordination_tokens();
        if !(256..=ceiling).contains(&coordination) {
            issues.push(format!("This proposal leaves {coordination} tokens for coordination and the combined result; it needs 256–{ceiling} within the total of {}. Rebalance the allocations without increasing the total or reducing the requested work.", content.token_budget));
        }
        if let Some(limit) = self.plan_token_limit().await? {
            if content.token_budget > limit as u64 {
                issues.push(format!("The proposed total of {} tokens exceeds the workspace allowance of {limit}. Propose work within that allowance, or explain why the owner needs to review the scope or limit before proceeding.", content.token_budget));
            }
        }
        let agents = self.services.agents().await?;
        for assignment in &content.assignments {
            if let Some(agent) = agents.iter().find(|a| a.key == assignment.agent_key) {
                if assignment.token_budget > agent.max_tokens {
                    issues.push(format!("{} allocates {} tokens to {}, whose per-run allowance is {}. Rebalance within that allowance; do not change the contributor or deliverable to hide the mismatch.", assignment.title, assignment.token_budget, agent.name, agent.max_tokens));
                }
            }
        }
        Ok(issues)
    }

    pub(super) async fn validate_plan_proposal(
        &self,
        source: &str,
        content: &PlanContent,
    ) -> Result<(), AppError> {
        let issues = self.plan_allowance_issues(content).await?;
        let agents = self.planning_agents(source).await?;
        for assignment in &content.assignments {
            if !agents.iter().any(|a| a.key == assignment.agent_key) {
                return Err(AppError::InvalidRequest(format!(
                    "{} is not in this discussion's available working roster. Keep the saved team and choose one of its working agents.",
                    assignment.agent_key
                )));
            }
        }
        if !issues.is_empty() {
            return Err(AppError::InvalidRequest(issues.join(" ")));
        }
        Ok(())
    }
}

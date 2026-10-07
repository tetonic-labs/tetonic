//! Model selection uses registered revisions; dispatch keeps its existing owner.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoordinationModel {
    pub provider: String,
    pub model: String,
}

impl From<&LocalAgent> for CoordinationModel {
    fn from(agent: &LocalAgent) -> Self {
        Self {
            provider: agent.provider.clone(),
            model: agent.model.clone(),
        }
    }
}

#[derive(Serialize)]
pub struct PlanSetupIssue {
    pub agent_key: String,
    pub message: String,
}

pub(super) fn check_model_choice(
    request: &StartPlan,
    model: &CoordinationModel,
) -> Result<(), AppError> {
    if request
        .coordinator
        .as_ref()
        .is_some_and(|expected| expected != model)
    {
        return Err(AppError::InvalidRequest("The coordination model changed. Review the current model before starting; no new team was launched.".into()));
    }
    if model.provider != "ollama"
        && (request.coordinator.is_none() || !request.hosted_coordination_consent)
    {
        return Err(AppError::InvalidRequest("Allow the shared brief, plan, team contributions and plan clarifications to be sent to the displayed coordination provider before starting.".into()));
    }
    if model.provider == "ollama" && request.hosted_coordination_consent {
        return Err(AppError::InvalidRequest(
            "The selected coordinator is local. Refresh this plan's model choice.".into(),
        ));
    }
    Ok(())
}

pub(super) fn configuration(
    guide: &LocalAgent,
    content: &PlanContent,
    seconds: u64,
) -> serde_json::Value {
    let own = content.token_budget
        - content
            .assignments
            .iter()
            .map(|a| a.token_budget)
            .sum::<u64>();
    serde_json::json!({
        "instructions": INSTRUCTIONS, "requested_tools":["finish",DISPATCH],
        "explain_turn":false, "max_steps":16,
        "preferences":crate::resources::GeneralAgentPreferences {
            provider: (guide.provider != "ollama").then(|| guide.provider.clone()),
            model: guide.model.clone(), display_name: COORDINATOR.into(),
            hosted_consent: guide.provider != "ollama", tool_disclosure: None, hosted_workspace: None,
            max_elapsed_seconds: seconds, reported_token_ceiling: own,
        }
    })
}

impl LocalWorkspace {
    pub(in crate::local_workspace) async fn guide_for_coordination(
        &self,
    ) -> Result<LocalAgent, AppError> {
        let stored = self.registered_agent(shaping::GUIDE).await?;
        let guide = self.agent_profile(shaping::GUIDE.into(), &stored)?;
        if !guide.tools.is_empty() {
            return Err(AppError::PolicyDenied(
                "The Guide's model must have the managed planning configuration.".into(),
            ));
        }
        Ok(guide)
    }

    async fn coordinator_revision(
        &self,
        receipt: &HuddleExecution,
    ) -> Result<tetonic_memory::RegisteredAgent, AppError> {
        self.local
            .resources()
            .get_agent_revision(
                &self.host.credential,
                ORG.into(),
                COORDINATOR.into(),
                receipt.coordinator_digest.clone(),
            )
            .await
            .map_err(resource)?
            .ok_or(AppError::InferenceUnavailable)
    }

    pub(super) async fn pinned_coordinator_model(
        &self,
        receipt: &HuddleExecution,
    ) -> Result<Option<CoordinationModel>, AppError> {
        let stored = self.coordinator_revision(receipt).await?;
        let value: serde_json::Value = serde_json::from_str(&stored.definition_json)
            .map_err(|_| AppError::InferenceUnavailable)?;
        // Historical receipts did not pin inference preferences. Do not report
        // today's host default as the model those old runs actually used.
        if value["configuration"]["preferences"].is_null() {
            return Ok(None);
        }
        Ok(Some(CoordinationModel::from(
            &self.agent_profile(COORDINATOR.into(), &stored)?,
        )))
    }

    pub(super) async fn pinned_coordinator(
        &self,
        receipt: &HuddleExecution,
    ) -> Result<LocalAgent, AppError> {
        let stored = self.coordinator_revision(receipt).await?;
        let mut agent = self.agent_profile(COORDINATOR.into(), &stored)?;
        if agent.tools.iter().any(|t| t != "finish" && t != DISPATCH) {
            return Err(AppError::PolicyDenied(
                "Unexpected coordinator tools.".into(),
            ));
        }
        // Dispatch is a host-bound internal control, not external tool access.
        // Its advertisements/authority are installed separately at admission.
        agent.tools.clear();
        Ok(agent)
    }

    pub(in crate::local_workspace) async fn plan_setup_issues(
        &self,
        content: &PlanContent,
    ) -> Result<Vec<PlanSetupIssue>, AppError> {
        let mut profiles = vec![self.guide_for_coordination().await?];
        let mut seen = HashSet::new();
        for assignment in &content.assignments {
            if !seen.insert(&assignment.agent_key) {
                continue;
            }
            if let Ok(stored) = self.registered_agent(&assignment.agent_key).await {
                profiles.push(self.agent_profile(assignment.agent_key.clone(), &stored)?);
            }
        }
        // One discovery per readiness read, not one request per local teammate.
        let models = if profiles.iter().any(|a| a.provider == "ollama") {
            Some(self.installed_models().await)
        } else {
            None
        };
        let mut issues = vec![];
        for agent in profiles {
            let result = match (&models, agent.provider.as_str()) {
                (Some(Err(error)), "ollama") => {
                    Err(AppError::InvalidRequest(error.employee_message()))
                }
                _ => self
                    .agent_execution_settings_with_models(
                        &agent,
                        models
                            .as_ref()
                            .and_then(|m| m.as_ref().ok())
                            .map(Vec::as_slice),
                    )
                    .await
                    .map(|_| ()),
            };
            if let Err(error) = result {
                let name = if agent.key == shaping::GUIDE {
                    "Coordination (Guide model)"
                } else {
                    &agent.name
                };
                issues.push(PlanSetupIssue {
                    agent_key: agent.key.clone(),
                    message: format!("{name}: {}", error.employee_message()),
                });
            }
        }
        Ok(issues)
    }
}

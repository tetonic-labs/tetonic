use super::*;
#[cfg(test)]
#[path = "editing_tests.rs"]
mod tests;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateLocalAgent {
    pub agent_key: String,
    pub expected_definition_digest: String,
    pub configuration: CreateLocalAgent,
}

impl LocalWorkspace {
    pub async fn update_agent(&self, input: UpdateLocalAgent) -> Result<LocalAgent, AppError> {
        let config = self.agent_configuration(input.configuration.clone())?;
        let _admission = self.admission.lock().await;
        let current = self.registered_agent(&input.agent_key).await?;
        let agent = self.agent_profile(input.agent_key.clone(), &current)?;
        if !agent.editable {
            return Err(AppError::InvalidRequest("This agent is managed by the engine's planning system. Create a teammate for custom settings and tools.".into()));
        }
        if agent.provider != input.configuration.provider
            || agent.model != input.configuration.model
        {
            if input.configuration.provider == "ollama" {
                self.require_installed_model(&input.configuration.model)
                    .await?;
            } else if !self.keys.ready(&input.configuration.provider).await {
                return Err(AppError::InvalidRequest(
                    "Save this provider's API key before changing the agent's model.".into(),
                ));
            }
        }
        let stored = self.local.resources().edit_agent(&self.host.credential, crate::resources::EditAgent {
            org: ORG.into(), key: input.agent_key.clone(),
            request: input.configuration.request_id,
            expected: input.expected_definition_digest,
            harness: input.configuration.harness,
            configuration: config,
        }).await.map_err(|error| match error {
            crate::resources::ResourceError::Conflict => AppError::InvalidRequest("This agent changed since you opened it, or this save belongs to a different edit. Reopen the agent before saving again.".into()),
            other => resource(other),
        })?;
        self.agent_profile(input.agent_key, &stored)
    }
}

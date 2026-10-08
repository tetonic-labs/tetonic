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

impl WorkspaceServices {
    pub async fn update_agent(&self, input: UpdateLocalAgent) -> Result<LocalAgent, AppError> {
        let app_scope = self.authorized_scope().await?;
        let config = self.agent_configuration(input.configuration.clone())?;
        let _admission = self.admission.lock().await;
        let current = self.registered_agent(&input.agent_key).await?;
        let agent = self.agent_profile(input.agent_key.clone(), &current)?;
        if !agent.editable {
            return Err(AppError::InvalidRequest("This agent is managed by the engine's planning system. Create a teammate for custom settings and tools.".into()));
        }
        // The Guide shares saved-agent identity, revisions and provider settings.
        // Its planning authority still comes only from a scoped host binding;
        // editing a model must not turn it into an arbitrary workspace executor.
        if input.agent_key == GUIDE
            && (input.configuration.name != agent.name
                || input.configuration.purpose != agent.purpose
                || input.configuration.harness != "general"
                || input
                    .configuration
                    .tools
                    .as_ref()
                    .is_some_and(|t| !t.is_empty())
                || input.configuration.hosted_tools_consent
                || input.configuration.expected_workspace_root.is_some())
        {
            return Err(AppError::InvalidRequest(
                "Choose the Guide's model and reply limits. Its identity, planning instructions and scoped tools are managed by the engine.".into(),
            ));
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
            org: app_scope.organization().into(), key: input.agent_key.clone(),
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

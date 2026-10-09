//! One resolver for a saved agent, whether assigned directly or to team work.
use super::*;
impl WorkspaceServices {
    pub(crate) async fn agent_execution_settings(
        &self,
        agent: &LocalAgent,
    ) -> Result<crate::resources::RegisteredExecutionSettings, AppError> {
        self.agent_execution_settings_with_models(agent, None).await
    }

    pub(crate) async fn agent_execution_settings_with_models(
        &self,
        agent: &LocalAgent,
        local_models: Option<&[String]>,
    ) -> Result<crate::resources::RegisteredExecutionSettings, AppError> {
        self.check_limits(agent.max_steps, agent.max_seconds, agent.max_tokens)?;
        let mut settings = self.host.settings.clone();
        settings.workspace_root = self.resolve_folder(agent.workspace_root.as_deref())?;
        settings
            .allowed_tools
            .retain(|tool| agent.tools.contains(tool));
        if let Some(mcp) = &settings.mcp {
            settings.allowed_tools.extend(
                mcp.tool_names()
                    .into_iter()
                    .filter(|tool| agent.tools.contains(tool)),
            );
        }
        if let Some(skills) = &settings.skills {
            settings.allowed_tools.extend(
                skills
                    .tool_names()
                    .into_iter()
                    .filter(|id| agent.tools.contains(id)),
            );
        }
        if agent
            .tools
            .iter()
            .any(|tool| tool != "finish" && !settings.allowed_tools.contains(tool))
        {
            return Err(AppError::InvalidRequest("A saved agent tool is unavailable on this host. Restore its connection or edit the agent; its configuration was not substituted.".into()));
        }
        self.apply_agent_inference(agent, &mut settings, local_models)
            .await?;
        if !crate::resources::uses_workspace(&agent.tools) {
            settings.workspace_root = None;
        }
        settings.max_elapsed_seconds = agent.max_seconds;
        settings.reported_token_ceiling = Some(agent.max_tokens);
        Ok(settings)
    }

    /// Shared model/egress resolver for workers and the host-bound coordinator.
    /// Callers enforce their distinct execution and budget ceilings beforehand.
    pub(crate) async fn apply_agent_inference(
        &self,
        agent: &LocalAgent,
        settings: &mut crate::resources::RegisteredExecutionSettings,
        local_models: Option<&[String]>,
    ) -> Result<(), AppError> {
        if agent.provider == "ollama" {
            if let Some(models) = local_models {
                if !models
                    .iter()
                    .any(|model| tetonic_inference::ollama_model_matches(model, &agent.model))
                {
                    return Err(AppError::InvalidRequest(format!("Model {} is unavailable. Choose an installed local model with tool calling support.", agent.model)));
                }
            } else {
                self.require_installed_model(&agent.model).await?;
            }
            settings.hosted = None;
        } else {
            settings.hosted = Some(self.hosted_binding(agent).await?);
            if agent.hosted_workspace.is_none() {
                settings.workspace_root = None;
            }
            settings.data_class = DataClass::SensitiveSource;
        }
        settings.model = agent.model.clone();
        Ok(())
    }
}

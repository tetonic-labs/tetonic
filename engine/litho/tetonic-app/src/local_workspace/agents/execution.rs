//! One resolver for a saved agent, whether assigned directly or to team work.
use super::*;
impl LocalWorkspace {
    pub(in crate::local_workspace) async fn agent_execution_settings(
        &self,
        agent: &LocalAgent,
    ) -> Result<crate::resources::RegisteredExecutionSettings, AppError> {
        self.check_limits(agent.max_steps, agent.max_seconds, agent.max_tokens)?;
        let mut settings = self.host.settings.clone();
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
        if agent
            .tools
            .iter()
            .any(|tool| tool != "finish" && !settings.allowed_tools.contains(tool))
        {
            return Err(AppError::InvalidRequest("A saved agent tool is unavailable on this host. Restore its connection or edit the agent; its configuration was not substituted.".into()));
        }
        if agent.provider == "ollama" {
            self.require_installed_model(&agent.model).await?;
            settings.hosted = None;
        } else {
            settings.hosted = Some(self.hosted_binding(agent).await?);
            if agent.hosted_workspace.is_none() {
                settings.workspace_root = None;
            }
            settings.data_class = DataClass::SensitiveSource;
        }
        if !crate::resources::uses_workspace(&agent.tools) {
            settings.workspace_root = None;
        }
        settings.model = agent.model.clone();
        settings.max_elapsed_seconds = agent.max_seconds;
        settings.reported_token_ceiling = Some(agent.max_tokens);
        Ok(settings)
    }
}

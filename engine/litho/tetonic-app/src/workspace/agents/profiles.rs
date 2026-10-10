//! Compatibility metadata shared by creation and the connected editor.
//! This is not credential validation or execution authority.
use super::*;

#[derive(Serialize)]
pub struct LocalAgentRuntimeProfile {
    pub requires_tool_consent: bool,
    pub provider: String,
    pub harness: String,
    pub tools: Vec<String>,
    pub tool_restriction: Option<String>,
}

impl WorkspaceServices {
    pub(crate) fn agent_runtime_profiles(&self) -> Vec<LocalAgentRuntimeProfile> {
        let mut tools: Vec<_> = self
            .host
            .settings
            .allowed_tools
            .iter()
            .filter(|tool| tool.as_str() != "finish")
            .cloned()
            .collect();
        tools.sort();
        tools.retain(|tool| {
            tool == "blackboard" || crate::resources::WORKSPACE_TOOLS.contains(&tool.as_str())
        });
        tools.extend(
            self.host
                .settings
                .mcp
                .as_ref()
                .map(|m| m.tool_names())
                .unwrap_or_default(),
        );
        if let Some(skills) = &self.host.settings.skills {
            tools.extend(skills.tool_names());
        }
        ["ollama", "openai", "anthropic", "google"]
            .into_iter()
            .map(|provider| LocalAgentRuntimeProfile {
                requires_tool_consent: provider != "ollama",
                provider: provider.into(),
                harness: "general".into(),
                tools: tools.clone(),
                tool_restriction: None,
            })
            .collect()
    }
}

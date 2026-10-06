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

impl LocalWorkspace {
    pub(super) fn agent_runtime_profiles(&self) -> Vec<LocalAgentRuntimeProfile> {
        let mut tools: Vec<_> = self
            .host
            .settings
            .allowed_tools
            .iter()
            .filter(|tool| tool.as_str() != "finish")
            .cloned()
            .collect();
        tools.sort();
        ["ollama", "openai", "anthropic"]
            .into_iter()
            .map(|provider| LocalAgentRuntimeProfile {
                requires_tool_consent: provider != "ollama",
                provider: provider.into(),
                harness: "general".into(),
                tools: match provider {
                    "ollama" => tools.iter().cloned().chain(self.host.settings.mcp.as_ref().map(|m| m.tool_names()).unwrap_or_default()).collect(),
                    "openai" => tools.iter().filter(|tool| crate::resources::HOSTED_READ_TOOLS.contains(&tool.as_str())).cloned().collect(),
                    _ => vec![],
                },
                tool_restriction: match provider {
                    "openai" => Some("This profile supports selected file reads. Writes and other tools are not enabled yet. Choose another provider or remove those tools.".into()),
                    "anthropic" => Some("Workspace tools are not supported for this provider yet. Choose another provider or remove the selected tools.".into()),
                    _ => None,
                },
            })
            .collect()
    }
}

//! Compatibility metadata shared by creation and the connected editor.
//! This is not credential validation or execution authority.
use super::*;

#[derive(Serialize)]
pub struct LocalAgentRuntimeProfile {
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
                provider: provider.into(),
                harness: "general".into(),
                tools: if provider == "ollama" { tools.clone() } else { vec![] },
                tool_restriction: (provider != "ollama").then(|| {
                    "Workspace tools are not supported for hosted models on this host. Choose a local model or remove the selected tools.".into()
                }),
            })
            .collect()
    }
}

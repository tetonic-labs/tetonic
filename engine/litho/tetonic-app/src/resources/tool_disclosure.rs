//! Versioned data-disclosure consent, separate from execution grants.
//! The host constructs and rechecks this binding; it never grants a tool itself.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ToolDisclosure {
    pub version: u32,
    pub provider: String,
    pub endpoint: String,
    pub tools: Vec<String>,
    pub workspace: Option<String>,
}

impl ToolDisclosure {
    pub fn matches(
        &self,
        provider: &str,
        endpoint: &str,
        tools: &[String],
        workspace: Option<&str>,
    ) -> bool {
        let mut selected = tools.to_vec();
        selected.sort();
        selected.dedup();
        self.version == 1
            && self.provider == provider
            && self.endpoint == endpoint
            && self.tools == selected
            && self.workspace.as_deref() == workspace
    }
}

/// These capabilities use the jailed workspace and existing effect/finalization
/// brokers. Model choice does not alter their implementation or grants.
pub const WORKSPACE_TOOLS: &[&str] = &[
    "read_file",
    "list_dir",
    "grep",
    "glob",
    "outline",
    "search_code",
    "edit_file",
    "write_file",
];

pub fn uses_workspace(tools: &[String]) -> bool {
    tools
        .iter()
        .any(|tool| WORKSPACE_TOOLS.contains(&tool.as_str()))
}

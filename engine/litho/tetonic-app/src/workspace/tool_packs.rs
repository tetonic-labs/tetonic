//! Reusable selections resolved before the existing saved-agent validation path.
use super::*;
pub use tetonic_memory::{SaveToolPack, ToolPack};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPackRef {
    pub id: String,
    pub revision: i64,
}

#[derive(Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPackSelection {
    pub packs: Vec<ToolPackRef>,
    #[serde(default)]
    pub excluded_tools: Vec<String>,
}

fn builtins(org: &str, team: &str) -> Vec<ToolPack> {
    // Revision 1 stays immutable. Introduce a new revision for future membership changes.
    [
        (
            "builtin:read-files",
            "Read files",
            "Read and search inside an approved working folder.",
            vec!["read_file", "list_dir", "grep", "glob"],
        ),
        (
            "builtin:edit-files",
            "Edit files",
            "Read, search, create and edit files in an approved working folder.",
            vec![
                "read_file",
                "list_dir",
                "grep",
                "glob",
                "write_file",
                "edit_file",
            ],
        ),
        (
            "builtin:terminal",
            "Terminal",
            "Run commands through the existing governed terminal tool.",
            vec!["run_shell"],
        ),
    ]
    .into_iter()
    .map(|(id, name, description, tools)| ToolPack {
        org_id: org.into(),
        team_id: team.into(),
        id: id.into(),
        revision: 1,
        name: name.into(),
        description: description.into(),
        tools: tools.into_iter().map(String::from).collect(),
        builtin: true,
    })
    .collect()
}

impl WorkspaceServices {
    pub async fn tool_packs(&self) -> Result<Vec<ToolPack>, AppError> {
        let scope = self.authorized_scope().await?;
        let mut packs = builtins(scope.organization(), scope.team());
        packs.extend(
            self.local
                .resources()
                .tool_packs(
                    &self.host.credential,
                    scope.organization().into(),
                    scope.team().into(),
                )
                .await
                .map_err(resource)?,
        );
        Ok(packs)
    }

    pub async fn save_tool_pack(&self, input: SaveToolPack) -> Result<ToolPack, AppError> {
        let scope = self.authorized_scope().await?;
        let available: std::collections::HashSet<_> = self
            .agent_runtime_profiles()
            .into_iter()
            .flat_map(|p| p.tools)
            .collect();
        if input.tools.iter().any(|tool| !available.contains(tool)) {
            return Err(AppError::InvalidRequest("Choose tools currently available in this workspace. Reconnect missing services or remove unavailable members before saving.".into()));
        }
        self.local
            .resources()
            .save_tool_pack(
                &self.host.credential,
                scope.organization().into(),
                scope.team().into(),
                input,
            )
            .await
            .map_err(|e| match e {
                crate::resources::ResourceError::Conflict => {
                    AppError::Conflict("This pack changed. Reopen it before saving again.".into())
                }
                other => resource(other),
            })
    }

    pub(crate) async fn resolved_agent_configuration(
        &self,
        mut input: CreateLocalAgent,
    ) -> Result<serde_json::Value, AppError> {
        let scope = self.authorized_scope().await?;
        let selection = input.tool_pack_selection.take().unwrap_or_default();
        if selection.packs.is_empty() && selection.excluded_tools.is_empty() {
            return self.agent_configuration(input);
        }
        if selection.packs.len() > 32 || selection.excluded_tools.len() > 128 {
            return Err(AppError::InvalidRequest(
                "Select at most 32 packs and 128 exclusions.".into(),
            ));
        }
        let defaults = builtins(scope.organization(), scope.team());
        let mut packs = Vec::new();
        let mut ids = std::collections::HashSet::new();
        for reference in selection.packs {
            if !ids.insert(reference.id.clone()) {
                return Err(AppError::InvalidRequest(
                    "A pack can only be selected once.".into(),
                ));
            }
            let pack = if reference.id.starts_with("builtin:") {
                defaults.iter().find(|p| p.id == reference.id && p.revision == reference.revision).cloned()
            } else {
                self.local.resources().tool_pack_revision(&self.host.credential, scope.organization().into(), scope.team().into(), reference.id, reference.revision).await.map_err(resource)?
            }.ok_or_else(|| AppError::InvalidRequest("A selected pack revision is unavailable in this workspace. Refresh the library.".into()))?;
            packs.push(pack);
        }
        packs.sort_by(|a, b| a.id.cmp(&b.id));
        let members: std::collections::HashSet<_> =
            packs.iter().flat_map(|p| p.tools.iter().cloned()).collect();
        if selection
            .excluded_tools
            .iter()
            .any(|t| !members.contains(t))
        {
            return Err(AppError::InvalidRequest(
                "Only pack members can be excluded.".into(),
            ));
        }
        let mut tools = input.tools.take().unwrap_or_default();
        tools.extend(members);
        tools.retain(|t| !selection.excluded_tools.contains(t));
        tools.sort();
        tools.dedup();
        input.tools = Some(tools);
        let mut configuration = self.agent_configuration(input)?;
        if !packs.is_empty() {
            configuration["tool_packs"] = serde_json::json!(packs);
        }
        Ok(configuration)
    }
}

#[cfg(test)]
mod tests;

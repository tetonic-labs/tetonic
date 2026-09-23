use std::sync::Arc;

use tetonic_orchestrator::{DomainPack, PackManifest, RoleId, SpecialistPack};

use crate::definition::CodingAgentDefinition;

static CODING_MANIFEST: std::sync::LazyLock<PackManifest> = std::sync::LazyLock::new(|| {
    PackManifest::new(
        "coding",
        "Coding Assistant",
        "0.1.0",
        "Software engineering and repository workbench",
        vec![
            "read_file".into(),
            "edit_file".into(),
            "run_shell".into(),
            "spawn_agent".into(),
            "finish".into(),
        ],
    )
});

/// Production coding pack. Delegates to [`CodingAgentDefinition::production()`].
#[derive(Debug, Default, Clone, Copy)]
pub struct CodingPack;

impl CodingPack {
    pub fn arc() -> Arc<dyn SpecialistPack> {
        Arc::new(CodingPack)
    }

    pub fn apply_tool_filter(role: &RoleId, tools: tetonic_tools::Tools) -> tetonic_tools::Tools {
        match CodingAgentDefinition::production().allowed_tools(role) {
            None => tools,
            Some(names) => {
                let set: std::collections::HashSet<String> = names.into_iter().collect();
                tools.with_allowed_tools(set)
            }
        }
    }

    pub fn apply_spawn_tool_filter(
        role: &RoleId,
        tools: tetonic_tools::Tools,
    ) -> tetonic_tools::Tools {
        let names = CodingAgentDefinition::production().spawn_allowed_tools(role);
        let set: std::collections::HashSet<String> = names.into_iter().collect();
        tools.with_allowed_tools(set)
    }
}

impl DomainPack for CodingPack {
    fn manifest(&self) -> &PackManifest {
        &CODING_MANIFEST
    }

    fn specialist_pack(&self) -> Arc<dyn SpecialistPack> {
        Arc::new(CodingPack)
    }

    fn default_verification_command(
        &self,
        workspace: &std::path::Path,
        explicit_override: Option<&str>,
    ) -> Option<String> {
        tetonic_tools::resolve_verify_cmd(explicit_override, workspace)
    }
}

impl SpecialistPack for CodingPack {
    fn parse(&self, s: &str) -> Option<RoleId> {
        CodingAgentDefinition::production().parse(s)
    }

    fn default_role(&self) -> RoleId {
        CodingAgentDefinition::production().default_role()
    }

    fn critic_role(&self) -> RoleId {
        CodingAgentDefinition::production().critic_role()
    }

    fn revision_role(&self) -> RoleId {
        CodingAgentDefinition::production().revision_role()
    }

    fn overlay(&self, role: &RoleId) -> String {
        CodingAgentDefinition::production().overlay(role)
    }

    fn allowed_tools(&self, role: &RoleId) -> Option<Vec<String>> {
        CodingAgentDefinition::production().allowed_tools(role)
    }

    fn spawn_allowed_tools(&self, role: &RoleId) -> Vec<String> {
        CodingAgentDefinition::production().spawn_allowed_tools(role)
    }

    fn should_run_critic(&self, role: &RoleId) -> bool {
        CodingAgentDefinition::production().should_run_critic(role)
    }

    fn max_steps(&self, role: &RoleId, base: usize) -> usize {
        CodingAgentDefinition::production().max_steps(role, base)
    }

    fn explain_turn(&self, role: &RoleId, base: bool) -> bool {
        CodingAgentDefinition::production().explain_turn(role, base)
    }

    fn root_explain_turn(&self, user_text: &str) -> bool {
        CodingAgentDefinition::production().root_explain_turn(user_text)
    }
}

/// Production session host with coding index opener injected (M9 S-4).
pub fn product_session_host(
    workspace_root: impl Into<std::path::PathBuf>,
    policy: Arc<tetonic_policy::PolicyEngine>,
) -> tetonic_orchestrator::SessionHost {
    tetonic_orchestrator::SessionHost::new(workspace_root, policy)
        .with_code_index(Arc::new(tetonic_index::FilesystemCodeIndex))
        .with_verify_resolver(Arc::new(|ws, explicit| {
            tetonic_tools::resolve_verify_cmd(explicit, ws)
        }))
}

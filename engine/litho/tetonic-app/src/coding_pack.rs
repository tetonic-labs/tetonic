//! Coding specialist pack façade (CODE-01). Production table is `CodingAgentDefinition`.

use std::sync::Arc;

use tetonic_orchestrator::{RoleId, SpecialistPack};

use crate::definition::CodingAgentDefinition;

/// Production coding pack. Delegates to [`CodingAgentDefinition::production()`].
#[derive(Debug, Default, Clone, Copy)]
pub struct CodingPack;

impl CodingPack {
    pub fn arc() -> Arc<dyn SpecialistPack> {
        Arc::new(CodingPack)
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
}

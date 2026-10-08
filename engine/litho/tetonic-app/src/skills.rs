//! Workspace-owned skill instructions, loaded through the shared tool host.
//! Skill text conveys no permission to execute files or acquire other tools.
use std::{collections::HashSet, sync::Arc};
use tetonic_memory::{SharedStore, WorkspaceSkill};

mod host;
mod manifest;
pub(crate) use host::{SkillAuthority, SkillToolHost};
pub(crate) use manifest::parse_skill;

#[derive(Clone)]
pub struct SkillLibrary {
    pub(crate) store: SharedStore,
    pub(crate) actor: String,
    pub(crate) org: String,
    pub(crate) team: String,
}

impl SkillLibrary {
    pub fn list(&self) -> tetonic_memory::Result<Vec<WorkspaceSkill>> {
        self.store
            .read_sync(|db| db.workspace_skills(&self.actor, &self.org, &self.team))?
    }
    pub fn tool_names(&self) -> Vec<String> {
        self.list()
            .unwrap_or_default()
            .into_iter()
            .filter(|s| s.enabled)
            .map(|s| s.id)
            .collect()
    }
    pub fn contains(&self, id: &str) -> bool {
        self.content(id, true).ok().flatten().is_some()
    }
    pub(crate) fn content(&self, id: &str, active: bool) -> tetonic_memory::Result<Option<String>> {
        self.store.read_sync(|db| {
            db.workspace_skill_content(&self.actor, &self.org, &self.team, id, active)
        })?
    }
    pub(crate) fn authorized(
        &self,
        scope: &tetonic_domain::ExecutionScope,
        selected: &HashSet<String>,
    ) -> bool {
        scope.organization_id == self.org
            && scope.principal_id == self.actor
            && selected
                .iter()
                .filter(|id| id.starts_with("skill_"))
                .all(|id| self.contains(id))
    }
}

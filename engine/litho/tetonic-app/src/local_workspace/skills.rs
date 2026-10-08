use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportSkill {
    pub content: String,
    #[serde(default)]
    pub source: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevokeSkill {
    pub id: String,
}

impl LocalWorkspace {
    pub fn workspace_skills(&self) -> Result<Vec<tetonic_memory::WorkspaceSkill>, AppError> {
        self.skill_library()?
            .list()
            .map_err(|_| AppError::InvalidRequest("Could not read workspace skills.".into()))
    }
    fn skill_library(&self) -> Result<&crate::skills::SkillLibrary, AppError> {
        self.host.settings.skills.as_deref().ok_or_else(|| {
            AppError::InvalidRequest("Skill library unavailable on this engine.".into())
        })
    }
    pub fn workspace_skill_content(&self, id: &str) -> Result<String, AppError> {
        self.skill_library()?
            .content(id, false)
            .map_err(|_| AppError::InvalidRequest("Could not read skill.".into()))?
            .ok_or_else(|| AppError::InvalidRequest("Skill not found.".into()))
    }
    pub async fn import_skill(
        &self,
        input: ImportSkill,
    ) -> Result<tetonic_memory::WorkspaceSkill, AppError> {
        let (name, description) =
            crate::skills::parse_skill(&input.content).map_err(AppError::InvalidRequest)?;
        if input.source.len() > 1024 || input.source.chars().any(char::is_control) {
            return Err(AppError::InvalidRequest(
                "Use a source label or URL of at most 1,024 bytes.".into(),
            ));
        }
        let id = self
            .skill_library()?
            .store
            .write(move |db| {
                db.import_workspace_skill(
                    OWNER,
                    ORG,
                    TEAM,
                    &name,
                    &description,
                    input.source.trim(),
                    &input.content,
                )
            })
            .await
            .map_err(|_| AppError::InvalidRequest("Could not save skill.".into()))?
            .map_err(|e| AppError::InvalidRequest(e.to_string()))?;
        self.workspace_skills()?
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| AppError::InvalidRequest("Could not read saved skill.".into()))
    }
    pub async fn revoke_skill(
        &self,
        input: RevokeSkill,
    ) -> Result<Vec<tetonic_memory::WorkspaceSkill>, AppError> {
        self.skill_library()?
            .store
            .write(move |db| db.revoke_workspace_skill(OWNER, ORG, TEAM, &input.id))
            .await
            .map_err(|_| AppError::InvalidRequest("Could not revoke skill.".into()))?
            .map_err(|e| AppError::InvalidRequest(e.to_string()))?;
        self.workspace_skills()
    }
}

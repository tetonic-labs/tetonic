//! Authenticated access to workspace tool-pack definitions.
use super::*;
use tetonic_memory::{SaveToolPack, ToolPack};

impl ResourceService {
    pub async fn tool_packs(
        &self,
        credential: &str,
        org: String,
        team: String,
    ) -> Result<Vec<ToolPack>, ResourceError> {
        let action = ResourceAction::ReadTeam {
            org_id: org.clone(),
            team_id: team.clone(),
        };
        let actor = self.authority.authorize(credential, &action).await?;
        let rows = self
            .store
            .read(move |db| db.tool_packs(&actor.principal_id, &org, &team))
            .await??;
        self.authority.authorize(credential, &action).await?;
        Ok(rows)
    }
    pub async fn tool_pack_revision(
        &self,
        credential: &str,
        org: String,
        team: String,
        id: String,
        revision: i64,
    ) -> Result<Option<ToolPack>, ResourceError> {
        let action = ResourceAction::ReadTeam {
            org_id: org.clone(),
            team_id: team.clone(),
        };
        let actor = self.authority.authorize(credential, &action).await?;
        let row = self
            .store
            .read(move |db| db.tool_pack_revision(&actor.principal_id, &org, &team, &id, revision))
            .await??;
        self.authority.authorize(credential, &action).await?;
        Ok(row)
    }
    pub async fn save_tool_pack(
        &self,
        credential: &str,
        org: String,
        team: String,
        input: SaveToolPack,
    ) -> Result<ToolPack, ResourceError> {
        let actor = self
            .authority
            .authorize(
                credential,
                &ResourceAction::ManageTeam {
                    org_id: org.clone(),
                    team_id: team.clone(),
                },
            )
            .await?;
        Ok(self
            .store
            .write(move |db| db.save_tool_pack(&actor.principal_id, &org, &team, &input))
            .await??)
    }
}

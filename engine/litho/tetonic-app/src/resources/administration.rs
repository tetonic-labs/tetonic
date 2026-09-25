use super::*;

impl ResourceService {
    /// Changes explicit metadata membership; ownership and organization grants are separate.
    pub async fn set_team_member(
        &self,
        credential: &str,
        org_id: String,
        team_id: String,
        subject: String,
        present: bool,
    ) -> Result<(), ResourceError> {
        let actor = self
            .authority
            .authorize(
                credential,
                &ResourceAction::ManageTeam {
                    org_id: org_id.clone(),
                    team_id: team_id.clone(),
                },
            )
            .await?;
        self.store
            .write(move |db| {
                db.administer_team_member(&actor.principal_id, &org_id, &team_id, &subject, present)
            })
            .await??;
        Ok(())
    }

    /// Organization metadata grants only; no execution or private memory access.
    pub async fn set_organization_member(
        &self,
        credential: &str,
        org_id: String,
        subject: String,
        role: Option<OrganizationRole>,
    ) -> Result<(), ResourceError> {
        let actor = self
            .authority
            .authorize(
                credential,
                &ResourceAction::ManageOrganization {
                    org_id: org_id.clone(),
                },
            )
            .await?;
        self.store
            .write(move |db| {
                db.administer_organization_member(&actor.principal_id, &org_id, &subject, role)
            })
            .await??;
        Ok(())
    }
}

impl LocalControl {
    /// Trusted local operator provisioning, not an authenticated remote endpoint.
    pub async fn register_principal(&self, principal: String) -> Result<(), ResourceError> {
        self.resources()
            .store
            .write(move |db| db.register_control_principal(&principal))
            .await??;
        Ok(())
    }
}

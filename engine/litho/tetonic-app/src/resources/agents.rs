use super::*;

impl ResourceService {
    /// Register immutable harness configuration. Requested capabilities are data,
    /// not effective grants; activation is a separate governed operation.
    pub async fn register_agent(
        &self,
        credential: &str,
        org: String,
        key: String,
        harness: String,
        configuration: serde_json::Value,
    ) -> Result<tetonic_memory::RegisteredAgent, ResourceError> {
        let actor = self
            .authority
            .authorize(
                credential,
                &ResourceAction::ManageOrganization {
                    org_id: org.clone(),
                },
            )
            .await?;
        Ok(self
            .store
            .write(move |db| {
                db.register_organization_agent(
                    &actor.principal_id,
                    &org,
                    &key,
                    &harness,
                    &configuration,
                )
            })
            .await??)
    }

    pub async fn get_agent(
        &self,
        credential: &str,
        org: String,
        key: String,
    ) -> Result<Option<tetonic_memory::RegisteredAgent>, ResourceError> {
        let actor = self
            .authority
            .authorize(
                credential,
                &ResourceAction::ReadOrganization {
                    org_id: org.clone(),
                },
            )
            .await?;
        Ok(self
            .store
            .read(move |db| db.get_organization_agent(&actor.principal_id, &org, &key))
            .await??)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn agent_registration_uses_verified_org_authority() {
        let dir = tempfile::tempdir().unwrap();
        let local = LocalControl::open(dir.path().join("control.db"), "test".into())
            .await
            .unwrap();
        local
            .bootstrap("admin".into(), "org".into(), "Org".into())
            .await
            .unwrap();
        let issued = local
            .credentials()
            .issue("admin".into(), 3600)
            .await
            .unwrap();
        let resources = local.resources();
        let config = serde_json::json!({"instructions":"Investigate the assigned goal"});
        assert!(resources
            .register_agent(
                "forged",
                "org".into(),
                "researcher".into(),
                "general".into(),
                config.clone()
            )
            .await
            .is_err());
        let first = resources
            .register_agent(
                issued.expose_secret(),
                "org".into(),
                "researcher".into(),
                "general".into(),
                config.clone(),
            )
            .await
            .unwrap();
        let retry = resources
            .register_agent(
                issued.expose_secret(),
                "org".into(),
                "researcher".into(),
                "general".into(),
                config,
            )
            .await
            .unwrap();
        assert_eq!(first, retry);
        assert_eq!(
            resources
                .get_agent(issued.expose_secret(), "org".into(), "researcher".into())
                .await
                .unwrap(),
            Some(first)
        );
        local
            .credentials()
            .revoke(issued.credential_id.clone())
            .await
            .unwrap();
        assert!(resources
            .get_agent(issued.expose_secret(), "org".into(), "researcher".into())
            .await
            .is_err());
    }
}

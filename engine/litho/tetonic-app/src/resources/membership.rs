use super::*;
use tetonic_memory::ControlPermission;

/// Trusted identity-provider adapter. Verify credential authenticity, expiry,
/// revocation and audience before returning a stable, issuer-qualified principal
/// ID. A supplied username, device certificate or model assertion is insufficient.
/// This establishes identity only; the store decides resource access separately.
#[async_trait]
pub trait CredentialVerifier: Send + Sync {
    /// Optional synchronous lifetime check for recall. Unsupported adapters deny
    /// recall binding rather than silently retaining admission-only authority.
    fn memory_credential_check(
        &self,
        _credential: &str,
    ) -> Option<tetonic_tools::MemoryCredentialCheck> {
        None
    }

    async fn verify(&self, credential: &str) -> Result<AuthorizedPrincipal, AccessError>;
}

pub(super) struct MembershipAuthority {
    pub(super) store: SharedStore,
    pub(super) verifier: Arc<dyn CredentialVerifier>,
}

#[async_trait]
impl ResourceAuthority for MembershipAuthority {
    async fn authorize(
        &self,
        credential: &str,
        action: &ResourceAction,
    ) -> Result<AuthorizedPrincipal, AccessError> {
        let principal = self.verifier.verify(credential).await?;
        let (permission, org, team) = match action {
            ResourceAction::ManageTeam { org_id, team_id } => (
                ControlPermission::ManageTeam,
                org_id.clone(),
                team_id.clone(),
            ),
            ResourceAction::ManageOrganization { org_id } => (
                ControlPermission::ManageOrganization,
                org_id.clone(),
                String::new(),
            ),
            ResourceAction::CreateOrganization { .. } => (
                ControlPermission::CreateOrganization,
                String::new(),
                String::new(),
            ),
            ResourceAction::ReadOrganization { org_id } => (
                ControlPermission::ReadOrganization,
                org_id.clone(),
                String::new(),
            ),
            ResourceAction::CreateTeam { org_id, team_id } => (
                ControlPermission::CreateTeam,
                org_id.clone(),
                team_id.clone(),
            ),
            ResourceAction::ReadTeam { org_id, team_id } => {
                (ControlPermission::ReadTeam, org_id.clone(), team_id.clone())
            }
        };
        let id = principal.principal_id.clone();
        let allowed = self
            .store
            .read(move |db| db.control_access(&id, permission, &org, &team))
            .await
            .map_err(|_| AccessError)?
            .map_err(|_| AccessError)?;
        if allowed {
            Ok(principal)
        } else {
            Err(AccessError)
        }
    }
}

impl crate::Application {
    /// Persistent resource authorization composed with an explicit identity
    /// verifier. No caller-provided roles and no cached membership decisions.
    pub fn membership_resource_service(
        &self,
        verifier: Arc<dyn CredentialVerifier>,
    ) -> Result<ResourceService, ResourceError> {
        let store = self
            .run_manager
            .managed()
            .store()
            .cloned()
            .ok_or(ResourceError::StorageRequired)?;
        let authority = Arc::new(MembershipAuthority {
            store: store.clone(),
            verifier,
        });
        Ok(ResourceService { store, authority })
    }
}

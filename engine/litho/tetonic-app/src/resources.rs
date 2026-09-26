//! Authorized durable resource operations. This is not an execution authority.
//! Authentication adapters must verify credentials and evaluate current grants;
//! request bodies never supply a trusted principal or a set of roles.

use std::sync::Arc;

use async_trait::async_trait;
use tetonic_memory::{OrganizationRow, SharedStore, StoreError, TeamRow};

mod local_control;
mod local_credentials;
pub use local_control::LocalControl;
mod membership;
mod administration;
mod team_work;
mod team_work_activation;
pub use team_work_activation::TeamWorkLaunch;
mod contexts;
mod context_compiler;
mod credential_binding;
mod context_artifacts;
pub use contexts::ContextService;
pub use tetonic_memory::team_participation_context_id;
pub use tetonic_memory::ContextOwner;
pub use tetonic_memory::HuddleProposal;
pub use tetonic_memory::OrganizationRole;
pub use local_credentials::{IssuedCredential, LocalCredentials};
pub use membership::CredentialVerifier;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceAction {
    ManageTeam { org_id: String, team_id: String },
    ManageOrganization { org_id: String },
    CreateOrganization { org_id: String },
    ReadOrganization { org_id: String },
    CreateTeam { org_id: String, team_id: String },
    ReadTeam { org_id: String, team_id: String },
}

/// Produced by a trusted authentication/authorization adapter, never deserialized
/// from a client request. Device identity and agent identity are separate concepts.
pub struct AuthorizedPrincipal {
    principal_id: String,
}

impl AuthorizedPrincipal {
    pub fn new(principal_id: String) -> Result<Self, AccessError> {
        if principal_id.trim().is_empty() || principal_id.contains('\0') {
            return Err(AccessError);
        }
        Ok(Self { principal_id })
    }
}

#[derive(Debug, thiserror::Error)]
#[error("resource access denied")]
pub struct AccessError;

/// Trusted composition dependency, not an extension callable by an agent.
/// Validate credential signature/session, expiry, organization membership and
/// exact requested action. Errors, unavailable policy and unknown grants deny.
/// Do not log credentials. Evaluate afresh on each call (including retries).
/// Authorization linearizes at this decision; this interface does not promise
/// atomic revocation of an operation already admitted to storage.
#[async_trait]
pub trait ResourceAuthority: Send + Sync {
    async fn authorize(
        &self,
        credential: &str,
        action: &ResourceAction,
    ) -> Result<AuthorizedPrincipal, AccessError>;
}

#[derive(Debug, thiserror::Error)]
pub enum ResourceError {
    #[error("resource access denied")]
    Denied,
    #[error("organization must retain an enabled administrator")]
    LastAdministrator,
    #[error("durable resource storage is required")]
    StorageRequired,
    #[error("resource already exists with different attributes")]
    Conflict,
    #[error("invalid resource attributes")]
    Invalid,
    #[error("resource storage operation failed")]
    Storage,
}

impl From<AccessError> for ResourceError {
    fn from(_: AccessError) -> Self {
        Self::Denied
    }
}

impl From<StoreError> for ResourceError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::ControlAccessDenied => Self::Denied,
            StoreError::LastOrganizationAdministrator => Self::LastAdministrator,
            StoreError::ControlResourceConflict => Self::Conflict,
            StoreError::InvalidControlResource(_) => Self::Invalid,
            _ => Self::Storage,
        }
    }
}

pub struct ResourceService {
    store: SharedStore,
    authority: Arc<dyn ResourceAuthority>,
}

impl crate::Application {
    /// Compose with the same durable store used by managed execution. No volatile
    /// fallback, implicit local administrator, or second database is introduced.
    pub fn resource_service(
        &self,
        authority: Arc<dyn ResourceAuthority>,
    ) -> Result<ResourceService, ResourceError> {
        let store = self
            .run_manager
            .managed()
            .store()
            .cloned()
            .ok_or(ResourceError::StorageRequired)?;
        Ok(ResourceService { store, authority })
    }
}

impl ResourceService {
    pub async fn create_organization(
        &self,
        credential: &str,
        org_id: String,
        name: String,
    ) -> Result<OrganizationRow, ResourceError> {
        self.authority
            .authorize(
                credential,
                &ResourceAction::CreateOrganization {
                    org_id: org_id.clone(),
                },
            )
            .await?;
        let row = OrganizationRow { org_id, name };
        self.store
            .write(move |db| {
                db.create_organization(&row)?;
                Ok::<_, StoreError>(row)
            })
            .await?
            .map_err(Into::into)
    }

    pub async fn get_organization(
        &self,
        credential: &str,
        org_id: String,
    ) -> Result<Option<OrganizationRow>, ResourceError> {
        self.authority
            .authorize(
                credential,
                &ResourceAction::ReadOrganization {
                    org_id: org_id.clone(),
                },
            )
            .await?;
        self.store
            .read(move |db| db.get_organization(&org_id))
            .await?
            .map_err(Into::into)
    }

    pub async fn create_team(
        &self,
        credential: &str,
        org_id: String,
        team_id: String,
        name: String,
    ) -> Result<TeamRow, ResourceError> {
        let principal = self
            .authority
            .authorize(
                credential,
                &ResourceAction::CreateTeam {
                    org_id: org_id.clone(),
                    team_id: team_id.clone(),
                },
            )
            .await?;
        let row = TeamRow {
            org_id,
            team_id,
            name,
            owner_principal_id: principal.principal_id,
        };
        self.store
            .write(move |db| {
                db.create_team(&row)?;
                Ok::<_, StoreError>(row)
            })
            .await?
            .map_err(Into::into)
    }

    pub async fn get_team(
        &self,
        credential: &str,
        org_id: String,
        team_id: String,
    ) -> Result<Option<TeamRow>, ResourceError> {
        // Authorize before reading: unknown and inaccessible resources have the
        // same denied response. A guessed identifier conveys no authority.
        self.authority
            .authorize(
                credential,
                &ResourceAction::ReadTeam {
                    org_id: org_id.clone(),
                    team_id: team_id.clone(),
                },
            )
            .await?;
        self.store
            .read(move |db| db.get_team(&org_id, &team_id))
            .await?
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod context_pipeline_tests;

mod agents;

mod general_harness;
pub use general_harness::{HarnessPreparationLimits, PreparedAgentRevision};

mod execution_authority;

mod execution_grants;
mod execution_limits;
pub use execution_limits::OrganizationExecutionLimits;

mod run_inspection;
pub use run_inspection::RunPoll;

mod activation;
pub use activation::RegisteredAgentJob;

mod registered_executor;
pub use registered_executor::{RegisteredExecutionSettings, RegisteredAgentSubmission, RegisteredAgentExecution};

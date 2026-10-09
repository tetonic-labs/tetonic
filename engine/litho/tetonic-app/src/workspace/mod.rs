//! Scoped agent configuration, provider selection and capability libraries.
use crate::errors::AppError;
use crate::job_launch::PreparedLaunch;
use crate::resources::{ApplicationScope, LocalControl};
use serde::{Deserialize, Serialize};
mod defaults;
pub(crate) use defaults::*;
pub const INPUT_LIMIT: usize = 12_000;
pub(crate) const DEFAULT_WORK_TOKENS: u64 = 4096;
pub(crate) const LOCAL_TOKEN_CEILING: u64 = 12_288;
pub(crate) mod agents;
mod folders;
mod mcp;
pub(crate) mod providers;
mod skills;
pub use agents::{CreateLocalAgent, LocalAgent, LocalAgentCatalog, UpdateLocalAgent};
pub use mcp::SaveMcpConnection;
pub use providers::{LocalModelCatalog, LocalProvider, RemoveProviderKey, SaveProviderKey};
pub use skills::{ImportSkill, RevokeSkill};
#[derive(Clone)]
pub(crate) struct WorkspaceServices {
    pub(crate) local: LocalControl,
    pub(crate) host: PreparedLaunch,
    pub(crate) scope: ApplicationScope,
    pub(crate) execution: crate::host::WorkspaceExecutionConfiguration,
    pub(crate) folders: Vec<std::path::PathBuf>,
    pub(crate) protected_folders: Vec<std::path::PathBuf>,
    // Only admission/configuration changes serialize; inference never holds this lock.
    pub(crate) admission: std::sync::Arc<tokio::sync::Mutex<()>>,
    pub(crate) keys: std::sync::Arc<providers::ProviderKeys>,
    #[cfg(test)]
    pub(crate) hosted_transport:
        Option<std::sync::Arc<dyn tetonic_inference::hosted::HostedTransport>>,
}

impl WorkspaceServices {
    pub(crate) async fn bind(
        local: LocalControl,
        mut host: PreparedLaunch,
        scope: ApplicationScope,
        keys: std::sync::Arc<providers::ProviderKeys>,
        execution: crate::host::WorkspaceExecutionConfiguration,
        folders: Vec<std::path::PathBuf>,
        protected_folders: Vec<std::path::PathBuf>,
    ) -> Result<Self, AppError> {
        local
            .resources()
            .validate_application_scope(&host.credential, &scope)
            .await
            .map_err(resource)?;
        host.settings.skills = Some(std::sync::Arc::new(crate::skills::SkillLibrary {
            store: local.store().clone(),
            actor: scope.principal().into(),
            org: scope.organization().into(),
            team: scope.team().into(),
        }));
        host.settings.mcp = Some(
            crate::mcp::McpRegistry::load(crate::mcp::McpScope {
                store: local.store().clone(),
                vault: keys.vault.clone(),
                actor: scope.principal().into(),
                org: scope.organization().into(),
                team: scope.team().into(),
            })
            .map_err(AppError::InvalidRequest)?,
        );
        Ok(Self {
            local,
            host,
            scope,
            keys,
            execution,
            folders,
            protected_folders,
            admission: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            #[cfg(test)]
            hosted_transport: None,
        })
    }

    pub(crate) async fn authorized_scope(&self) -> Result<ApplicationScope, AppError> {
        self.local
            .resources()
            .validate_application_scope(&self.host.credential, &self.scope)
            .await
            .map_err(resource)?;
        Ok(self.scope.clone())
    }
}
pub(crate) fn resource(error: crate::resources::ResourceError) -> AppError {
    error.into()
}
pub(crate) fn validate_request_id(id: &str) -> Result<(), AppError> {
    if uuid::Uuid::parse_str(id).is_err() {
        return Err(AppError::InvalidRequest("invalid request id".into()));
    }
    Ok(())
}
#[cfg(test)]
use crate::local_workspace::{LocalWorkspace, ORG, OWNER, TEAM};

#[cfg(test)]
use crate::work::{LocalTask, ResolveApprovalRequest, WorkPurpose};
use tetonic_domain::DataClass;

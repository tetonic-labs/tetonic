//! Compatibility entry points delegate capability management to its scoped owner.
use super::*;
impl WorkService {
    pub fn with_mcp_config(mut self, bytes: &[u8]) -> Result<Self, AppError> {
        self.services = self.services.with_mcp_config(bytes)?;
        Ok(self)
    }
    pub async fn update_agent(&self, input: UpdateLocalAgent) -> Result<LocalAgent, AppError> {
        self.services.update_agent(input).await
    }
    pub async fn agent_catalog(&self) -> Result<LocalAgentCatalog, AppError> {
        self.services.agent_catalog().await
    }
    pub async fn create_agent(&self, input: CreateLocalAgent) -> Result<LocalAgent, AppError> {
        self.services.create_agent(input).await
    }
    pub fn mcp_connections(&self) -> Vec<crate::mcp::McpConnectionView> {
        self.services.mcp_connections()
    }
    pub async fn discover_mcp(&self, id: &str) -> Result<crate::mcp::McpConnectionView, AppError> {
        self.services.discover_mcp(id).await
    }
    pub async fn save_mcp_connection(
        &self,
        input: SaveMcpConnection,
    ) -> Result<crate::mcp::McpConnectionView, AppError> {
        self.services.save_mcp_connection(input).await
    }
    pub async fn provider_models(&self, provider: &str) -> Result<LocalModelCatalog, AppError> {
        self.services.provider_models(provider).await
    }
    pub async fn remove_provider_key(
        &self,
        input: RemoveProviderKey,
    ) -> Result<LocalProvider, AppError> {
        self.services.remove_provider_key(input).await
    }
    pub async fn save_provider_key(
        &self,
        input: SaveProviderKey,
    ) -> Result<LocalProvider, AppError> {
        self.services.save_provider_key(input).await
    }
    pub fn workspace_skills(&self) -> Result<Vec<tetonic_memory::WorkspaceSkill>, AppError> {
        self.services.workspace_skills()
    }
    pub fn workspace_skill_content(&self, id: &str) -> Result<String, AppError> {
        self.services.workspace_skill_content(id)
    }
    pub async fn import_skill(
        &self,
        input: ImportSkill,
    ) -> Result<tetonic_memory::WorkspaceSkill, AppError> {
        self.services.import_skill(input).await
    }
    pub async fn revoke_skill(
        &self,
        input: RevokeSkill,
    ) -> Result<Vec<tetonic_memory::WorkspaceSkill>, AppError> {
        self.services.revoke_skill(input).await
    }
}

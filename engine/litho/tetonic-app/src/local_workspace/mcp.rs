use super::*;

impl LocalWorkspace {
    /// Startup-only operator configuration. Never called from a model or a web payload.
    pub fn with_mcp_config(mut self, bytes: &[u8]) -> Result<Self, AppError> {
        self.host.settings.mcp =
            Some(crate::mcp::McpRegistry::from_json(bytes).map_err(AppError::InvalidRequest)?);
        Ok(self)
    }
    pub fn mcp_connections(&self) -> Vec<crate::mcp::McpConnectionView> {
        self.host
            .settings
            .mcp
            .as_ref()
            .map(|m| m.views())
            .unwrap_or_default()
    }
    pub async fn discover_mcp(&self, id: &str) -> Result<crate::mcp::McpConnectionView, AppError> {
        self.host
            .settings
            .mcp
            .as_ref()
            .ok_or_else(|| {
                AppError::InvalidRequest("No MCP connections are configured on this engine".into())
            })?
            .refresh(id)
            .await
            .map_err(AppError::InvalidRequest)
    }
}

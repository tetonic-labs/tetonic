use super::*;

impl LocalWorkspace {
    /// Startup-only operator configuration. Never called from a model or a web payload.
    pub fn with_mcp_config(self, bytes: &[u8]) -> Result<Self, AppError> {
        self.mcp_registry()?
            .add_operator_connections(bytes)
            .map_err(AppError::InvalidRequest)?;
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

    pub(super) fn mcp_scope(&self) -> crate::mcp::McpScope {
        crate::mcp::McpScope {
            store: self.keys.store.clone(),
            vault: self.keys.vault.clone(),
            actor: OWNER.into(),
            org: ORG.into(),
            team: TEAM.into(),
        }
    }
    fn mcp_registry(&self) -> Result<&crate::mcp::McpRegistry, AppError> {
        self.host
            .settings
            .mcp
            .as_deref()
            .ok_or_else(|| AppError::InvalidRequest("Connection library unavailable".into()))
    }
    /// Authenticated local-owner operation. No model/tool can call this API.
    pub async fn save_mcp_connection(
        &self,
        input: SaveMcpConnection,
    ) -> Result<crate::mcp::McpConnectionView, AppError> {
        use tetonic_domain::key_storage::{SecretBytes, SecretKeyRef};
        let _serial = self.admission.lock().await;
        let scope = self.mcp_scope();
        let registry = self.mcp_registry()?;
        let old = scope.record(&input.id);
        if old.as_ref().map_or(0, |r| r.revision) != input.expected_revision
            || (old.is_none() && registry.views().iter().any(|r| r.id == input.id))
        {
            return Err(AppError::InvalidRequest(
                "Connection changed. Reload its settings before saving.".into(),
            ));
        }
        let endpoint = tetonic_egress::mcp_endpoint(input.endpoint.trim()).map_err(|_| AppError::InvalidRequest("Use a public HTTPS endpoint or numeric loopback HTTP, without credentials, query or fragment.".into()))?;
        if old.as_ref().is_some_and(|r| r.endpoint != endpoint) {
            return Err(AppError::InvalidRequest("Add a new connection to use a different endpoint. Existing agent access cannot be redirected.".into()));
        }
        if !matches!(input.auth.as_str(), "none" | "bearer")
            || input.name.trim().is_empty()
            || input.name.len() > 80
            || input.name.chars().any(char::is_control)
            || input.id.is_empty()
            || input.id.len() > 20
            || !input
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(AppError::InvalidRequest(
                "Choose a name and supported authentication method.".into(),
            ));
        }
        let changing_auth = input.token.is_some()
            || old.as_ref().is_some_and(|r| r.auth != input.auth)
            || !input.enabled;
        if changing_auth && input.approved_tools.as_ref().is_some_and(|v| !v.is_empty()) {
            return Err(AppError::InvalidRequest(
                "Changing credentials requires rediscovery and a fresh tool review.".into(),
            ));
        }
        let approved_manifests = if changing_auth {
            vec![]
        } else if let Some(ids) = &input.approved_tools {
            if old.is_none() && ids.is_empty() {
                vec![]
            } else {
                registry
                    .reviewed_manifests(&input.id, ids)
                    .map_err(AppError::InvalidRequest)?
            }
        } else {
            old.as_ref()
                .map(|r| r.approved_manifests.clone())
                .unwrap_or_default()
        };
        let new_secret = if let Some(token) = input.token {
            if input.auth != "bearer"
                || !input.enabled
                || token.len() > 8192
                || tetonic_egress::HostedCredential::bearer(&token).is_err()
            {
                return Err(AppError::InvalidRequest(
                    "Enter a valid service token, without spaces or line breaks.".into(),
                ));
            }
            let secret = SecretBytes::new(token.into_bytes());
            let vault = scope.vault.clone();
            Some(
                tokio::task::spawn_blocking(move || vault.create(secret.as_ref()))
                    .await
                    .map_err(|_| credential_error())?
                    .map_err(|_| credential_error())?,
            )
        } else {
            None
        };
        let secret_ref = if input.auth == "none" || !input.enabled {
            None
        } else {
            new_secret
                .as_ref()
                .map(|r| r.0.clone())
                .or_else(|| old.as_ref().and_then(|r| r.secret_ref.clone()))
        };
        let row = tetonic_memory::WorkspaceMcpConnection {
            id: input.id.clone(),
            name: input.name.trim().into(),
            endpoint,
            auth: input.auth,
            secret_ref,
            revision: input
                .expected_revision
                .checked_add(1)
                .ok_or_else(|| AppError::InvalidRequest("Connection revision exhausted".into()))?,
            binding_epoch: old
                .as_ref()
                .map_or(1, |r| r.binding_epoch + u64::from(changing_auth)),
            enabled: input.enabled,
            approved_manifests,
        };
        let saved = row.clone();
        let write = scope
            .store
            .write(move |db| {
                db.save_workspace_mcp(OWNER, ORG, TEAM, &saved, input.expected_revision)
            })
            .await;
        if !matches!(write, Ok(Ok(()))) {
            if let Some(reference) = new_secret {
                let vault = scope.vault.clone();
                let _ = tokio::task::spawn_blocking(move || vault.delete(&reference)).await;
            }
            return Err(AppError::InvalidRequest(
                "Could not save connection. Check the token and reload before retrying.".into(),
            ));
        }
        registry
            .install(scope.clone(), &row)
            .map_err(AppError::InvalidRequest)?;
        if let Some(reference) = old
            .and_then(|r| r.secret_ref)
            .filter(|r| row.secret_ref.as_ref() != Some(r))
        {
            let vault = scope.vault.clone();
            let _ =
                tokio::task::spawn_blocking(move || vault.delete(&SecretKeyRef(reference))).await;
        }
        registry
            .views()
            .into_iter()
            .find(|v| v.id == row.id)
            .ok_or_else(|| {
                AppError::InvalidRequest("Connection saved; refresh the library.".into())
            })
    }
}

// Never Debug/Serialize credential-bearing input.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveMcpConnection {
    pub id: String,
    pub expected_revision: u64,
    pub name: String,
    pub endpoint: String,
    pub auth: String,
    #[serde(default)]
    pub token: Option<String>,
    pub enabled: bool,
    pub approved_tools: Option<Vec<String>>,
}
fn credential_error() -> AppError {
    AppError::InvalidRequest("Could not access the OS credential store. Unlock it and retry; service tokens are never stored as plaintext.".into())
}

#[cfg(test)]
mod connection_tests;

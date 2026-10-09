use super::*;
use tetonic_domain::key_storage::{KeyStorage, SecretKeyRef};
use tetonic_memory::{SharedStore, WorkspaceMcpConnection};

#[derive(Clone)]
pub(crate) struct McpScope {
    pub store: SharedStore,
    pub vault: Arc<dyn KeyStorage>,
    pub actor: String,
    pub org: String,
    pub team: String,
}
impl McpScope {
    pub fn records(&self) -> Result<Vec<WorkspaceMcpConnection>, String> {
        self.store
            .read_sync(|db| db.workspace_mcp_connections(&self.actor, &self.org, &self.team))
            .map_err(|_| "Connection storage unavailable")?
            .map_err(|_| "Connection storage unavailable".into())
    }
    pub fn record(&self, id: &str) -> Option<WorkspaceMcpConnection> {
        self.store
            .read_sync(|db| db.workspace_mcp_connection(&self.actor, &self.org, &self.team, id))
            .ok()?
            .ok()?
    }
}
impl McpRegistry {
    pub(super) fn all(&self) -> Vec<Arc<Connection>> {
        self.connections
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    pub(super) fn connection(&self, id: &str) -> Option<Arc<Connection>> {
        self.all().into_iter().find(|c| c.config.id == id)
    }
    pub(crate) fn load(scope: McpScope) -> Result<Arc<Self>, String> {
        let registry = Arc::new(Self {
            connections: RwLock::new(vec![]),
        });
        for record in scope.records()? {
            registry.install(scope.clone(), &record)?;
        }
        Ok(registry)
    }
    pub(crate) fn add_operator_connections(&self, config: &[u8]) -> Result<(), String> {
        let operator = Self::from_json(config)?;
        let mut current = self.connections.write().unwrap_or_else(|e| e.into_inner());
        if operator
            .all()
            .iter()
            .any(|c| current.iter().any(|old| old.config.id == c.config.id))
        {
            return Err("An operator connection ID conflicts with a workspace connection".into());
        }
        current.extend(operator.all());
        Ok(())
    }
    pub(crate) fn install(
        &self,
        scope: McpScope,
        r: &WorkspaceMcpConnection,
    ) -> Result<(), String> {
        let endpoint =
            tetonic_egress::mcp_endpoint(&r.endpoint).map_err(|_| "Invalid MCP endpoint")?;
        let guard = EgressGuard::new();
        if endpoint.starts_with("https:") {
            guard
                .allow_hosted_endpoint(&endpoint)
                .map_err(|_| "Invalid MCP endpoint")?;
        } else {
            let (_, address) = tetonic_egress::local_mcp_endpoint(&endpoint)
                .map_err(|_| "Invalid MCP endpoint")?;
            guard.allow_node(format!("mcp:{}", r.id), address.ip(), Some(address.port()));
        }
        let c = Arc::new(Connection {
            config: McpConnectionConfig {
                id: r.id.clone(),
                name: r.name.clone(),
                endpoint,
                read_tools: vec![],
                action_tools: vec![],
            },
            stored: Some((scope, r.binding_epoch)),
            guard,
            refresh: tokio::sync::Mutex::new(()),
            view: RwLock::new(McpConnectionView {
                id: r.id.clone(),
                name: r.name.clone(),
                endpoint: r.endpoint.clone(),
                status: if r.enabled {
                    "unchecked"
                } else {
                    "disconnected"
                }
                .into(),
                message: if r.enabled {
                    "Saved access restored. Each call checks the connection and exact tool version."
                } else {
                    "Disconnected. Saved agent selections remain visible, but cannot execute."
                }
                .into(),
                tools: vec![],
                editable: true,
                revision: r.revision,
                auth: r.auth.clone(),
                enabled: r.enabled,
            }),
        });
        let previous = self.connection(&r.id).filter(|old| {
            r.enabled
                && old
                    .stored
                    .as_ref()
                    .is_some_and(|(_, epoch)| *epoch == r.binding_epoch)
        });
        let checked = previous.as_ref().and_then(|old| {
            old.view
                .read()
                .ok()
                .filter(|v| v.status == "discovered")
                .map(|v| v.clone())
        });
        let manifests = checked
            .as_ref()
            .map(|v| {
                v.tools
                    .iter()
                    .map(|t| t.manifest.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| r.approved_manifests.clone());
        let mut tools = Vec::new();
        for manifest in &manifests {
            tools.push(pinned_tool(&c, manifest.clone())?);
        }
        {
            let mut view = c.view.write().unwrap_or_else(|e| e.into_inner());
            view.tools = tools;
            if checked.is_some() {
                view.status = "discovered".into();
                view.message =
                    "Connection checked. Reviewed tools are available to select in agent settings."
                        .into();
            }
        }
        let mut connections = self.connections.write().unwrap_or_else(|e| e.into_inner());
        if let Some(index) = connections.iter().position(|old| old.config.id == r.id) {
            if connections[index].stored.is_none() {
                return Err("Operator connections cannot be edited here".into());
            }
            connections[index] = c;
        } else {
            connections.push(c);
        }
        Ok(())
    }
    pub(crate) fn reviewed_manifests(
        &self,
        id: &str,
        ids: &[String],
    ) -> Result<Vec<Value>, String> {
        let c = self.connection(id).ok_or("Connection not found")?;
        let view = c.view.read().unwrap_or_else(|e| e.into_inner());
        if ids.len() > 32 || ids.iter().collect::<HashSet<_>>().len() != ids.len() {
            return Err("Select at most 32 distinct tools".into());
        }
        ids.iter()
            .map(|id| {
                view.tools
                    .iter()
                    .find(|t| &t.id == id)
                    .map(|t| t.manifest.clone())
                    .ok_or_else(|| {
                        "Tool discovery changed. Check the connection and review again.".into()
                    })
            })
            .collect()
    }
    pub(crate) fn authorized(
        &self,
        scope: &tetonic_domain::ExecutionScope,
        selected: &[String],
    ) -> bool {
        selected
            .iter()
            .filter(|id| id.starts_with("mcp_"))
            .all(|id| {
                self.binding(id).is_some_and(|(c, _)| {
                    c.stored.as_ref().is_none_or(|(s, _)| {
                        s.org == scope.organization_id && s.actor == scope.principal_id
                    })
                })
            })
    }
}
impl Connection {
    pub(super) fn view(&self) -> McpConnectionView {
        let mut view = self.view.read().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some((scope, epoch)) = &self.stored {
            let record = scope.record(&self.config.id);
            for tool in &mut view.tools {
                tool.approved = record.as_ref().is_some_and(|r| {
                    r.enabled
                        && r.binding_epoch == *epoch
                        && r.approved_manifests.contains(&tool.manifest)
                });
            }
            if record
                .as_ref()
                .is_none_or(|r| r.binding_epoch != *epoch || !r.enabled)
            {
                view.enabled = false;
                view.status = "disconnected".into();
                view.message =
                    "Connection access changed or is unavailable. Reload the connection settings."
                        .into();
            }
        }
        view
    }
    fn current(&self) -> bool {
        self.stored.as_ref().is_none_or(|(s, epoch)| {
            s.record(&self.config.id)
                .is_some_and(|r| r.enabled && r.binding_epoch == *epoch)
        })
    }
    pub(super) fn allowed(&self, tool: &McpTool) -> bool {
        self.stored.as_ref().is_none_or(|(s, epoch)| {
            s.record(&self.config.id).is_some_and(|r| {
                r.enabled
                    && r.binding_epoch == *epoch
                    && r.approved_manifests.contains(&tool.manifest)
            })
        })
    }
    pub(super) async fn credential(
        &self,
    ) -> Result<Option<tetonic_egress::HostedCredential>, String> {
        let Some((scope, epoch)) = &self.stored else {
            return Ok(None);
        };
        let record = scope
            .record(&self.config.id)
            .filter(|r| r.enabled && r.binding_epoch == *epoch)
            .ok_or("Connection disconnected or changed. Review its current access.")?;
        if record.auth == "none" {
            return Ok(None);
        }
        let reference = record
            .secret_ref
            .ok_or("Service token missing. Update the connection.")?;
        let vault = scope.vault.clone();
        let secret = tokio::task::spawn_blocking(move || vault.read(&SecretKeyRef(reference)))
            .await
            .map_err(|_| "Credential store unavailable")?
            .map_err(|_| "Service token unavailable. Update the connection.")?;
        let value = std::str::from_utf8(secret.as_ref()).map_err(|_| "Invalid service token")?;
        tetonic_egress::HostedCredential::bearer(value)
            .map(Some)
            .map_err(|_| "Invalid service token".into())
    }
    pub(super) async fn revoked(&self) {
        while self.current() {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    }
    pub(super) async fn tool_revoked(&self, tool: &McpTool) {
        while self.allowed(tool) {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    }
}

pub(crate) struct McpAuthority {
    pub inner: Arc<dyn tetonic_run::managed::ExecutionAuthority>,
    pub registry: Arc<McpRegistry>,
}
#[async_trait::async_trait]
impl tetonic_run::managed::ExecutionAuthority for McpAuthority {
    async fn authorize(
        &self,
        scope: &tetonic_domain::ExecutionScope,
        identity: &tetonic_domain::AgentIdentity,
        job: &tetonic_domain::AgentJobSpec,
    ) -> Result<(), ()> {
        self.inner.authorize(scope, identity, job).await?;
        self.registry
            .authorized(scope, &job.capability_bindings)
            .then_some(())
            .ok_or(())
    }
    async fn revoked_during_execution(
        &self,
        scope: &tetonic_domain::ExecutionScope,
        identity: &tetonic_domain::AgentIdentity,
        job: &tetonic_domain::AgentJobSpec,
    ) -> bool {
        !self.registry.authorized(scope, &job.capability_bindings)
            || self
                .inner
                .revoked_during_execution(scope, identity, job)
                .await
    }
}

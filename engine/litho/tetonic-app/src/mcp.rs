//! Operator-bound local MCP connections. Discovery is inventory, not an agent
//! grant. Versioned tool names pin endpoint + complete manifest into definitions.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    sync::{Arc, RwLock},
};
use tetonic_domain::work_scope::CancellationSignal;
use tetonic_domain::{ToolAdvertisement, ToolOutcome};
use tetonic_egress::EgressGuard;

mod client;
mod host;
mod persistence;
pub(crate) use persistence::McpAuthority;
pub(crate) use persistence::McpScope;
#[cfg(test)]
pub(crate) mod tests;
pub(crate) use host::McpToolHost;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpConnectionConfig {
    pub id: String,
    pub name: String,
    pub endpoint: String,
    /// Operator-vetted read operations. Server annotations never confer access.
    pub read_tools: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    connections: Vec<McpConnectionConfig>,
}

#[derive(Clone, Serialize)]
pub struct McpTool {
    pub id: String,
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub approved: bool,
    #[serde(skip)]
    manifest: Value,
}
#[derive(Clone, Serialize)]
pub struct McpConnectionView {
    pub id: String,
    pub name: String,
    pub endpoint: String,
    pub status: String,
    pub message: String,
    pub tools: Vec<McpTool>,
    pub editable: bool,
    pub revision: u64,
    pub auth: String,
    pub enabled: bool,
}
struct Connection {
    config: McpConnectionConfig,
    stored: Option<(McpScope, u64)>,
    guard: EgressGuard,
    view: RwLock<McpConnectionView>,
    refresh: tokio::sync::Mutex<()>,
}
pub struct McpRegistry {
    connections: RwLock<Vec<Arc<Connection>>>,
}

impl McpRegistry {
    pub fn from_json(bytes: &[u8]) -> Result<Arc<Self>, String> {
        if bytes.len() > 65_536 {
            return Err("MCP configuration exceeds 64 KiB".into());
        }
        let config: Config =
            serde_json::from_slice(bytes).map_err(|_| "Invalid MCP configuration")?;
        if config.connections.len() > 8 {
            return Err("At most eight MCP connections are supported".into());
        }
        let mut ids = HashSet::new();
        let mut connections = Vec::new();
        for mut c in config.connections {
            if c.id.is_empty()
                || c.id.len() > 20
                || !c.id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                || !ids.insert(c.id.clone())
                || c.name.trim().is_empty()
                || c.name.len() > 80
                || c.name.chars().any(char::is_control)
                || c.read_tools.is_empty()
                || c.read_tools.len() > 32
                || c.read_tools.iter().any(|n| !tool_name(n))
                || c.read_tools.iter().collect::<HashSet<_>>().len() != c.read_tools.len()
            {
                return Err(
                    "MCP connections need unique IDs, a name and exact vetted read tool names"
                        .into(),
                );
            }
            let (endpoint, address) =
                tetonic_egress::local_mcp_endpoint(&c.endpoint).map_err(|e| e.to_string())?;
            c.endpoint = endpoint;
            let guard = EgressGuard::new();
            guard.allow_node(format!("mcp:{}", c.id), address.ip(), Some(address.port()));
            let view = McpConnectionView {
                id: c.id.clone(),
                name: c.name.clone(),
                endpoint: c.endpoint.clone(),
                status: "unchecked".into(),
                message:
                    "Discover tools to check this connection. No tools are granted by connecting."
                        .into(),
                tools: vec![],
                editable: false,
                revision: 0,
                auth: "none".into(),
                enabled: true,
            };
            connections.push(Arc::new(Connection {
                stored: None,
                config: c,
                guard,
                view: RwLock::new(view),
                refresh: tokio::sync::Mutex::new(()),
            }));
        }
        Ok(Arc::new(Self {
            connections: RwLock::new(connections),
        }))
    }
    pub fn views(&self) -> Vec<McpConnectionView> {
        self.all().iter().map(|c| c.view()).collect()
    }
    pub fn tool_names(&self) -> Vec<String> {
        self.views()
            .into_iter()
            .flat_map(|v| v.tools.into_iter().filter(|t| t.approved).map(|t| t.id))
            .collect()
    }
    pub fn contains(&self, name: &str) -> bool {
        self.binding(name).is_some()
    }
    pub async fn refresh(&self, id: &str) -> Result<McpConnectionView, String> {
        let c = self
            .connection(id)
            .ok_or("Unknown configured MCP connection")?;
        if !c.view.read().unwrap_or_else(|e| e.into_inner()).enabled {
            return Ok(c.view.read().unwrap_or_else(|e| e.into_inner()).clone());
        }
        let _lock = c
            .refresh
            .try_lock()
            .map_err(|_| "This connection is already being checked")?;
        let result = client::discover(&c).await;
        let mut view = c.view.write().unwrap_or_else(|e| e.into_inner());
        match result {
            Ok(tools) => {
                view.tools = tools;
                view.status = "discovered".into();
                view.message = "Connection checked. Select the read tools to make available, then assign them to agents.".into();
            }
            Err(error) => {
                view.tools.clear();
                view.status = "unavailable".into();
                view.message = error;
            }
        }
        Ok(view.clone())
    }
    fn binding(&self, name: &str) -> Option<(Arc<Connection>, McpTool)> {
        self.all().into_iter().find_map(|c| {
            let tool = c
                .view
                .read()
                .ok()?
                .tools
                .iter()
                .find(|t| t.id == name && t.approved)
                .cloned();
            tool.filter(|t| c.allowed(t)).map(|t| (c.clone(), t))
        })
    }
    pub(crate) fn advertisements(&self, selected: &HashSet<String>) -> Vec<ToolAdvertisement> {
        self.views()
            .into_iter()
            .flat_map(|c| c.tools.into_iter().map(move |t| (c.name.clone(), t)))
            .filter(|(_, t)| selected.contains(&t.id) && t.approved && self.contains(&t.id))
            .map(|(connection, t)| ToolAdvertisement {
                name: t.id,
                description: format!("{} — {}. {}", connection, t.name, t.description),
                parameters: t.input_schema,
            })
            .collect()
    }
    pub(crate) fn endpoint(&self, name: &str) -> Option<String> {
        self.binding(name)
            .map(|(c, _)| format!("{}#{}", c.config.endpoint, name))
    }
    pub(crate) async fn call(
        &self,
        name: &str,
        args: &Value,
        cancel: &CancellationSignal,
    ) -> ToolOutcome {
        let Some((connection, tool)) = self.binding(name) else {
            return ToolOutcome::fail(
                "MCP tool is unavailable. Refresh its connection and check the agent's selection.",
                "unavailable",
            );
        };
        if !args.is_object() || args.to_string().len() > 16_384 {
            return ToolOutcome::fail("MCP arguments must be an object under 16 KiB", "bad_args");
        }
        client::call(&connection, tool, args, cancel).await
    }
}
fn tool_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}
fn pinned_tool(c: &Connection, manifest: Value) -> Result<McpTool, String> {
    let name = manifest
        .get("name")
        .and_then(Value::as_str)
        .ok_or("MCP tool has no name")?;
    let schema = manifest
        .get("inputSchema")
        .filter(|s| s.get("type").and_then(Value::as_str) == Some("object"))
        .ok_or("MCP tool requires an object input schema")?;
    if !tool_name(name) || manifest.to_string().len() > 16_384 {
        return Err("MCP tool manifest exceeds supported limits".into());
    }
    let digest = Sha256::digest(
        (if let Some((scope, epoch)) = &c.stored {
            json!([
                c.config.endpoint,
                c.config.id,
                manifest,
                epoch,
                scope.org,
                scope.team
            ])
        } else {
            json!([c.config.endpoint, c.config.id, manifest])
        })
        .to_string()
        .as_bytes(),
    );
    Ok(McpTool {
        id: format!("mcp_{}_{:x}", c.config.id, digest)[..(5 + c.config.id.len() + 24)].to_owned(),
        name: name.to_owned(),
        description: manifest
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or(name)
            .chars()
            .take(1500)
            .collect(),
        input_schema: schema.clone(),
        approved: c.stored.as_ref().is_none_or(|(scope, epoch)| {
            scope.record(&c.config.id).is_some_and(|r| {
                r.enabled && r.binding_epoch == *epoch && r.approved_manifests.contains(&manifest)
            })
        }),
        manifest,
    })
}

use super::*;
use tetonic_domain::{ActionKind, AuthorizedAction, CapabilityConsumer, ToolHost, ToolProposal};

/// Composite host on the existing blocking tool boundary. No separate agent loop.
#[derive(Clone)]
pub(crate) struct McpToolHost {
    pub inner: Box<dyn ToolHost>,
    pub registry: Arc<McpRegistry>,
    pub selected: HashSet<String>,
    pub consumer: Arc<dyn CapabilityConsumer>,
    pub runtime: tokio::runtime::Handle,
}
impl ToolHost for McpToolHost {
    fn checkpoint_ready(&self) -> bool {
        // This adapter opens and closes a session for each read invocation.
        // WorkScope proves quiescence; no remote session is carried across calls.
        // Endpoint and full manifest are pinned in each selected tool ID.
        self.inner.checkpoint_ready()
            && self
                .selected
                .iter()
                .filter(|name| name.starts_with("mcp_"))
                .all(|name| self.registry.contains(name))
    }

    fn clone_box(&self) -> Box<dyn ToolHost> {
        Box::new(self.clone())
    }
    fn propose(&self, name: &str, args: &Value) -> Option<ToolProposal> {
        if name.starts_with("mcp_") {
            return self.registry.endpoint(name).map(|endpoint| ToolProposal {
                kind: ActionKind::NetworkRequest,
                resolved_path: Some(endpoint),
            });
        }
        self.inner.propose(name, args)
    }
    fn is_tool_allowed(&self, name: &str) -> bool {
        if name.starts_with("mcp_") {
            self.selected.contains(name) && self.registry.contains(name)
        } else {
            self.inner.is_tool_allowed(name)
        }
    }
    fn is_read_only(&self, name: &str) -> bool {
        if name.starts_with("mcp_") {
            self.is_tool_allowed(name)
        } else {
            self.inner.is_read_only(name)
        }
    }
    fn requires_action_broker(&self, name: &str) -> bool {
        name.starts_with("mcp_") || self.inner.requires_action_broker(name)
    }
    fn requires_user_approval(&self, name: &str) -> bool {
        self.inner.requires_user_approval(name)
    }
    fn advertisements(&self) -> Vec<ToolAdvertisement> {
        let mut ads = self.inner.advertisements();
        ads.extend(self.registry.advertisements(&self.selected));
        ads
    }
    fn validate_tool_args(&self, name: &str, args: &Value) -> Result<(), String> {
        if !name.starts_with("mcp_") {
            return self.inner.validate_tool_args(name, args);
        }
        if !self.is_tool_allowed(name) || !args.is_object() || args.to_string().len() > 16_384 {
            return Err("MCP tool is unavailable or arguments exceed supported limits".into());
        }
        // The complete pinned JSON schema is advertised; the server validates
        // domain arguments. Bounds/type here do not claim full schema validation.
        Ok(())
    }
    fn execute_authorized(
        &self,
        name: &str,
        args: &Value,
        auth: Option<&AuthorizedAction>,
        cancel: &CancellationSignal,
    ) -> ToolOutcome {
        if !name.starts_with("mcp_") {
            return self.inner.execute_authorized(name, args, auth, cancel);
        }
        let Some(auth) = auth else {
            return ToolOutcome::fail("MCP invocation requires an action capability", "denied");
        };
        if cancel.is_canceled()
            || self.validate_tool_args(name, args).is_err()
            || auth.action.kind != ActionKind::NetworkRequest
            || auth.action.parameters.resolved_path != self.registry.endpoint(name)
            || auth.action.parameters.tool_arguments.as_ref() != Some(args)
            || self.consumer.authorize(auth).is_err()
        {
            return ToolOutcome::fail(
                "MCP action capability does not match this invocation or is revoked",
                "denied",
            );
        }
        self.runtime
            .block_on(self.registry.call(name, args, cancel))
    }
}

use super::prelude::*;
use subtle::ConstantTimeEq;

impl Daemon {
    pub(in crate::daemon) async fn initialize(&mut self, params: Value) -> Result<Value, RpcError> {
        let p: InitializeParams = parse(params)?;
        if !rpc_auth_disabled()
            && !bool::from(p.rpc_token.as_bytes().ct_eq(self.rpc_token.as_bytes()))
        {
            return Err(RpcError::new(
                ErrorCode::InvalidRequest,
                "invalid rpc_token",
            ));
        }
        if self.in_flight.load(Ordering::Relaxed) > 0
            || self
                .services
                .as_ref()
                .is_some_and(|s| s.app.sessions.live_count() > 0)
        {
            return Err(RpcError::new(
                ErrorCode::InvalidRequest,
                "active sessions or in-flight turns — end sessions before re-initialize",
            ));
        }
        self.rpc_authenticated.store(true, Ordering::Relaxed);

        let placement_sink: Arc<dyn lokai_app::DispatchPlacementSink> = Arc::new(
            crate::daemon::placement::DaemonPlacementSink::new(self.notifier.clone()),
        );

        let out = lokai_app::Application::bootstrap_daemon(lokai_app::DaemonBootstrapParams {
            workspace_root: p.workspace_root,
            event_sink: Arc::new(crate::daemon::events::DaemonEventSink::new(
                self.notifier.clone(),
            )),
            placement_sink: Some(placement_sink),
        })
        .await
        .map_err(|e| RpcError::new(ErrorCode::InvalidParams, format!("app: {e}")))?;

        let caps = Capabilities {
            streaming: true,
            approvals: true,
            tools: out.tools.clone(),
            orchestration_tools: vec!["spawn_agent".to_string()],
        };

        let capacity = out.capacity_status.clone().map(to_capacity_summary);

        let result = InitializeResult {
            protocol_version: lokai_rpc::PROTOCOL_VERSION,
            daemon_info: DaemonInfo {
                name: "lokaid".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            capabilities: caps,
            capacity,
            rpc_token: None,
        };

        if !out.reachable {
            tracing::warn!("inference runtime not reachable; chat will fail until it is running");
        }
        if out.shell_tool_enabled {
            tracing::warn!(
                "run_shell is enabled — subprocesses bypass EgressGuard network policy; use approval hooks"
            );
        }

        self.services = Some(EngineServices {
            app: out.app,
            workspace_root: out.workspace_root,
            model: out.model_fast,
            model_hard: out.model_hard,
            models: out.models,
            tool_capable: out.tool_capable,
            tools: out.tools,
            fabric_pooled: out.fabric_pooled,
            num_ctx: out.num_ctx,
            ollama_base: out.ollama_base,
            capacity: out.capacity_status,
        });

        Ok(to_value(result))
    }
}

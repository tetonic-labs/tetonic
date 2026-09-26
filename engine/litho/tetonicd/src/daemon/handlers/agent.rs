use super::prelude::*;

impl Daemon {
    pub(in crate::daemon) fn agent_spawn(&mut self, params: Value) -> Result<Value, RpcError> {
        let p: AgentSpawnParams = parse(params)?;
        let services = self.services()?;
        let live = services
            .app
            .sessions
            .live(&p.session_id)
            .map_err(|_| RpcError::new(ErrorCode::UnknownSession, "unknown session_id"))?;

        let parent_agent_id = live
            .spawn_track
            .resolve_spawn_parent(p.parent_agent_id.as_deref())
            .map_err(|e| RpcError::new(ErrorCode::InvalidParams, e))?;
        let n = live.spawn_serial.fetch_add(1, Ordering::Relaxed);
        let agent_id = next_child_agent_id(&parent_agent_id, n);
        let spawned_id = agent_id.clone();

        services
            .app
            .submit_spawn(tetonic_app::commands::SpawnAgentCommand {
                session_id: p.session_id,
                agent_id,
                parent_agent_id,
                role: p.role,
                task: p.task,
            })
            .map_err(crate::daemon::rpc::map::map_app_error)?;

        Ok(to_value(AgentSpawnResult {
            agent_id: spawned_id,
            accepted: true,
        }))
    }
}

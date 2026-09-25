use super::prelude::*;

impl Daemon {
    pub(in crate::daemon) fn egress_policy_get(&mut self) -> Result<Value, RpcError> {
        let services = self.services()?;
        let allow: Vec<EgressAllowRule> = services
            .app
            .egress_allow_rules()
            .into_iter()
            .map(|r| EgressAllowRule {
                label: r.label,
                ip: r.ip.to_string(),
                port: r.port,
            })
            .collect();
        Ok(to_value(EgressPolicy {
            default: "deny".to_string(),
            allow,
        }))
    }

    pub(in crate::daemon) fn egress_policy_set(
        &mut self,
        params: Value,
    ) -> Result<Value, RpcError> {
        if !rpc_egress_mutations_allowed() {
            return Err(RpcError::new(
                ErrorCode::InvalidRequest,
                "egress/policy.set is disabled over stdio RPC — use `lokai estate egress allow` or set LOKAI_ALLOW_RPC_EGRESS=1 for development",
            ));
        }
        let p: EgressPolicySetParams = parse(params)?;
        let services = self.services()?;
        for rule in &p.add {
            services
                .app
                .upsert_egress_allow_rule(&rule.label, &rule.ip, rule.port)
                .map_err(|e| RpcError::new(ErrorCode::InternalError, format!("egress: {e}")))?;
        }
        for label in &p.remove {
            let _ = services.app.remove_egress_allow_rule(label);
        }
        let allow: Vec<EgressAllowRule> = services
            .app
            .egress_allow_rules()
            .into_iter()
            .map(|r| EgressAllowRule {
                label: r.label,
                ip: r.ip.to_string(),
                port: r.port,
            })
            .collect();
        Ok(to_value(EgressPolicySetResult { ok: true, allow }))
    }

    fn policy_epoch(services: &EngineServices) -> u64 {
        services.app.policy_epoch()
    }

    fn bump_policy_epoch(services: &EngineServices) {
        services.app.bump_policy_epoch();
    }

    // ---- policy/get | policy/set (D1) --------------------------------------

    pub(in crate::daemon) async fn policy_get(&self) -> Result<Value, RpcError> {
        let services = self.services()?;
        let cmd = tetonic_app::commands::GetPolicyCommand {
            workspace_root: services.workspace_root.clone(),
        };
        let result = services
            .app
            .policies
            .get_policy(cmd)
            .await
            .map_err(|e| RpcError::new(ErrorCode::InternalError, format!("app: {}", e)))?;
        Ok(to_value(PolicyGetResult {
            mode: result.mode,
            default_data_class: result.default_data_class,
            verify_allowed: result.verify_allowed,
            mutations_allowed: result.mutations_allowed,
            policy_epoch: Self::policy_epoch(services),
            allow_sensitive_to_owner_estate: result.allow_sensitive_to_owner_estate,
            allow_repository_to_admin_managed: result.allow_repository_to_admin_managed,
        }))
    }

    pub(in crate::daemon) async fn policy_set(&mut self, params: Value) -> Result<Value, RpcError> {
        if !rpc_control_plane_mutations_allowed() {
            return Err(RpcError::new(
                ErrorCode::InvalidRequest,
                "policy/set is disabled when LOKAI_STRICT_RPC=1 — use tetonic CLI for control-plane changes",
            ));
        }
        let p: PolicySetParams = parse(params)?;
        let services = self.services()?;

        let cmd = tetonic_app::commands::SetPolicyCommand {
            mode: p.mode,
            verify_allowed: p.verify_allowed,
            mutations_allowed: p.mutations_allowed,
            allow_sensitive_to_owner_estate: p.allow_sensitive_to_owner_estate,
            allow_repository_to_admin_managed: p.allow_repository_to_admin_managed,
        };

        services
            .app
            .policies
            .set_policy(cmd)
            .await
            .map_err(|e| RpcError::new(ErrorCode::InternalError, format!("app: {}", e)))?;

        Self::bump_policy_epoch(services);
        tracing::info!("workspace policy updated via app service");

        let cur = services
            .app
            .policies
            .get_policy(tetonic_app::commands::GetPolicyCommand {
                workspace_root: services.workspace_root.clone(),
            })
            .await
            .map_err(|e| RpcError::new(ErrorCode::InternalError, format!("app: {}", e)))?;

        Ok(to_value(PolicySetResult {
            ok: true,
            mode: cur.mode,
            verify_allowed: cur.verify_allowed,
            mutations_allowed: cur.mutations_allowed,
            policy_epoch: Self::policy_epoch(services),
            allow_sensitive_to_owner_estate: cur.allow_sensitive_to_owner_estate,
            allow_repository_to_admin_managed: cur.allow_repository_to_admin_managed,
        }))
    }

    // ---- estate/status (D3) ------------------------------------------------

    pub(in crate::daemon) fn estate_status(&self) -> Result<Value, RpcError> {
        let services = self.services()?;
        let cmd = tetonic_app::commands::GetEstateStatusCommand {
            fabric_pooled: services.fabric_pooled,
        };
        let result = services
            .app
            .estate
            .get_estate_status(cmd)
            .map_err(|e| RpcError::new(ErrorCode::InternalError, format!("app: {e}")))?;
        Ok(to_value(EstateStatusResult {
            policy_mode: result.policy_mode,
            workers_enrolled: result.workers_enrolled,
            fabric_pooled: result.fabric_pooled,
        }))
    }
}

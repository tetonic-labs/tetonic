//! One production harness: broker, tools, audit, consent and usage accounting.
use super::*;
use preparation::RegisteredHarnessPlan;

pub(super) struct RegisteredHarness {
    pub prepared: super::super::activation::PreparedRegisteredJob,
    pub agent: tetonic_core::Agent,
    pub history: String,
    pub store: tetonic_memory::SharedStore,
    pub restore: Option<tetonic_run::managed::ActivationReceipt>,
}

impl crate::Application {
    pub(super) async fn assemble_registered_harness(
        &self,
        credential: &str,
        plan: RegisteredHarnessPlan,
    ) -> Result<RegisteredHarness, AppError> {
        let RegisteredHarnessPlan {
            mut prepared,
            settings,
            workspace,
            root,
            data_class,
            work,
            approved_environment,
            store: kind_store,
            restore,
        } = plan;
        let restoring = restore.is_some();
        if prepared
            .command
            .job_spec
            .capability_bindings
            .iter()
            .any(|id| id.starts_with("mcp_"))
        {
            let registry = settings
                .mcp
                .clone()
                .ok_or_else(|| AppError::PolicyDenied("MCP library unavailable".into()))?;
            if !registry.authorized(
                &prepared.authorization.scope,
                &prepared.command.job_spec.capability_bindings,
            ) {
                return Err(AppError::PolicyDenied(
                    "MCP access unavailable or revoked".into(),
                ));
            }
            prepared.authorization.authority = Arc::new(crate::mcp::McpAuthority {
                inner: prepared.authorization.authority.clone(),
                registry,
            });
        }
        if prepared
            .command
            .job_spec
            .capability_bindings
            .iter()
            .any(|id| id.starts_with("skill_"))
        {
            let library = settings
                .skills
                .clone()
                .ok_or_else(|| AppError::PolicyDenied("Skill library unavailable".into()))?;
            let selected = prepared
                .command
                .job_spec
                .capability_bindings
                .iter()
                .cloned()
                .collect();
            if !library.authorized(&prepared.authorization.scope, &selected) {
                return Err(AppError::PolicyDenied(
                    "Skill access was revoked or belongs to another workspace".into(),
                ));
            }
            prepared.authorization.authority = Arc::new(crate::skills::SkillAuthority {
                inner: prepared.authorization.authority.clone(),
                library,
            });
        }
        if let Some(human) = settings
            .plan_dispatch
            .as_ref()
            .and_then(|d| d.human.as_ref())
            .filter(|h| h.durable_wait_seconds.is_some())
        {
            let stop_binding = human.prepared_stop_binding.clone().ok_or_else(|| {
                AppError::PolicyDenied("Missing human wait authority binding.".into())
            })?;
            prepared.authorization.authority = Arc::new(super::wait_authority::WaitAuthority {
                inner: prepared.authorization.authority.clone(),
                store: kind_store.clone(),
                org: human.org.clone(),
                team: human.team.clone(),
                work: human.work.clone(),
                stop_binding,
            });
        }
        let provider = self
            .turn
            .registered_provider()
            .filter(|provider| provider.has_secret_scanner())
            .ok_or(AppError::InferenceUnavailable)?;
        let provider = match settings.hosted {
            Some(hosted) => Arc::new(provider.for_hosted(hosted.provider)),
            None => provider,
        };
        let runtime = &self.turn.runtime;
        let history = prepared
            .activation
            .as_ref()
            .ok_or_else(|| AppError::InvalidRequest("registered activation missing".into()))?
            .audit_session_id
            .clone();
        let mut allowed: std::collections::HashSet<_> = prepared
            .command
            .job_spec
            .capability_bindings
            .iter()
            .cloned()
            .collect();
        allowed.insert("finish".into());
        let mut tools = match workspace {
            Some(workspace) => tetonic_tools::Tools::new(workspace, allowed.contains("run_shell")),
            None => tetonic_tools::Tools::without_repository()
                .map_err(|_| AppError::WorkspaceUnavailable)?,
        }
        .with_enforcement_level(tetonic_tools::EnforcementLevel::Sandboxed)
        .with_capability_consumer(runtime.capability_store().clone())
        .with_allowed_tools(allowed);
        if let Some(store) = &self.turn.store {
            if let Ok(path) = store.read_sync(|db| db.path().to_path_buf()) {
                tools = tools.protect_store_file(path);
            }
        }
        if prepared
            .command
            .job_spec
            .capability_bindings
            .iter()
            .any(|tool| tool == "recall")
        {
            tools = prepared
                .contexts
                .bind_recall(
                    credential,
                    prepared.authorization.scope.information_context_id.clone(),
                    tools,
                )
                .await
                .map_err(resource_error)?;
        }
        let store = self
            .run_manager
            .managed()
            .store()
            .cloned()
            .ok_or_else(|| resource_error(ResourceError::StorageRequired))?;
        let scope = prepared.authorization.scope.clone();
        let audit_session = history.clone();
        store
            .write(move |db| {
                if restoring {
                    return db.require_execution_audit_history(
                        &scope.information_context_id,
                        &audit_session,
                    );
                }
                db.preflight_registered_capacity(
                    &scope.organization_id,
                    &scope.principal_id,
                    &scope.information_context_id,
                )?;
                db.create_execution_audit_history(
                    &scope.principal_id,
                    &scope.information_context_id,
                    &audit_session,
                )
            })
            .await
            .map_err(|_| resource_error(ResourceError::Storage))?
            .map_err(capacity_app_error)?;
        let failed = Arc::new(AtomicBool::new(false));
        let audit = crate::store_audit::scoped_execution_audit(
            store,
            prepared.authorization.scope.information_context_id.clone(),
            history.clone(),
            prepared.command.identity.id.0.clone(),
            failed.clone(),
        );
        prepared.authorization.authority = Arc::new(AuditedAuthority {
            inner: prepared.authorization.authority.clone(),
            failed,
        });
        let process_broker = Arc::new(tetonic_broker::BrokerGatedProcessBroker::new(
            provider.broker().clone(),
            Arc::new(tools.executor().clone()),
        ));
        let approval = super::super::shell_approval::for_work(
            kind_store.clone(),
            prepared.authorization.scope.clone(),
            work.clone(),
            root.clone(),
            approved_environment,
        );
        let provider: Arc<dyn tetonic_inference::InferenceProvider> = match work {
            Some((team, work)) => Arc::new(super::super::work_usage::WorkUsageProvider {
                inner: provider,
                store: kind_store.clone(),
                actor: prepared.authorization.scope.principal_id.clone(),
                org: prepared.authorization.scope.organization_id.clone(),
                team,
                work,
                own_limit: settings.reported_token_ceiling,
            }),
            None => provider,
        };
        prepared.finalization = Some(tetonic_run::FinalizationPolicy {
            effect_driver: Some(Arc::new(crate::turn_execution::ToolsFinalizationDriver(
                Arc::new(tools.clone()),
            ))),
            verify_cmd: None,
        });
        let abort_tools = tools.clone();
        let config = tetonic_core::AgentConfig {
            model: settings.model,
            num_ctx: settings.num_ctx,
            max_steps: prepared.command.invocation.max_steps,
            workspace_root: root.clone(),
            process_working_directory: root,
            agent_id: prepared.command.identity.id.0.clone(),
            session_id: Some(history.clone()),
            information_context_id: Some(
                prepared.authorization.scope.information_context_id.clone(),
            ),
            data_class,
            reported_token_ceiling: settings.reported_token_ceiling,
            response_schema: settings.response_schema,
            ..Default::default()
        };
        // No global briefing, project digest, legacy conversation or coding
        // compiler is attached. Explicit scoped recall is available when granted.
        let host = super::super::plan_dispatch::RegisteredToolHost {
            tools,
            dispatch: settings.plan_dispatch.clone(),
        };
        let selected: std::collections::HashSet<String> = prepared
            .command
            .job_spec
            .capability_bindings
            .iter()
            .cloned()
            .collect();
        let host: Box<dyn tetonic_domain::ToolHost> = if let Some(registry) = settings.mcp {
            Box::new(crate::mcp::McpToolHost {
                inner: Box::new(host),
                registry,
                selected: selected.clone(),
                consumer: runtime.capability_store().clone(),
                runtime: tokio::runtime::Handle::current(),
            })
        } else {
            Box::new(host)
        };
        let agent = tetonic_core::Agent::new(
            provider,
            crate::skills::SkillToolHost {
                inner: host,
                library: settings.skills,
                selected,
            },
            config,
        );
        let agent = agent.with_abort_staged(Arc::new(move || {
            let _ = abort_tools.abort_staged_if_any();
        }));
        let (post_edit_snapshot, resolve_under_root, capture_workspace_version) =
            crate::turn_execution::composition_capability_hooks();
        let agent = runtime
            .assemble_agent(
                tetonic_runtime::AssemblyMode::Session,
                tetonic_runtime::AgentAssemblyParts {
                    agent,
                    audit,
                    approval: tetonic_runtime::ProductionApproval::host(approval.clone()),
                    spawn: settings
                        .plan_dispatch
                        .as_ref()
                        .map(|dispatch| dispatch.hook(self.run_manager.managed().clone())),
                    process_broker: Some(process_broker),
                    context_compiler: None,
                    post_edit_snapshot,
                    resolve_under_root,
                    capture_workspace_version,
                },
            )
            .map_err(|_| AppError::InvalidRequest("registered runtime assembly failed".into()))?
            .with_action_broker(runtime.action_broker().with_approval(approval));
        // Only an explicitly configured root host or matching reconstruction opts in.
        // The managed owner verifies the sealed checkpoint against this exact harness.
        let agent = if restoring
            || settings
                .plan_dispatch
                .as_ref()
                .and_then(|d| d.human.as_ref())
                .is_some_and(|h| h.durable_wait_seconds.is_some())
        {
            agent.with_durable_waits()
        } else {
            agent
        };
        Ok(RegisteredHarness {
            prepared,
            agent,
            history,
            store: kind_store,
            restore,
        })
    }
}

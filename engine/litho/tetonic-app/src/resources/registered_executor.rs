//! Production assembly for registered workspace jobs. Reuses runtime, tools,
//! brokered inference and the product audit writer; no coding session is created.
use super::activation::resource_error;
use super::*;
use crate::errors::AppError;
use std::sync::atomic::{AtomicBool, Ordering};

/// Operator-selected settings, not fields accepted from an employee request.
/// The workspace and tool ceiling must be authorized by the host. Stored job
/// grants constrain requested tool names; they do not grant arbitrary host paths.
pub struct RegisteredExecutionSettings {
    /// Host ceiling, including preparation and managed execution/finalization.
    /// Persisted as a Unix-seconds deadline; resolution can shorten this by <1s.
    pub max_elapsed_seconds: u64,
    pub workspace_root: std::path::PathBuf,
    pub model: String,
    pub num_ctx: usize,
    pub data_class: tetonic_domain::DataClass,
    pub allowed_tools: std::collections::HashSet<String>,
    pub limits: HarnessPreparationLimits,
}

pub struct RegisteredAgentSubmission {
    pub run_id: tetonic_domain::RunId,
    pub task_id: tetonic_domain::TaskId,
    /// Read with ContextService::transcript using current context authorization.
    pub audit_session_id: String,
    /// Present only for the winning launch. Retries inspect the durable run;
    /// they neither attach a second completion owner nor resume interrupted work.
    pub execution: Option<RegisteredAgentExecution>,
}

pub struct RegisteredAgentExecution {
    pub attempt_id: tetonic_domain::AttemptId,
    pub completion: tokio::sync::oneshot::Receiver<tetonic_run::StartIdentityJobResult>,
}

impl From<tetonic_run::managed::ActivationReceipt> for RegisteredAgentSubmission {
    fn from(receipt: tetonic_run::managed::ActivationReceipt) -> Self {
        Self {
            run_id: receipt.run_id,
            task_id: receipt.task_id,
            audit_session_id: receipt.audit_session_id,
            execution: None,
        }
    }
}

struct AuditedAuthority {
    inner: Arc<dyn tetonic_run::managed::ExecutionAuthority>,
    failed: Arc<AtomicBool>,
}
#[async_trait]
impl tetonic_run::managed::ExecutionAuthority for AuditedAuthority {
    async fn authorize(
        &self,
        scope: &tetonic_domain::ExecutionScope,
        identity: &tetonic_domain::AgentIdentity,
        job: &tetonic_domain::AgentJobSpec,
    ) -> Result<(), ()> {
        if self.failed.load(Ordering::SeqCst) {
            return Err(());
        }
        self.inner.authorize(scope, identity, job).await?;
        if self.failed.load(Ordering::SeqCst) {
            Err(())
        } else {
            Ok(())
        }
    }
}

impl crate::Application {
    /// Trusted host launch using the installed compute plane and runtime. No
    /// caller-supplied Agent/provider/audit can bypass this assembly. Requires a
    /// LocalSet, a stored job grant and a brokered compute plane installed through
    /// install_compute_services. Interactive actions deny until scoped approvals
    /// are integrated. This is not yet an employee transport or cumulative budget.
    pub async fn submit_registered_job(
        &self,
        credential: &str,
        verifier: Arc<dyn CredentialVerifier>,
        request: RegisteredAgentJob,
        settings: RegisteredExecutionSettings,
    ) -> Result<RegisteredAgentSubmission, AppError> {
        if settings.model.is_empty()
            || settings.max_elapsed_seconds == 0
            || settings.max_elapsed_seconds > 86_400
            || settings
                .model
                .chars()
                .any(|c| c.is_whitespace() || c.is_control())
            || settings.num_ctx <= 1024
            || settings.num_ctx > u32::MAX as usize
        {
            return Err(AppError::InvalidRequest(
                "invalid registered execution settings".into(),
            ));
        }
        let deadline = u64::try_from(chrono::Utc::now().timestamp())
            .ok()
            .and_then(|now| now.checked_add(settings.max_elapsed_seconds))
            .ok_or_else(|| AppError::InvalidRequest("invalid execution deadline".into()))?;
        let request_id = request.request_id.clone();
        let preparation_limits = (settings.limits.max_steps, settings.limits.max_input_bytes);
        let mut prepared = self
            .run_manager
            .prepare_registered_job(credential, verifier, request, settings.limits)
            .await?;
        prepared.deadline = Some(deadline);
        if prepared
            .command
            .job_spec
            .capability_bindings
            .iter()
            .any(|tool| tool != "finish" && !settings.allowed_tools.contains(tool))
        {
            return Err(AppError::PolicyDenied(
                "requested tools exceed host grant".into(),
            ));
        }
        let workspace = tetonic_tools::Workspace::new(&settings.workspace_root)
            .map_err(|_| AppError::WorkspaceUnavailable)?;
        let root = workspace.root().to_path_buf();
        let root_key = root.to_str().ok_or(AppError::WorkspaceUnavailable)?;
        let mut ceiling: Vec<_> = settings.allowed_tools.iter().collect();
        ceiling.sort();
        // The fingerprint covers effective host settings as well as the exact
        // granted job. Absolute time and a new audit UUID are not request inputs.
        let request_bytes = serde_json::to_vec(&serde_json::json!({
            "version": 1, "scope": prepared.authorization.scope,
            "grant_id": prepared.authorization.grant_id, "job": prepared.command.job_spec,
            "workspace": root_key, "model": settings.model, "num_ctx": settings.num_ctx,
            "data_class": settings.data_class, "tools": ceiling,
            "preparation_limits": preparation_limits, "max_elapsed_seconds": settings.max_elapsed_seconds,
        })).map_err(|_| AppError::InvalidRequest("invalid activation settings".into()))?;
        use sha2::Digest;
        let activation = tetonic_domain::ActivationBinding {
            request_id,
            request_digest: format!("sha256:{:x}", sha2::Sha256::digest(request_bytes)),
            audit_session_id: format!("execution-audit-{}", uuid::Uuid::new_v4()),
        };
        if let Some(receipt) = self
            .run_manager
            .managed()
            .lookup_activation(
                &prepared.authorization,
                &activation,
                &prepared.command.identity,
                &prepared.command.job_spec,
                None,
            )
            .await?
        {
            return Ok(receipt.into());
        }
        let provider = self
            .turn
            .registered_provider()
            .filter(|provider| provider.has_secret_scanner())
            .ok_or(AppError::InferenceUnavailable)?;
        let runtime = &self.turn.runtime;
        let history = activation.audit_session_id.clone();
        prepared.activation = Some(activation);
        let mut allowed: std::collections::HashSet<_> = prepared
            .command
            .job_spec
            .capability_bindings
            .iter()
            .cloned()
            .collect();
        allowed.insert("finish".into());
        let mut tools = tetonic_tools::Tools::new(workspace, false)
            .with_enforcement_level(tetonic_tools::EnforcementLevel::Sandboxed)
            .with_capability_consumer(runtime.capability_store().clone())
            .with_allowed_tools(allowed);
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
                db.create_execution_audit_history(
                    &scope.principal_id,
                    &scope.information_context_id,
                    &audit_session,
                )
            })
            .await
            .map_err(|_| resource_error(ResourceError::Storage))?
            .map_err(|error| resource_error(error.into()))?;
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
            workspace_root: Some(root.clone()),
            process_working_directory: Some(root),
            agent_id: prepared.command.identity.id.0.clone(),
            session_id: Some(history.clone()),
            data_class: settings.data_class,
            ..Default::default()
        };
        // No global briefing, project digest, legacy conversation or coding
        // compiler is attached. Explicit scoped recall is available when granted.
        let agent = tetonic_core::Agent::new(provider, tools, config).with_abort_staged(Arc::new(
            move || {
                let _ = abort_tools.abort_staged_if_any();
            },
        ));
        let (post_edit_snapshot, resolve_under_root, capture_workspace_version) =
            crate::turn_execution::composition_capability_hooks();
        let agent = runtime
            .assemble_agent(
                tetonic_runtime::AssemblyMode::Session,
                tetonic_runtime::AgentAssemblyParts {
                    agent,
                    audit,
                    approval: tetonic_runtime::ProductionApproval::host(Arc::new(|_| {
                        Box::pin(async { false })
                    })),
                    spawn: None,
                    process_broker: Some(process_broker),
                    context_compiler: None,
                    post_edit_snapshot,
                    resolve_under_root,
                    capture_workspace_version,
                },
            )
            .map_err(|_| AppError::InvalidRequest("registered runtime assembly failed".into()))?;
        let submission = self
            .run_manager
            .submit_prepared_registered_job(prepared, agent)
            .await?;
        match submission {
            tetonic_run::managed::ManagedSubmission::Started {
                binding,
                completion,
            } => Ok(RegisteredAgentSubmission {
                run_id: binding.run_id,
                task_id: binding.task_id,
                audit_session_id: history,
                execution: Some(RegisteredAgentExecution {
                    attempt_id: binding.attempt_id,
                    completion,
                }),
            }),
            tetonic_run::managed::ManagedSubmission::Existing(receipt) => Ok(receipt.into()),
        }
    }
}

#[cfg(test)]
#[path = "registered_executor_tests.rs"]
mod tests;

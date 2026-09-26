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
    /// Provider-reported token ceiling for this job. Unreported usage is not charged.
    pub reported_token_ceiling: Option<u64>,
    /// Absent for a noncoding job. Repository tools then fail before any path is opened.
    pub workspace_root: Option<std::path::PathBuf>,
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

    async fn revoked_during_execution(
        &self,
        scope: &tetonic_domain::ExecutionScope,
        identity: &tetonic_domain::AgentIdentity,
        job: &tetonic_domain::AgentJobSpec,
    ) -> bool {
        self.failed.load(Ordering::SeqCst)
            || self
                .inner
                .revoked_during_execution(scope, identity, job)
                .await
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
        if let Some(tool) = prepared
            .command
            .job_spec
            .capability_bindings
            .iter()
            .find(|tool| !supported_registered_tool(tool))
        {
            return Err(AppError::PolicyDenied(format!(
                "{tool} is not a supported registered isolation profile"
            )));
        }
        let repository_requested = settings
            .allowed_tools
            .iter()
            .chain(prepared.command.job_spec.capability_bindings.iter())
            .any(|tool| tool != "finish" && tool != "recall");
        if repository_requested && settings.workspace_root.is_none() {
            return Err(AppError::WorkspaceUnavailable);
        }
        let workspace = match &settings.workspace_root {
            Some(path) => Some(
                tetonic_tools::Workspace::new(path).map_err(|_| AppError::WorkspaceUnavailable)?,
            ),
            None => None,
        };
        let root = workspace.as_ref().map(|workspace| workspace.root().to_path_buf());
        let root_key = root
            .as_ref()
            .map(|path| {
                path.to_str()
                    .map(str::to_string)
                    .ok_or(AppError::WorkspaceUnavailable)
            })
            .transpose()?;
        let mut ceiling: Vec<_> = settings.allowed_tools.iter().collect();
        ceiling.sort();
        let context_id = prepared.authorization.scope.information_context_id.clone();
        let kind_store = self
            .run_manager
            .managed()
            .store()
            .cloned()
            .ok_or_else(|| resource_error(ResourceError::StorageRequired))?;
        let context_kind = kind_store
            .read(move |db| db.information_context_kind(&context_id))
            .await
            .map_err(|_| resource_error(ResourceError::Storage))?
            .map_err(|_| resource_error(ResourceError::Storage))?;
        // Governed private and team prompts stay on this machine. The operator's
        // requested class remains part of the fingerprint so two host classes
        // do not alias to the same activation.
        let data_class = floor_governed_context(context_kind.as_deref(), settings.data_class);
        // The fingerprint covers effective host settings as well as the exact
        // granted job. Absolute time and a new audit UUID are not request inputs.
        let request_bytes = serde_json::to_vec(&serde_json::json!({
            "version": 1, "scope": prepared.authorization.scope,
            "grant_id": prepared.authorization.grant_id, "job": prepared.command.job_spec,
            "workspace": root_key, "model": settings.model, "num_ctx": settings.num_ctx,
            "requested_data_class": settings.data_class, "data_class": data_class, "tools": ceiling,
            "preparation_limits": preparation_limits, "max_elapsed_seconds": settings.max_elapsed_seconds,
            "reported_token_ceiling": settings.reported_token_ceiling,
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
        let mut tools = match workspace {
            Some(workspace) => tetonic_tools::Tools::new(workspace, false),
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

/// In-process tools and workspace-jailed file tools are supported. Model-requested
/// shells are not, because this path does not start an OS sandbox for them.
fn supported_registered_tool(tool: &str) -> bool {
    matches!(
        tool,
        "finish"
            | "recall"
            | "read_file"
            | "list_dir"
            | "grep"
            | "glob"
            | "outline"
            | "find_definition"
            | "find_mentions"
            | "find_references"
            | "search_code"
            | "edit_file"
            | "write_file"
    )
}

fn floor_governed_context(
    kind: Option<&str>,
    class: tetonic_domain::DataClass,
) -> tetonic_domain::DataClass {
    if kind == Some("private") || kind == Some("team") {
        class.max(tetonic_domain::DataClass::Secret)
    } else {
        class
    }
}

fn capacity_app_error(error: tetonic_memory::StoreError) -> AppError {
    match error {
        tetonic_memory::StoreError::OrganizationCapacityExceeded => {
            AppError::OrganizationCapacityExceeded
        }
        tetonic_memory::StoreError::TeamCapacityExceeded => AppError::TeamCapacityExceeded,
        tetonic_memory::StoreError::PrincipalCapacityExceeded => AppError::PrincipalCapacityExceeded,
        tetonic_memory::StoreError::ExecutionCapacityExceeded => AppError::ExecutionCapacityExceeded,
        other => resource_error(other.into()),
    }
}

#[cfg(test)]
mod isolation_tests {
    use super::supported_registered_tool;

    #[test]
    fn governed_contexts_floor_the_host_data_class_to_secret() {
        use tetonic_domain::DataClass;
        assert_eq!(
            super::floor_governed_context(Some("private"), DataClass::RepositorySource),
            DataClass::Secret
        );
        assert_eq!(
            super::floor_governed_context(Some("team"), DataClass::RepositorySource),
            DataClass::Secret
        );
        assert_eq!(
            super::floor_governed_context(Some("private"), DataClass::Secret),
            DataClass::Secret
        );
        assert_eq!(
            super::floor_governed_context(Some("legacy_local"), DataClass::RepositorySource),
            DataClass::RepositorySource
        );
        assert_eq!(
            super::floor_governed_context(None, DataClass::SensitiveSource),
            DataClass::SensitiveSource
        );
    }

    #[test]
    fn shell_is_outside_the_registered_isolation_matrix() {
        assert!(supported_registered_tool("recall"));
        assert!(supported_registered_tool("read_file"));
        assert!(supported_registered_tool("write_file"));
        assert!(!supported_registered_tool("run_shell"));
    }
}

#[cfg(test)]
#[path = "registered_executor_tests.rs"]
mod tests;

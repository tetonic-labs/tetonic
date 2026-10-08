//! Production assembly for registered workspace jobs. Reuses runtime, tools,
//! brokered inference and the product audit writer; no coding session is created.
pub(super) use admission::resource_error;
mod admission;
mod authority;
mod harness;
use super::*;
use crate::errors::AppError;
pub use admission::RegisteredAgentJob;
pub use harness::{GeneralAgentPreferences, HarnessPreparationLimits, PreparedAgentRevision};
use std::sync::atomic::{AtomicBool, Ordering};
mod assembly;
mod environment;
mod preparation;
#[cfg(test)]
mod reconstruction_tests;
mod wait_authority;

/// Operator-selected settings, not fields accepted from an employee request.
/// The workspace and tool ceiling must be authorized by the host. Stored job
/// grants constrain requested tool names; they do not grant arbitrary host paths.
#[derive(Clone)]
pub struct RegisteredExecutionSettings {
    pub skills: Option<Arc<crate::skills::SkillLibrary>>,
    /// Operator-configured MCP inventory, never supplied by an employee request.
    pub mcp: Option<Arc<crate::mcp::McpRegistry>>,
    pub plan_dispatch: Option<super::plan_dispatch::PlanDispatch>,
    /// Host-only, output-only inference contract; not employee-controlled authority.
    pub response_schema: Option<serde_json::Value>,
    /// Explicit owner-approved hosted route. Not deserializable.
    pub hosted: Option<RegisteredHostedInference>,
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

#[derive(Clone)]
pub struct RegisteredHostedInference {
    pub(crate) provider: Arc<tetonic_inference::hosted::HostedChatProvider>,
    pub(crate) binding: String,
    /// Owner-approved tool data and destination, separate from execution grants.
    pub(crate) tool_disclosure: Option<super::ToolDisclosure>,
}

pub(crate) const HOSTED_READ_TOOLS: &[&str] = &["read_file", "list_dir", "grep", "glob"];

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
    /// install_compute_services. Shell approvals require a bound team work item;
    /// other interactive actions fail closed.
    pub async fn submit_registered_job(
        &self,
        credential: &str,
        verifier: Arc<dyn CredentialVerifier>,
        request: RegisteredAgentJob,
        settings: RegisteredExecutionSettings,
    ) -> Result<RegisteredAgentSubmission, AppError> {
        self.submit_registered_job_for_work(credential, verifier, request, settings, None, None)
            .await
    }

    pub(super) async fn submit_registered_job_for_work(
        &self,
        credential: &str,
        verifier: Arc<dyn CredentialVerifier>,
        request: RegisteredAgentJob,
        settings: RegisteredExecutionSettings,
        work: Option<(String, String)>,
        parent: Option<tetonic_run::managed::DelegationParent>,
    ) -> Result<RegisteredAgentSubmission, AppError> {
        // Composition carries sizeable provider/agent state. Keep this future
        // off the caller's async frame (notably Windows' small thread stacks).
        Box::pin(self.execute_registered_job(
            credential,
            verifier,
            request,
            settings,
            preparation::RegisteredJobContext {
                work,
                parent,
                restore: None,
            },
        ))
        .await
    }

    async fn execute_registered_job(
        &self,
        credential: &str,
        verifier: Arc<dyn CredentialVerifier>,
        request: RegisteredAgentJob,
        settings: RegisteredExecutionSettings,
        context: preparation::RegisteredJobContext,
    ) -> Result<RegisteredAgentSubmission, AppError> {
        let plan = match self
            .prepare_registered_harness(credential, verifier, request, settings, context)
            .await?
        {
            preparation::HarnessPreparation::Existing(receipt) => return Ok(receipt.into()),
            preparation::HarnessPreparation::Ready(plan) => *plan,
        };
        let assembly::RegisteredHarness {
            prepared,
            agent,
            history,
            store: kind_store,
            restore,
        } = self.assemble_registered_harness(credential, plan).await?;
        let submission = match restore {
            Some(receipt) => {
                self.run_manager
                    .execute_prepared_registered_job(prepared, agent, Some(receipt))
                    .await?
            }
            None => {
                self.run_manager
                    .submit_prepared_registered_job(prepared, agent)
                    .await?
            }
        };
        match submission {
            tetonic_run::managed::ManagedSubmission::Started {
                binding,
                completion,
            } => {
                let (send, settled) = tokio::sync::oneshot::channel();
                let attempt = binding.attempt_id.0.clone();
                tokio::task::spawn_local(async move {
                    if let Ok(result) = completion.await {
                        if !matches!(
                            kind_store
                                .write(move |db| db.settle_work_inference(&attempt))
                                .await,
                            Ok(Ok(()))
                        ) {
                            tracing::warn!(
                                "work usage settlement unconfirmed; reservation retained"
                            );
                        }
                        let _ = send.send(result);
                    }
                });
                Ok(RegisteredAgentSubmission {
                    run_id: binding.run_id,
                    task_id: binding.task_id,
                    audit_session_id: history,
                    execution: Some(RegisteredAgentExecution {
                        attempt_id: binding.attempt_id,
                        completion: settled,
                    }),
                })
            }
            tetonic_run::managed::ManagedSubmission::Existing(receipt) => Ok(receipt.into()),
        }
    }
}

/// Tool implementations share the existing capability and process brokers.
/// Shell execution additionally requires a live, exact-command human approval.
fn supported_registered_tool(tool: &str) -> bool {
    matches!(
        tool,
        "finish"
            | "run_shell"
            | "dispatch_assignment"
            | "ask_human"
            | "work_plan"
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
        tetonic_memory::StoreError::PrincipalCapacityExceeded => {
            AppError::PrincipalCapacityExceeded
        }
        tetonic_memory::StoreError::ExecutionCapacityExceeded => {
            AppError::ExecutionCapacityExceeded
        }
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
    fn shell_uses_the_registered_process_broker() {
        assert!(supported_registered_tool("recall"));
        assert!(supported_registered_tool("read_file"));
        assert!(supported_registered_tool("write_file"));
        assert!(supported_registered_tool("run_shell"));
    }
}

#[cfg(test)]
mod execution_tests;

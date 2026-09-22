//! Async process ports retain ownership in the actual blocking worker.
use super::*;
use async_trait::async_trait;
use lokai_domain::sinks::{ProcessBrokerError, ProcessSink};
use lokai_domain::work_scope::{CancellationSignal, WorkScope};

#[async_trait]
impl ProcessSink for ProcessExecutor {
    async fn run_process(
        &self,
        authorized: &AuthorizedAction,
        scope: &WorkScope,
    ) -> ExecutionOutcome {
        let execution_id = ExecutionId::new(format!("exec_{}", authorized.action.action_id));
        let Some(lease) = scope.try_enter() else {
            return ExecutionOutcome::Failed {
                execution_id,
                reason: "process scope canceled".into(),
            };
        };
        let signal = scope.cancellation_signal();
        let authorized = authorized.clone();
        let executor = self.clone();
        match tokio::task::spawn_blocking(move || {
            let _lease = lease;
            if signal.is_canceled() {
                return ExecutionOutcome::Failed {
                    execution_id: ExecutionId::new(format!("exec_{}", authorized.action.action_id)),
                    reason: "process scope canceled".into(),
                };
            }
            executor.run_process_sync_cancellable(&authorized, Some(&signal))
        })
        .await
        {
            Ok(outcome) => outcome,
            Err(_) => ExecutionOutcome::Failed {
                execution_id,
                reason: "process sink task panicked".into(),
            },
        }
    }
}

#[async_trait]
impl lokai_domain::sinks::ProcessBroker for ProcessExecutor {
    async fn execute(
        &self,
        request: lokai_domain::sinks::AuthorizedProcessRequest,
    ) -> Result<lokai_domain::sinks::ManagedProcessResult, ProcessBrokerError> {
        let lease = request
            .work_scope
            .try_enter()
            .ok_or_else(|| ProcessBrokerError::ExecutionFailed("process scope canceled".into()))?;
        let signal = request.work_scope.cancellation_signal();
        let executor = self.clone();
        tokio::task::spawn_blocking(move || {
            let _lease = lease;
            if signal.is_canceled() {
                return Err(ProcessBrokerError::ExecutionFailed(
                    "process scope canceled".into(),
                ));
            }
            executor.execute_broker_sync(&request.authorized_action, signal)
        })
        .await
        .map_err(|_| ProcessBrokerError::ExecutionFailed("process broker task panicked".into()))?
    }

    async fn start_service(
        &self,
        _request: lokai_domain::sinks::AuthorizedServiceRequest,
    ) -> Result<
        Box<dyn lokai_domain::sinks::ManagedProcessHandle>,
        lokai_domain::sinks::ProcessBrokerError,
    > {
        Err(lokai_domain::sinks::ProcessBrokerError::ExecutionFailed(
            "ProcessBroker::start_service is retired; production LSP uses SandboxLspLauncher"
                .into(),
        ))
    }
}

impl ProcessExecutor {
    fn execute_broker_sync(
        &self,
        authorized: &AuthorizedAction,
        signal: CancellationSignal,
    ) -> Result<lokai_domain::sinks::ManagedProcessResult, ProcessBrokerError> {
        self.validate_capability(authorized)?;
        if !self.uses_os_sandbox() {
            return Err(lokai_domain::sinks::ProcessBrokerError::ExecutionFailed(
                "ProcessBroker::execute requires Sandboxed ProcessExecutor; Constrained is refused"
                    .into(),
            ));
        }
        let cwd = self.effective_cwd(authorized);
        let req =
            sandbox_request_from_authorized(authorized, &cwd, crate::exec::DEFAULT_VERIFY_TIMEOUT)
                .map_err(lokai_domain::sinks::ProcessBrokerError::ExecutionFailed)?;
        let execution_id = ExecutionId::new(format!("exec_{}", new_exec_suffix()));
        let backend = self.sandbox.clone();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| lokai_domain::sinks::ProcessBrokerError::ExecutionFailed(e.to_string()))?;
        let result = rt.block_on(async move { backend.execute_cancellable(req, signal).await });
        match result {
            Ok(SandboxedProcess::Completed(r)) => {
                let audit = format_sandbox_audit(&r.report);
                tracing::info!("lokai_sandbox: {audit}");
                let body = format!(
                    "{}\nexit code: {}\nstdout:\n{}\nstderr:\n{}",
                    r.report.format_user_visible(),
                    r.exit_code.unwrap_or(-1),
                    redact_process_io(&r.stdout),
                    redact_process_io(&r.stderr)
                );
                Ok(lokai_domain::sinks::ManagedProcessResult {
                    success: r.success,
                    output: prepend_sandbox_audit(exec::truncate_output(&body), Some(audit)),
                    execution_id,
                })
            }
            Ok(SandboxedProcess::Service(_)) => {
                Err(lokai_domain::sinks::ProcessBrokerError::ExecutionFailed(
                    "expected one-shot sandbox".into(),
                ))
            }
            Err(crate::SandboxError::Timeout) => Err(
                lokai_domain::sinks::ProcessBrokerError::ExecutionFailed(format!(
                    "command timed out after {}s",
                    crate::exec::DEFAULT_VERIFY_TIMEOUT.as_secs()
                )),
            ),
            Err(e) => Err(lokai_domain::sinks::ProcessBrokerError::ExecutionFailed(
                e.to_string(),
            )),
        }
    }
}

//! Product event and approval projections of manager-owned lifetimes.
use crate::events::*;
use std::sync::{Arc, Mutex};
use tetonic_memory::RecoverMutex;
use tetonic_run::{ManagedBinding, ManagedRunHooks, StartIdentityJobResult};
pub(crate) struct ProductRunHooks {
    pub bindings: Mutex<std::collections::HashMap<tetonic_domain::AttemptId, ManagedBinding>>,
    pub events: Arc<dyn ApplicationEventSink>,
    pub scanner: tetonic_secrets::ScannerEngine,
    pub approvals: Arc<Mutex<Option<Arc<dyn crate::approval::ApprovalService>>>>,
    pub runtime: Arc<Mutex<Option<Arc<tetonic_runtime::EngineRuntime>>>>,
}
impl ManagedRunHooks for ProductRunHooks {
    fn started(&self, binding: &ManagedBinding) {
        // The legacy application sink has no subscriber authorization. Scoped
        // runs must use a scoped delivery adapter before projecting content here.
        if binding.execution_scope.is_some() {
            return;
        }
        self.bindings
            .lock_recover()
            .insert(binding.attempt_id.clone(), binding.clone());
        if binding.execution_scope.is_none() && binding.session_id.is_none() {
            emit(
                &self.events,
                ApplicationEvent::run_status(
                    binding.run_id.0.clone(),
                    "started".into(),
                    None,
                    None,
                    &envelope(binding),
                ),
            );
        }
    }
    fn step(&self, binding: &ManagedBinding, step: &tetonic_core::Step) {
        if binding.execution_scope.is_none() && binding.session_id.is_none() {
            crate::turn_execution::step_to_events(
                &self.events,
                "",
                &binding.job_spec.identity_id.0,
                step.clone(),
                Some(&self.scanner),
                None,
                Some(&envelope(binding)),
            );
        }
    }
    fn terminal(&self, result: &StartIdentityJobResult) {
        let binding = self.bindings.lock_recover().remove(&result.attempt_id);
        if let Some(binding) =
            binding.filter(|b| b.execution_scope.is_none() && b.session_id.is_none())
        {
            emit(
                &self.events,
                ApplicationEvent::turn_completed(
                    String::new(),
                    terminal_status(&result.outcome),
                    outcome_error(&result.outcome),
                    &envelope(&binding),
                ),
            );
        }
    }

    fn fail_approval_waits(&self, attempt: &tetonic_domain::AttemptId) {
        let approvals = self.approvals.lock_recover().clone();
        if let Some(approvals) = approvals {
            approvals.fail_attempt_waits(&attempt.0);
        }
        let runtime = self.runtime.lock_recover().clone();
        if let Some(runtime) = runtime {
            runtime.action_broker().unregister_attempt_approval(attempt);
        }
    }
}
fn envelope(binding: &ManagedBinding) -> EventEnvelope {
    EventEnvelope {
        run_id: Some(binding.run_id.0.clone()),
        task_id: Some(binding.task_id.0.clone()),
        attempt_id: Some(binding.attempt_id.0.clone()),
        identity_id: Some(binding.job_spec.identity_id.0.clone()),
    }
}

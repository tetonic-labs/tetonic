//! Turn execution wired through RunSupervisor (M3-1 / M3-2).

use crate::{commands::*, errors::AppError, events::*, turn_execution::TurnExecutionHost};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tetonic_domain::{
    AgentIdentity, AgentJobSpec, AttemptId, CandidateOutcome, CompletionKind, ReplayGap,
    RunEventEnvelope, RunId, RunSnapshot, SessionId, TaskId,
};
use tetonic_memory::RecoverMutex;
use tetonic_orchestrator::ChildJob;
use tetonic_run::{job_input_digest, RunSupervisor};
use tokio::sync::oneshot;

#[path = "attempt_completion.rs"]
mod attempt_completion;
#[path = "identity_job.rs"]
mod identity_job;
#[path = "turn_finalization.rs"]
mod turn_finalization;

pub use tetonic_run::{ExecutionPolicy, FinalizationEffectDriver, FinalizationPolicy};

pub type IdentitySupplier = Arc<dyn Fn(&str) -> (AgentIdentity, AgentJobSpec) + Send + Sync>;

#[async_trait::async_trait]
pub trait RunService: Send + Sync {
    fn execution_service(&self) -> Arc<dyn RunService>;
    async fn execute_attempt(
        &self,
        attempt_id: AttemptId,
        agent: &mut tetonic_core::Agent,
        conversation: &mut tetonic_core::Conversation,
        invocation: tetonic_domain::AgentInvocation,
        on_step: &mut (dyn FnMut(tetonic_core::Step) + Send),
    ) -> CandidateOutcome;
    fn event_sink(&self) -> Arc<dyn ApplicationEventSink>;
    async fn create_run(&self, cmd: CreateRunCommand) -> Result<RunId, AppError>;
    async fn inspect_run(&self, cmd: InspectRunCommand) -> Result<RunSnapshot, AppError>;
    async fn resume_events(
        &self,
        cmd: ResumeRunEventsCommand,
    ) -> Result<Result<Vec<RunEventEnvelope>, ReplayGap>, AppError>;
    async fn cancel_run(&self, cmd: CancelByRunCommand) -> Result<(), AppError>;
    async fn plan_turn(&self, cmd: &RunTurnCommand) -> Result<RunTurnPlan, AppError>;
    async fn complete_turn(
        &self,
        cmd: &CompleteTurnCommand,
        outcome: Option<&CandidateOutcome>,
        policy: Option<FinalizationPolicy>,
    ) -> Result<CandidateOutcome, AppError>;
    async fn report_run_status(&self, cmd: &ReportRunStatusCommand) -> Result<(), AppError>;
    async fn register_spawn_task(
        &self,
        session_id: &str,
        agent_id: &str,
        role: &str,
        job_input: &str,
    ) -> Result<AttemptId, AppError>;
    fn child_job(&self, session_id: &str) -> Arc<dyn ChildJob>;
    fn active_parent_attempt(&self, session_id: &str) -> Option<AttemptId>;
    fn active_run_for_session(&self, session_id: &str) -> Option<RunId>;
    async fn heartbeat_turn(&self, attempt_id: &str) -> Result<(), AppError>;
    fn drop_active_for_run(&self, run_id: &RunId);
    fn run_id_for_attempt(&self, attempt_id: &AttemptId) -> Option<RunId>;
    fn task_id_for_attempt(&self, _attempt_id: &AttemptId) -> Option<TaskId> {
        None
    }
    fn active_job_spec(&self, _attempt_id: &AttemptId) -> Option<AgentJobSpec> {
        None
    }
    fn active_input_digest(&self, _attempt_id: &AttemptId) -> Option<String> {
        None
    }
    async fn start_identity_job(
        &self,
        cmd: StartIdentityJobCommand,
        agent: &mut tetonic_core::Agent,
    ) -> Result<StartIdentityJobResult, AppError>;
    fn arm_attempt_join(
        &self,
        _attempt_id: &AttemptId,
    ) -> oneshot::Receiver<StartIdentityJobResult> {
        let (_tx, rx) = oneshot::channel();
        rx
    }
    async fn submit_identity_job(
        &self,
        _cmd: StartIdentityJobCommand,
        _agent: tetonic_core::Agent,
    ) -> Result<(AttemptId, oneshot::Receiver<StartIdentityJobResult>), AppError> {
        Err(AppError::InvalidRequest(
            "submit_identity_job not implemented".into(),
        ))
    }
    fn is_canceled_attempt(&self, _attempt_id: &AttemptId) -> bool {
        false
    }
}

#[derive(Clone)]
pub struct RunTurnPlan {
    pub verify_gated: bool,
    pub llm_router: bool,
    pub run_id: RunId,
    pub task_id: TaskId,
    pub attempt_id: AttemptId,
    pub job_spec: AgentJobSpec,
}

pub(crate) type ActiveTurnRun = tetonic_run::ManagedBinding;

#[derive(Clone)]
pub struct DefaultRunService {
    store: Option<tetonic_memory::SharedStore>,
    policy: Arc<tetonic_policy::PolicyEngine>,
    events: Arc<dyn ApplicationEventSink>,
    live: Arc<crate::session_live::SessionLiveStore>,
    artifacts: Arc<dyn tetonic_domain::artifact::ArtifactStore>,
    identity_supplier: IdentitySupplier,
    session_dispatches: Arc<Mutex<HashMap<String, tetonic_run::DispatchId>>>,
    session_runs: Arc<Mutex<HashMap<String, RunId>>>,
    pub(crate) managed: Arc<tetonic_run::ManagedRunService>,
    approvals: Arc<Mutex<Option<Arc<dyn crate::approval::ApprovalService>>>>,
    runtime: Arc<Mutex<Option<Arc<tetonic_runtime::EngineRuntime>>>>,
}

impl DefaultRunService {
    pub fn new(
        store: Option<tetonic_memory::SharedStore>,
        policy: Arc<tetonic_policy::PolicyEngine>,
        events: Arc<dyn ApplicationEventSink>,
        supervisor: Arc<dyn RunSupervisor>,
        live: Arc<crate::session_live::SessionLiveStore>,
        artifacts: Arc<dyn tetonic_domain::artifact::ArtifactStore>,
    ) -> Self {
        let identity_supplier: IdentitySupplier = Arc::new(|job_input: &str| {
            let (def, rec) = ("sha256:default".to_string(), "rec_default".to_string());
            let id = AgentIdentity {
                id: tetonic_domain::IdentityId::new("identity_default"),
                owning_application: "tetonic-app".into(),
                bound_definition_digest: def.clone(),
                privilege_class: "default".into(),
                toolset_subscriptions: vec![],
                context_bindings: vec![],
                recovery_id: rec.clone(),
            };
            let spec = AgentJobSpec {
                identity_id: id.id.clone(),
                definition_digest: def,
                input_digest: job_input_digest(job_input),
                capability_bindings: vec![],
                artifact_bindings: vec![],
                recovery_id: rec,
            };
            (id, spec)
        });
        let managed = Arc::new(
            tetonic_run::ManagedRunService::new(
                supervisor.clone(),
                store.clone(),
                artifacts.clone(),
                policy.clone(),
            )
            .with_execution_policy(Arc::new(|_, _, _, _, _| {
                Err("execution policy is not configured".into())
            })),
        );
        let approvals = Arc::new(Mutex::new(None));
        let runtime = Arc::new(Mutex::new(None));
        managed.attach_hooks(Arc::new(super::run_service_hooks::ProductRunHooks {
            bindings: Mutex::new(HashMap::new()),
            scanner: tetonic_secrets::ScannerEngine::default_engine(),
            events: events.clone(),
            approvals: approvals.clone(),
            runtime: runtime.clone(),
        }));
        Self {
            managed,
            store,
            policy,
            events,
            live,
            artifacts,
            identity_supplier,
            session_dispatches: Arc::new(Mutex::new(HashMap::new())),
            session_runs: Arc::new(Mutex::new(HashMap::new())),
            approvals,
            runtime,
        }
    }

    pub fn managed(&self) -> &Arc<tetonic_run::ManagedRunService> {
        &self.managed
    }

    pub fn with_execution_policy(mut self, policy: ExecutionPolicy) -> Self {
        let m = (*self.managed)
            .clone()
            .with_execution_policy(policy.clone());
        self.managed = Arc::new(m);
        self
    }

    pub fn attach_approvals(&self, approvals: Arc<dyn crate::approval::ApprovalService>) {
        *self.approvals.lock_recover() = Some(approvals);
    }

    pub fn attach_runtime(&self, runtime: Arc<tetonic_runtime::EngineRuntime>) {
        *self.runtime.lock_recover() = Some(runtime);
    }

    pub fn with_identity_supplier(mut self, supplier: IdentitySupplier) -> Self {
        self.identity_supplier = supplier;
        self
    }

    async fn begin_turn_run(
        &self,
        session_id: &str,
        identity: &AgentIdentity,
        spec: AgentJobSpec,
    ) -> Result<ActiveTurnRun, AppError> {
        self.begin_job_run(Some(session_id), identity, spec, None)
            .await
    }

    fn bind_live_run(&self, session_id: &str, active: &ActiveTurnRun) {
        self.session_runs
            .lock_recover()
            .insert(session_id.to_string(), active.run_id.clone());
        if let Some(live) = self.live.get(session_id) {
            live.set_current_run(active.run_id.clone(), active.task_id.clone());
        }
    }

    fn find_parent_active(&self, session_id: &str) -> Option<ActiveTurnRun> {
        let run_id = self.session_runs.lock_recover().get(session_id)?.clone();
        let bindings = self.managed.active_bindings(&run_id);
        bindings
            .iter()
            .find(|a| a.task_id.0.starts_with("task_root_"))
            .cloned()
            .or_else(|| bindings.into_iter().next())
    }
    async fn heartbeat_active(&self, attempt: &AttemptId) -> Result<(), AppError> {
        self.managed.heartbeat(attempt).await.map_err(Into::into)
    }
    pub async fn cancel_run(&self, cmd: CancelByRunCommand) -> Result<(), AppError> {
        let run_id = RunId::new(cmd.run_id);
        self.managed.cancel_run(&run_id).await?;
        let sessions: Vec<_> = self
            .session_runs
            .lock_recover()
            .iter()
            .filter(|(_, run)| *run == &run_id)
            .map(|(id, _)| id.clone())
            .collect();
        for session in sessions {
            if let Some(live) = self.live.get(&session) {
                live.request_cancel();
            }
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) async fn run_turn(
        &self,
        cmd: &RunTurnCommand,
        host: &TurnExecutionHost,
        conversation: &mut tetonic_core::Conversation,
        spawn_serial: &mut u32,
    ) -> Result<(), AppError> {
        crate::turn_execution::execute_turn(
            self,
            &self.events,
            cmd,
            host,
            conversation,
            spawn_serial,
        )
        .await
    }
}

#[async_trait::async_trait]
impl RunService for DefaultRunService {
    fn execution_service(&self) -> Arc<dyn RunService> {
        Arc::new(self.clone())
    }

    async fn execute_attempt(
        &self,
        attempt_id: AttemptId,
        agent: &mut tetonic_core::Agent,
        conversation: &mut tetonic_core::Conversation,
        invocation: tetonic_domain::AgentInvocation,
        on_step: &mut (dyn FnMut(tetonic_core::Step) + Send),
    ) -> CandidateOutcome {
        self.execute_bound_attempt(attempt_id, agent, conversation, invocation, on_step)
            .await
    }

    fn event_sink(&self) -> Arc<dyn ApplicationEventSink> {
        self.events.clone()
    }

    async fn create_run(&self, cmd: CreateRunCommand) -> Result<RunId, AppError> {
        // Delegates RunCommand::CreateRun to supervisor via managed
        let session_id = cmd.session_id.filter(|s| !s.is_empty()).map(SessionId::new);
        let task_id = cmd.root_task_id.map(TaskId::new);
        let (identity, spec) = match (cmd.identity, cmd.job_spec) {
            (Some(id), Some(spec)) => (Some(id), Some(spec)),
            (Some(id), None) => {
                let spec = AgentJobSpec {
                    identity_id: id.id.clone(),
                    definition_digest: id.bound_definition_digest.clone(),
                    input_digest: job_input_digest(""),
                    capability_bindings: Vec::new(),
                    artifact_bindings: Vec::new(),
                    recovery_id: id.recovery_id.clone(),
                };
                (Some(id), Some(spec))
            }
            (None, Some(spec)) => (None, Some(spec)),
            (None, None) => {
                let (id, spec) = (self.identity_supplier)("");
                (Some(id), Some(spec))
            }
        };
        self.managed
            .create_run(session_id, task_id, identity, spec)
            .await
            .map_err(Into::into)
    }

    async fn inspect_run(&self, cmd: InspectRunCommand) -> Result<RunSnapshot, AppError> {
        self.managed
            .inspect_run(&RunId::new(cmd.run_id))
            .await
            .map_err(Into::into)
    }

    async fn resume_events(
        &self,
        cmd: ResumeRunEventsCommand,
    ) -> Result<Result<Vec<RunEventEnvelope>, ReplayGap>, AppError> {
        let limit = cmd.limit.unwrap_or(256).min(1024) as usize;
        self.managed
            .resume_events(&RunId::new(cmd.run_id), cmd.after_sequence, limit)
            .await
            .map_err(Into::into)
    }

    async fn cancel_run(&self, cmd: CancelByRunCommand) -> Result<(), AppError> {
        self.cancel_run(cmd).await
    }

    async fn plan_turn(&self, cmd: &RunTurnCommand) -> Result<RunTurnPlan, AppError> {
        let verify_gated = cmd
            .verify_cmd
            .as_deref()
            .map(str::trim)
            .is_some_and(|c| !c.is_empty());
        let llm_router = cmd.llm_router.unwrap_or_else(|| {
            std::env::var("LOKAI_LLM_ROUTER")
                .ok()
                .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        });
        if let Some(s) = &self.store {
            s.write({
                let session_id = cmd.session_id.clone();
                let user_input = cmd.user_input.clone();
                move |db| {
                    db.append_message(&session_id, "user", "", &user_input, None)
                        .map_err(|e| AppError::PersistenceFailed(e.to_string()))
                }
            })
            .await
            .map_err(|e| AppError::PersistenceFailed(e.to_string()))??;
        }
        let (identity, spec) = (self.identity_supplier)(&cmd.user_input);
        let active = self
            .begin_turn_run(&cmd.session_id, &identity, spec)
            .await?;
        self.bind_live_run(&cmd.session_id, &active);
        // R06 named crash inject: durable run journal exists; turn not finished.
        tetonic_telemetry::fault::inject_fault("after_turn_plan");
        let envelope = crate::events::EventEnvelope {
            run_id: Some(active.run_id.0.clone()),
            task_id: Some(active.task_id.0.clone()),
            attempt_id: Some(active.attempt_id.0.clone()),
            identity_id: Some(active.job_spec.identity_id.0.clone()),
        };
        emit(
            &self.events,
            ApplicationEvent::run_status(
                active.run_id.to_string(),
                "started".into(),
                None,
                None,
                &envelope,
            ),
        );
        Ok(RunTurnPlan {
            verify_gated,
            llm_router,
            run_id: active.run_id,
            task_id: active.task_id,
            attempt_id: active.attempt_id,
            job_spec: active.job_spec,
        })
    }

    async fn complete_turn(
        &self,
        cmd: &CompleteTurnCommand,
        outcome: Option<&CandidateOutcome>,
        policy: Option<FinalizationPolicy>,
    ) -> Result<CandidateOutcome, AppError> {
        let canceled = cmd.canceled || matches!(outcome, Some(CandidateOutcome::Canceled { .. }));
        let resolved = if canceled {
            CandidateOutcome::Canceled {
                reason: cmd.error.clone().unwrap_or_else(|| "canceled".into()),
            }
        } else if let Some(outcome) = outcome {
            outcome.clone()
        } else if let Some(error) = &cmd.error {
            CandidateOutcome::Failed {
                message: error.clone(),
            }
        } else {
            CandidateOutcome::Completed {
                summary: String::new(),
                kind: CompletionKind::Finish,
            }
        };
        let active_turn = self
            .managed
            .binding(&cmd.attempt_id)
            .ok_or_else(|| AppError::InvalidRequest("no active turn run".into()))?;
        let run_id = active_turn.run_id.to_string();
        let envelope = crate::events::EventEnvelope {
            run_id: Some(run_id.clone()),
            task_id: Some(active_turn.task_id.0),
            attempt_id: Some(active_turn.attempt_id.0),
            identity_id: Some(active_turn.job_spec.identity_id.0),
        };
        let mut finish_err = None;
        let final_outcome = match self
            .finalize_attempt(
                &cmd.attempt_id,
                &cmd.session_id,
                &resolved,
                canceled,
                policy,
                true,
            )
            .await
        {
            Ok(sealed) => sealed,
            Err(e) => {
                finish_err = Some(e);
                CandidateOutcome::Failed {
                    message: finish_err.as_ref().unwrap().to_string(),
                }
            }
        };
        let canceled = cmd.canceled || matches!(final_outcome, CandidateOutcome::Canceled { .. });
        let status = if canceled {
            "canceled".to_string()
        } else if matches!(
            final_outcome,
            CandidateOutcome::Failed { .. } | CandidateOutcome::Limited { .. }
        ) || finish_err.is_some()
        {
            "error".to_string()
        } else {
            "ok".to_string()
        };
        let error = match &final_outcome {
            CandidateOutcome::Failed { message } | CandidateOutcome::Limited { message, .. } => {
                Some(message.clone())
            }
            CandidateOutcome::Canceled { reason } => Some(reason.clone()),
            _ => cmd
                .error
                .clone()
                .or_else(|| finish_err.as_ref().map(|e| e.to_string())),
        };
        if status == "ok" {
            if let Some(s) = &self.store {
                let persist = s
                    .write({
                        let workspace_root = cmd.workspace_root.clone();
                        let policy = self.policy.clone();
                        let session_id = cmd.session_id.clone();
                        move |db| {
                            let session_host =
                                tetonic_orchestrator::SessionHost::new(workspace_root, policy);
                            session_host.on_turn_end(&session_id, Some(db));
                        }
                    })
                    .await
                    .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")));
                if let Err(e) = persist {
                    finish_err = Some(e);
                }
            }
        }
        emit(
            &self.events,
            ApplicationEvent::turn_completed(
                cmd.session_id.clone(),
                status.clone(),
                error.clone(),
                &envelope,
            ),
        );
        emit(
            &self.events,
            ApplicationEvent::run_status(run_id, status, None, error, &envelope),
        );
        match finish_err {
            Some(e) => Err(e),
            None => Ok(final_outcome),
        }
    }

    async fn report_run_status(&self, cmd: &ReportRunStatusCommand) -> Result<(), AppError> {
        emit(
            &self.events,
            ApplicationEvent::run_status(
                cmd.session_id.clone(),
                cmd.status.clone(),
                cmd.agent_id.clone(),
                cmd.error.clone(),
                &Default::default(),
            ),
        );
        Ok(())
    }

    async fn register_spawn_task(
        &self,
        session_id: &str,
        agent_id: &str,
        role: &str,
        job_input: &str,
    ) -> Result<AttemptId, AppError> {
        if self.find_parent_active(session_id).is_some() {
            return self
                .start_child_attempt(
                    session_id,
                    agent_id,
                    Some(role.to_string()),
                    Some(job_input.to_string()),
                )
                .await;
        }
        let (identity, spec) = (self.identity_supplier)(job_input);
        let active = self
            .begin_job_run(Some(session_id), &identity, spec, Some(role.to_string()))
            .await?;
        self.bind_live_run(session_id, &active);
        // insert(active.attempt_id.clone(), active)
        Ok(active.attempt_id)
    }

    fn child_job(&self, session_id: &str) -> Arc<dyn ChildJob> {
        Arc::new(self::identity_job::AppChildJob {
            session_id: session_id.into(),
            runs: self.clone(),
        })
    }

    fn active_parent_attempt(&self, session_id: &str) -> Option<AttemptId> {
        self.find_parent_active(session_id).map(|a| a.attempt_id)
    }

    fn active_run_for_session(&self, session_id: &str) -> Option<RunId> {
        self.session_runs.lock_recover().get(session_id).cloned()
    }

    async fn heartbeat_turn(&self, attempt_id: &str) -> Result<(), AppError> {
        self.heartbeat_active(&AttemptId::new(attempt_id)).await
    }

    fn drop_active_for_run(&self, run_id: &RunId) {
        self.session_runs.lock_recover().retain(|_, r| r != run_id);
    }

    fn run_id_for_attempt(&self, id: &AttemptId) -> Option<RunId> {
        self.managed.binding(id).map(|a| a.run_id)
    }
    fn task_id_for_attempt(&self, id: &AttemptId) -> Option<TaskId> {
        self.managed.binding(id).map(|a| a.task_id)
    }
    fn active_job_spec(&self, id: &AttemptId) -> Option<AgentJobSpec> {
        self.managed.binding(id).map(|a| a.job_spec)
    }
    fn active_input_digest(&self, id: &AttemptId) -> Option<String> {
        self.managed.binding(id).map(|a| a.job_spec.input_digest)
    }

    async fn start_identity_job(
        &self,
        cmd: StartIdentityJobCommand,
        agent: &mut tetonic_core::Agent,
    ) -> Result<StartIdentityJobResult, AppError> {
        DefaultRunService::start_identity_job(self, cmd, agent).await
    }

    fn arm_attempt_join(&self, id: &AttemptId) -> oneshot::Receiver<StartIdentityJobResult> {
        DefaultRunService::arm_attempt_join(self, id)
    }

    async fn submit_identity_job(
        &self,
        cmd: StartIdentityJobCommand,
        agent: tetonic_core::Agent,
    ) -> Result<(AttemptId, oneshot::Receiver<StartIdentityJobResult>), AppError> {
        DefaultRunService::submit_identity_job(self, cmd, agent).await
    }

    fn is_canceled_attempt(&self, id: &AttemptId) -> bool {
        self.managed.is_canceled(id)
    }
}

#[cfg(test)]
#[path = "execution_regression_tests.rs"]
mod execution_regression_tests;

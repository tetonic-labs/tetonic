//! Lease Infer hops through RunSupervisor (R2-1). Never mint an unleased attempt id.

use chrono::Utc;
use tetonic_domain::ids::{AttemptId, LeaseId};
use tetonic_domain::SessionId;
use tetonic_inference::{ChatRequest, InferenceError};
use tetonic_memory::new_id;

use crate::broker::DefaultComputeBroker;
use crate::scheduler::types::ExecutionTargetId;
use crate::types::ComputeRequest;

/// How this hop relates to the previous Infer attempt.
#[derive(Debug, Clone)]
pub(crate) enum HopAttemptMode {
    /// First hop: keep the incoming id if already leased; create+lease it if missing.
    EnsureExisting,
    /// Sequential failover: fail `previous`, then create and lease a new id.
    Failover { previous: AttemptId },
    /// Speculative extra attempt: create+lease a new id without failing the primary.
    Additional,
}

impl DefaultComputeBroker {
    /// Bind `compute_req.attempt_id` (and `req.fabric.attempt_id`) to a RunSupervisor lease.
    ///
    /// Failover / additional hops fail closed when no supervisor is bound — they must not
    /// dispatch `att_{uuid}` ids the supervisor never leased.
    pub(crate) async fn bind_hop_attempt(
        &self,
        compute_req: &mut ComputeRequest,
        req: &mut ChatRequest,
        target: &ExecutionTargetId,
        mode: HopAttemptMode,
    ) -> Result<(), InferenceError> {
        let Some(supervisor) = self.run_supervisor() else {
            return match mode {
                HopAttemptMode::EnsureExisting => {
                    stamp_fabric_ids(req, compute_req);
                    Ok(())
                }
                HopAttemptMode::Failover { .. } | HopAttemptMode::Additional => {
                    Err(InferenceError::Provider(
                        "failover hop requires RunSupervisor to lease a new attempt".into(),
                    ))
                }
            };
        };

        let hop_session_id = req
            .fabric
            .as_ref()
            .and_then(|f| f.session_id.clone())
            .filter(|s| !s.is_empty());
        self.supervisor_ensure_run(compute_req, hop_session_id)
            .await?;

        let snap = supervisor
            .snapshot(compute_req.run_id.clone())
            .await
            .map_err(|e| {
                InferenceError::Provider(format!("RunSupervisor snapshot for hop lease: {e}"))
            })?;

        if !tetonic_run::hop_run_classified(&snap) {
            return Err(InferenceError::Provider(format!(
                "hop bind refused: run {} is not hop-classified",
                compute_req.run_id.0
            )));
        }

        match mode {
            HopAttemptMode::EnsureExisting => {
                if snap
                    .attempts
                    .get(&compute_req.attempt_id)
                    .and_then(|a| a.lease.as_ref())
                    .is_some()
                {
                    stamp_fabric_ids(req, compute_req);
                    return Ok(());
                }
                self.supervisor_create_and_lease(
                    compute_req,
                    req,
                    target,
                    compute_req.attempt_id.clone(),
                )
                .await
            }
            HopAttemptMode::Failover { previous } => {
                self.supervisor_fail_attempt(compute_req, &previous, snap.sequence, target)
                    .await?;
                let new_id = AttemptId::new(new_id("att"));
                self.supervisor_create_and_lease(compute_req, req, target, new_id)
                    .await
            }
            HopAttemptMode::Additional => {
                let new_id = AttemptId::new(new_id("att"));
                self.supervisor_create_and_lease(compute_req, req, target, new_id)
                    .await
            }
        }
    }

    /// Interactive chat often mints a `run_{uuid}` when fabric metadata is incomplete.
    /// Create+start that run (and its root task) so hop lease does not fail-close
    /// before Ollama runs.
    async fn supervisor_ensure_run(
        &self,
        compute_req: &ComputeRequest,
        session_id: Option<String>,
    ) -> Result<(), InferenceError> {
        let Some(supervisor) = self.run_supervisor() else {
            return Ok(());
        };

        let session_id = session_id.filter(|s| !s.is_empty()).map(SessionId::new);
        tetonic_run::ensure_hop_run(
            supervisor.as_ref(),
            compute_req.run_id.clone(),
            compute_req.task_id.clone(),
            session_id,
        )
        .await
        .map_err(|e| InferenceError::Provider(format!("hop admission: {e}")))?;
        Ok(())
    }

    async fn supervisor_fail_attempt(
        &self,
        compute_req: &ComputeRequest,
        previous: &AttemptId,
        expected_sequence: u64,
        target: &ExecutionTargetId,
    ) -> Result<u64, InferenceError> {
        let Some(supervisor) = self.run_supervisor() else {
            return Err(InferenceError::Provider(
                "failover hop requires RunSupervisor".into(),
            ));
        };
        tetonic_run::fail_hop(
            supervisor.as_ref(),
            compute_req.run_id.clone(),
            previous.clone(),
            expected_sequence,
            format!("infer hop failed; retrying on {}", target.as_label()),
        )
        .await
        .map_err(|e| InferenceError::Provider(format!("FailAttempt for failover hop: {e}")))
    }

    async fn supervisor_create_and_lease(
        &self,
        compute_req: &mut ComputeRequest,
        req: &mut ChatRequest,
        target: &ExecutionTargetId,
        attempt_id: AttemptId,
    ) -> Result<(), InferenceError> {
        let Some(supervisor) = self.run_supervisor() else {
            return Err(InferenceError::Provider(
                "hop lease requires RunSupervisor".into(),
            ));
        };

        let snap = supervisor
            .snapshot(compute_req.run_id.clone())
            .await
            .map_err(|e| InferenceError::Provider(format!("snapshot before CreateAttempt: {e}")))?;
        if let Some(task) = snap.tasks.get(&compute_req.task_id) {
            if !tetonic_run::hop_task_leaseable(&task.state, snap.speculation.allowed) {
                return Err(InferenceError::Provider(format!(
                    "cannot lease hop attempt: task in {:?}",
                    task.state
                )));
            }
        }
        let expected_sequence = snap.sequence;

        let created_seq = tetonic_run::create_hop_attempt(
            supervisor.as_ref(),
            compute_req.run_id.clone(),
            compute_req.task_id.clone(),
            attempt_id.clone(),
            expected_sequence,
        )
        .await
        .map_err(|e| InferenceError::Provider(format!("CreateAttempt for hop: {e}")))?;

        let issued_at = Utc::now().timestamp().max(0) as u64;
        let holder = target.clone();
        let leased = tetonic_run::lease_hop(
            supervisor.as_ref(),
            compute_req.run_id.clone(),
            attempt_id.clone(),
            LeaseId::new(new_id("lease")),
            holder,
            issued_at,
            created_seq,
        )
        .await
        .map_err(|e| InferenceError::Provider(format!("LeaseAttempt for hop: {e}")))?;

        let rec =
            leased.snapshot.attempts.get(&attempt_id).ok_or_else(|| {
                InferenceError::Provider("attempt missing after hop lease".into())
            })?;
        if rec.lease.is_none() {
            return Err(InferenceError::Provider(
                "hop attempt leased command did not record a lease".into(),
            ));
        }

        compute_req.attempt_id = attempt_id;
        compute_req.trace_context.attempt_id = Some(compute_req.attempt_id.clone());
        stamp_fabric_ids(req, compute_req);
        Ok(())
    }
}

fn stamp_fabric_ids(req: &mut ChatRequest, compute_req: &ComputeRequest) {
    let meta = req.fabric.get_or_insert_with(Default::default);
    meta.hop_attempt_id = Some(compute_req.attempt_id.0.clone());
    meta.hop_run_id = Some(compute_req.run_id.0.clone());
    meta.hop_task_id = Some(compute_req.task_id.0.clone());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tetonic_domain::ids::RunId;
    use tetonic_domain::{
        AgentJobSpec, AttemptState, CreateAttempt, CreateRun, IdentityId, LeaseAttempt, LeaseProof,
        RunCommand, SessionId, SpeculationConfig, StartAttempt, StartRun, TaskId, TaskState,
    };
    use tetonic_inference::{ChatRequest, Message};
    use tetonic_run::command_envelope;
    use tetonic_run::{DurableRunSupervisor, RunSupervisor};

    use crate::admission::HierarchicalAdmissionController;
    use crate::persist::InMemoryReservationStore;

    async fn seed_run(
        sup: &DurableRunSupervisor,
        run: &str,
        task: &str,
        attempt: &str,
        speculation: Option<SpeculationConfig>,
    ) {
        let run_id = RunId::new(run);
        let task_id = TaskId::new(task);
        let attempt_id = AttemptId::new(attempt);
        let created = sup
            .handle(RunCommand::CreateRun(CreateRun {
                envelope: command_envelope("t_create", None, "test"),
                session_id: Some(SessionId::new("sess")),
                run_id: run_id.clone(),
                root_task_id: task_id.clone(),
                root_binding: Default::default(),
                speculation,
                job_spec: None,
            }))
            .await
            .unwrap();
        let started = sup
            .handle(RunCommand::StartRun(StartRun {
                envelope: command_envelope("t_start", Some(created.sequence), "test"),
                run_id: run_id.clone(),
            }))
            .await
            .unwrap();
        let made = sup
            .handle(RunCommand::CreateAttempt(CreateAttempt {
                envelope: command_envelope("t_att", Some(started.sequence), "test"),
                run_id: run_id.clone(),
                task_id: task_id.clone(),
                attempt_id: attempt_id.clone(),
                delivery_key: Some("turn".into()),
            }))
            .await
            .unwrap();
        sup.handle(RunCommand::LeaseAttempt(LeaseAttempt {
            envelope: command_envelope("t_lease", Some(made.sequence), "test"),
            run_id,
            attempt_id,
            lease_id: LeaseId::new("lease_0"),
            lease_epoch: 0,
            holder: ExecutionTargetId::Local,
            issued_at: 1,
            expires_at: 10_000,
            heartbeat_interval_secs: 30,
        }))
        .await
        .unwrap();
    }

    fn broker_with_supervisor(sup: Arc<DurableRunSupervisor>) -> DefaultComputeBroker {
        let budgets = Arc::new(crate::budget::HierarchicalBudgetLedger::new(
            crate::budget::BudgetLimits::default(),
        ));
        let queue = Arc::new(crate::queue::QueueManager::new(
            crate::queue::QueueLimits::default(),
            crate::priority::FairnessPolicy::default(),
        ));
        let admission = Arc::new(HierarchicalAdmissionController::new(budgets, queue));
        let persist: Arc<dyn crate::persist::ReservationStore> =
            Arc::new(InMemoryReservationStore::default());
        DefaultComputeBroker::new(
            admission,
            persist,
            None,
            Some(sup as Arc<dyn RunSupervisor>),
        )
    }

    fn chat() -> ChatRequest {
        ChatRequest {
            max_tokens: None,
            model: "m".into(),
            model_digest: None,
            messages: vec![Message::user("hi")],
            tools: vec![],
            temperature: 0.0,
            num_ctx: None,
            draft_model: None,
            draft_count: None,
            keep_alive: None,
            fabric: Some(tetonic_inference::FabricCallMeta {
                run_id: Some("run_r2".into()),
                task_id: Some("task_r2".into()),
                attempt_id: Some("att_first".into()),
                ..Default::default()
            }),
            response_format: None,
            outbound_scan: Default::default(),
        }
    }

    #[tokio::test]
    async fn failover_hop_leases_second_attempt_in_supervisor() {
        let sup = Arc::new(DurableRunSupervisor::new(None));
        let broker = broker_with_supervisor(sup.clone());
        let mut compute = crate::chat_request::compute_request_from_chat(&chat());
        let mut req = chat();
        broker
            .bind_hop_attempt(
                &mut compute,
                &mut req,
                &ExecutionTargetId::Local,
                HopAttemptMode::EnsureExisting,
            )
            .await
            .unwrap();
        let first = compute.attempt_id.clone();
        assert_ne!(first.0, "att_first");

        broker
            .bind_hop_attempt(
                &mut compute,
                &mut req,
                &ExecutionTargetId::Local,
                HopAttemptMode::Failover {
                    previous: first.clone(),
                },
            )
            .await
            .unwrap();
        assert_ne!(compute.attempt_id, first);
        assert_eq!(
            req.fabric
                .as_ref()
                .and_then(|f| f.hop_attempt_id.as_deref()),
            Some(compute.attempt_id.0.as_str())
        );
        assert_eq!(
            req.fabric.as_ref().and_then(|f| f.attempt_id.as_deref()),
            Some("att_first")
        );

        let snap = sup.snapshot(compute.run_id.clone()).await.unwrap();
        let first_rec = snap.attempts.get(&first).expect("first attempt in journal");
        assert!(
            matches!(
                first_rec.state,
                tetonic_domain::AttemptState::Failed
                    | tetonic_domain::AttemptState::Canceled
                    | tetonic_domain::AttemptState::LeaseExpired
            ),
            "previous attempt must be terminal, got {:?}",
            first_rec.state
        );
        let second = snap
            .attempts
            .get(&compute.attempt_id)
            .expect("second attempt in journal");
        assert!(second.lease.is_some(), "failover hop must be leased");
        assert!(
            first_rec.lease.is_some()
                || matches!(
                    first_rec.state,
                    tetonic_domain::AttemptState::Failed
                        | tetonic_domain::AttemptState::Canceled
                        | tetonic_domain::AttemptState::LeaseExpired
                ),
            "first attempt was leased before failover"
        );
        assert!(second
            .lease
            .as_ref()
            .map(|l| l.lease_id.0.as_str())
            .is_some());
    }

    #[tokio::test]
    async fn failover_without_supervisor_is_rejected() {
        let budgets = Arc::new(crate::budget::HierarchicalBudgetLedger::new(
            crate::budget::BudgetLimits::default(),
        ));
        let queue = Arc::new(crate::queue::QueueManager::new(
            crate::queue::QueueLimits::default(),
            crate::priority::FairnessPolicy::default(),
        ));
        let admission = Arc::new(HierarchicalAdmissionController::new(budgets, queue));
        let persist: Arc<dyn crate::persist::ReservationStore> =
            Arc::new(InMemoryReservationStore::default());
        let broker = DefaultComputeBroker::new(admission, persist, None, None);
        let mut compute = crate::chat_request::compute_request_from_chat(&chat());
        let mut req = chat();
        let err = broker
            .bind_hop_attempt(
                &mut compute,
                &mut req,
                &ExecutionTargetId::Local,
                HopAttemptMode::Failover {
                    previous: AttemptId::new("att_first"),
                },
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("RunSupervisor"), "got {err}");
    }

    #[tokio::test]
    async fn ensure_existing_leases_unleased_first_hop() {
        let sup = Arc::new(DurableRunSupervisor::new(None));
        let broker = broker_with_supervisor(sup.clone());
        let mut compute = crate::chat_request::compute_request_from_chat(&chat());
        let mut req = chat();
        broker
            .bind_hop_attempt(
                &mut compute,
                &mut req,
                &ExecutionTargetId::Local,
                HopAttemptMode::EnsureExisting,
            )
            .await
            .unwrap();
        let snap = sup.snapshot(compute.run_id.clone()).await.unwrap();
        let rec = snap
            .attempts
            .get(&compute.attempt_id)
            .expect("first hop attempt in journal");
        assert!(
            rec.lease.is_some(),
            "EnsureExisting must CreateAttempt+LeaseAttempt"
        );
        assert!(snap.job_spec.is_none());
    }

    #[tokio::test]
    async fn additional_hop_leases_second_attempt_without_failing_primary() {
        let sup = Arc::new(DurableRunSupervisor::new(None));
        seed_run(
            &sup,
            "run_r2",
            "task_r2",
            "att_first",
            Some(SpeculationConfig {
                allowed: true,
                max_simultaneous_attempts: 2,
                require_result_agreement: false,
            }),
        )
        .await;
        let broker = broker_with_supervisor(sup.clone());
        let mut compute = crate::chat_request::compute_request_from_chat(&chat());
        let mut req = chat();
        broker
            .bind_hop_attempt(
                &mut compute,
                &mut req,
                &ExecutionTargetId::Local,
                HopAttemptMode::EnsureExisting,
            )
            .await
            .unwrap();
        let first = compute.attempt_id.clone();
        broker
            .bind_hop_attempt(
                &mut compute,
                &mut req,
                &ExecutionTargetId::Local,
                HopAttemptMode::Additional,
            )
            .await
            .unwrap();
        assert_ne!(compute.attempt_id, first);
        let snap = sup.snapshot(compute.run_id.clone()).await.unwrap();
        let primary = snap.attempts.get(&first).expect("primary still in journal");
        assert!(
            matches!(
                primary.state,
                tetonic_domain::AttemptState::Leased
                    | tetonic_domain::AttemptState::Created
                    | tetonic_domain::AttemptState::Starting
                    | tetonic_domain::AttemptState::Running
            ),
            "primary must remain active, got {:?}",
            primary.state
        );
        let extra = snap
            .attempts
            .get(&compute.attempt_id)
            .expect("speculative attempt in journal");
        assert!(extra.lease.is_some(), "additional hop must be leased");
    }

    #[tokio::test]
    async fn ensure_existing_creates_missing_run_instead_of_failing_chat() {
        let sup = Arc::new(DurableRunSupervisor::new(None));
        let broker = broker_with_supervisor(sup.clone());
        let req_src = ChatRequest {
            max_tokens: None,
            model: "m".into(),
            model_digest: None,
            messages: vec![Message::user("hi")],
            tools: vec![],
            temperature: 0.0,
            num_ctx: None,
            draft_model: None,
            draft_count: None,
            keep_alive: None,
            fabric: None,
            response_format: None,
            outbound_scan: Default::default(),
        };
        let mut compute = crate::chat_request::compute_request_from_chat(&req_src);
        let mut req = req_src;
        broker
            .bind_hop_attempt(
                &mut compute,
                &mut req,
                &ExecutionTargetId::Local,
                HopAttemptMode::EnsureExisting,
            )
            .await
            .expect("missing run must be created so Infer can proceed");
        let snap = sup
            .snapshot(compute.run_id.clone())
            .await
            .expect("hop CreateRun must persist the minted run");
        let rec = snap.attempts.get(&compute.attempt_id).expect("hop attempt");
        assert!(
            rec.lease.is_some(),
            "EnsureExisting must lease after CreateRun"
        );
    }

    fn agent_job_spec() -> AgentJobSpec {
        AgentJobSpec {
            identity_id: IdentityId::new("id_cmp02"),
            definition_digest: "digest".into(),
            input_digest: "input:1".into(),
            capability_bindings: Vec::new(),
            artifact_bindings: Vec::new(),
            recovery_id: "id_cmp02".into(),
        }
    }

    async fn seed_agent_run(sup: &DurableRunSupervisor, run: &str, task: &str, attempt: &str) {
        let run_id = RunId::new(run);
        let task_id = TaskId::new(task);
        let attempt_id = AttemptId::new(attempt);
        let created = sup
            .handle(RunCommand::CreateRun(CreateRun {
                envelope: command_envelope("t_agent_create", None, "test"),
                session_id: Some(SessionId::new("sess")),
                run_id: run_id.clone(),
                root_task_id: task_id.clone(),
                root_binding: Default::default(),
                speculation: None,
                job_spec: Some(agent_job_spec()),
            }))
            .await
            .unwrap();
        let started = sup
            .handle(RunCommand::StartRun(StartRun {
                envelope: command_envelope("t_agent_start", Some(created.sequence), "test"),
                run_id: run_id.clone(),
            }))
            .await
            .unwrap();
        let made = sup
            .handle(RunCommand::CreateAttempt(CreateAttempt {
                envelope: command_envelope("t_agent_att", Some(started.sequence), "test"),
                run_id: run_id.clone(),
                task_id: task_id.clone(),
                attempt_id: attempt_id.clone(),
                delivery_key: Some("turn".into()),
            }))
            .await
            .unwrap();
        let leased = sup
            .handle(RunCommand::LeaseAttempt(LeaseAttempt {
                envelope: command_envelope("t_agent_lease", Some(made.sequence), "test"),
                run_id: run_id.clone(),
                attempt_id: attempt_id.clone(),
                lease_id: LeaseId::new("lease_agent"),
                lease_epoch: 0,
                holder: ExecutionTargetId::Local,
                issued_at: 1,
                expires_at: 10_000,
                heartbeat_interval_secs: 30,
            }))
            .await
            .unwrap();
        let rec = leased.snapshot.attempts.get(&attempt_id).unwrap();
        let lease = rec.lease.as_ref().unwrap();
        sup.handle(RunCommand::StartAttempt(StartAttempt {
            envelope: command_envelope("t_agent_running", Some(leased.sequence), "test"),
            run_id,
            attempt_id,
            lease_proof: LeaseProof {
                lease_id: lease.lease_id.clone(),
                lease_epoch: lease.lease_epoch,
                holder: lease.holder.clone(),
            },
        }))
        .await
        .unwrap();
    }

    fn chat_with_agent_ids() -> ChatRequest {
        ChatRequest {
            max_tokens: None,
            model: "m".into(),
            model_digest: None,
            messages: vec![Message::user("hi")],
            tools: vec![],
            temperature: 0.0,
            num_ctx: None,
            draft_model: None,
            draft_count: None,
            keep_alive: None,
            fabric: Some(tetonic_inference::FabricCallMeta {
                run_id: Some("run_agent".into()),
                task_id: Some("task_agent".into()),
                attempt_id: Some("att_agent".into()),
                ..Default::default()
            }),
            response_format: None,
            outbound_scan: Default::default(),
        }
    }

    #[test]
    fn cmp02_compute_request_mints_hop_ids() {
        let req = chat_with_agent_ids();
        let compute = crate::chat_request::compute_request_from_chat(&req);
        assert_ne!(compute.run_id.0, "run_agent");
        assert_ne!(compute.task_id.0, "task_agent");
        assert_ne!(compute.attempt_id.0, "att_agent");
        assert_eq!(
            req.fabric.as_ref().and_then(|f| f.run_id.as_deref()),
            Some("run_agent")
        );
    }

    #[tokio::test]
    async fn cmp02_ensure_existing_rejects_leased_agent_attempt() {
        let sup = Arc::new(DurableRunSupervisor::new(None));
        seed_agent_run(&sup, "run_agent", "task_agent", "att_agent").await;
        let broker = broker_with_supervisor(sup.clone());
        let mut compute = crate::types::ComputeRequest {
            run_id: RunId::new("run_agent"),
            task_id: TaskId::new("task_agent"),
            attempt_id: AttemptId::new("att_agent"),
            ..crate::chat_request::compute_request_from_chat(&chat_with_agent_ids())
        };
        let mut req = chat_with_agent_ids();
        let err = broker
            .bind_hop_attempt(
                &mut compute,
                &mut req,
                &ExecutionTargetId::Local,
                HopAttemptMode::EnsureExisting,
            )
            .await
            .expect_err("agent Attempt must not be admitted as a hop");
        assert!(err.to_string().contains("hop"), "got {err}");
        let snap = sup.snapshot(RunId::new("run_agent")).await.unwrap();
        assert_eq!(
            snap.attempts
                .get(&AttemptId::new("att_agent"))
                .map(|a| a.state.clone()),
            Some(AttemptState::Running)
        );
    }

    #[tokio::test]
    async fn cmp02_failover_leaves_agent_attempt_running() {
        let sup = Arc::new(DurableRunSupervisor::new(None));
        seed_agent_run(&sup, "run_agent", "task_agent", "att_agent").await;
        let broker = broker_with_supervisor(sup.clone());
        let mut req = chat_with_agent_ids();
        let mut compute = crate::chat_request::compute_request_from_chat(&req);
        assert_ne!(compute.run_id.0, "run_agent");
        broker
            .bind_hop_attempt(
                &mut compute,
                &mut req,
                &ExecutionTargetId::Local,
                HopAttemptMode::EnsureExisting,
            )
            .await
            .unwrap();
        let hop_first = compute.attempt_id.clone();
        broker
            .bind_hop_attempt(
                &mut compute,
                &mut req,
                &ExecutionTargetId::Local,
                HopAttemptMode::Failover {
                    previous: hop_first.clone(),
                },
            )
            .await
            .unwrap();

        let agent = sup.snapshot(RunId::new("run_agent")).await.unwrap();
        assert_eq!(
            agent
                .attempts
                .get(&AttemptId::new("att_agent"))
                .map(|a| a.state.clone()),
            Some(AttemptState::Running)
        );
        assert!(agent.job_spec.is_some());

        let hop = sup.snapshot(compute.run_id.clone()).await.unwrap();
        assert!(hop.job_spec.is_none());
        assert_eq!(
            hop.attempts.get(&hop_first).map(|a| a.state.clone()),
            Some(AttemptState::Failed)
        );
        assert!(hop
            .attempts
            .get(&compute.attempt_id)
            .unwrap()
            .lease
            .is_some());
        assert!(
            !hop.attempts
                .values()
                .any(|a| a.state == AttemptState::Succeeded),
            "no hop CompleteAttempt"
        );
        assert_eq!(
            req.fabric.as_ref().and_then(|f| f.attempt_id.as_deref()),
            Some("att_agent")
        );
    }

    #[tokio::test]
    async fn cmp02_speculation_cancel_does_not_cancel_agent_task() {
        let sup = Arc::new(DurableRunSupervisor::new(None));
        seed_agent_run(&sup, "run_agent", "task_agent", "att_agent").await;
        let broker = broker_with_supervisor(sup.clone());
        let mut req = chat_with_agent_ids();
        let mut compute = crate::chat_request::compute_request_from_chat(&req);
        broker
            .bind_hop_attempt(
                &mut compute,
                &mut req,
                &ExecutionTargetId::Local,
                HopAttemptMode::EnsureExisting,
            )
            .await
            .unwrap();
        broker
            .bind_hop_attempt(
                &mut compute,
                &mut req,
                &ExecutionTargetId::Local,
                HopAttemptMode::Additional,
            )
            .await
            .unwrap();
        tetonic_run::cancel_hop(
            sup.as_ref(),
            compute.run_id.clone(),
            compute.task_id.clone(),
        )
        .await
        .unwrap();

        let hop = sup.snapshot(compute.run_id.clone()).await.unwrap();
        assert_eq!(
            hop.tasks.get(&compute.task_id).map(|t| t.state.clone()),
            Some(TaskState::Canceled)
        );
        let agent = sup.snapshot(RunId::new("run_agent")).await.unwrap();
        assert_ne!(
            agent
                .tasks
                .get(&TaskId::new("task_agent"))
                .map(|t| t.state.clone()),
            Some(TaskState::Canceled)
        );
        assert_eq!(
            agent
                .attempts
                .get(&AttemptId::new("att_agent"))
                .map(|a| a.state.clone()),
            Some(AttemptState::Running)
        );
    }

    #[tokio::test]
    async fn cmp02_stamp_does_not_clobber_agent_correlation() {
        let sup = Arc::new(DurableRunSupervisor::new(None));
        let broker = broker_with_supervisor(sup);
        let mut req = chat_with_agent_ids();
        let mut compute = crate::chat_request::compute_request_from_chat(&req);
        broker
            .bind_hop_attempt(
                &mut compute,
                &mut req,
                &ExecutionTargetId::Local,
                HopAttemptMode::EnsureExisting,
            )
            .await
            .unwrap();
        let fabric = req.fabric.as_ref().unwrap();
        assert_eq!(fabric.run_id.as_deref(), Some("run_agent"));
        assert_eq!(fabric.task_id.as_deref(), Some("task_agent"));
        assert_eq!(fabric.attempt_id.as_deref(), Some("att_agent"));
        assert_eq!(
            fabric.hop_run_id.as_deref(),
            Some(compute.run_id.0.as_str())
        );
        assert_eq!(
            fabric.hop_task_id.as_deref(),
            Some(compute.task_id.0.as_str())
        );
        assert_eq!(
            fabric.hop_attempt_id.as_deref(),
            Some(compute.attempt_id.0.as_str())
        );
    }
}

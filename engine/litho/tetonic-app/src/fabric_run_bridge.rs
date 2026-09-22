//! RunSupervisor bridge for M5-4 remote result acceptance.

use std::sync::Arc;

use async_trait::async_trait;
use tetonic_domain::{AttemptId, CommandEnvelope, LeaseProof, RunId, RunSnapshot};
use tetonic_fabric_client::RemoteResultRunBridge;
use tetonic_run::{command_envelope, RunSupervisor};

/// Production bridge: fabric may load snapshot/lease proof for quarantine. It
/// does not submit a manager completion command.
pub(crate) struct SupervisorRunBridge {
    supervisor: Arc<dyn RunSupervisor>,
}

impl SupervisorRunBridge {
    pub(crate) fn new(supervisor: Arc<dyn RunSupervisor>) -> Self {
        Self { supervisor }
    }

    pub(crate) fn arc(supervisor: Arc<dyn RunSupervisor>) -> Arc<dyn RemoteResultRunBridge> {
        Arc::new(Self::new(supervisor))
    }
}

#[async_trait]
impl RemoteResultRunBridge for SupervisorRunBridge {
    async fn load_snapshot(&self, run_id: &str) -> Result<Option<RunSnapshot>, String> {
        match self.supervisor.snapshot(RunId::new(run_id)).await {
            Ok(snap) => Ok(Some(snap)),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("not found") {
                    Ok(None)
                } else {
                    Err(msg)
                }
            }
        }
    }

    async fn lease_proof_for_attempt(
        &self,
        run_id: &str,
        attempt_id: &str,
    ) -> Result<Option<(LeaseProof, CommandEnvelope)>, String> {
        let Some(snap) = self.load_snapshot(run_id).await? else {
            return Ok(None);
        };
        let attempt = snap.attempts.get(&AttemptId::new(attempt_id));
        let Some(attempt) = attempt else {
            return Ok(None);
        };
        let Some(lease) = attempt.lease.as_ref() else {
            return Ok(None);
        };
        let proof = LeaseProof {
            lease_id: lease.lease_id.clone(),
            lease_epoch: lease.lease_epoch,
            holder: lease.holder.clone(),
        };
        let env = command_envelope("fabric_result_complete", Some(snap.sequence), "fabric");
        Ok(Some((proof, env)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tetonic_domain::{
        CreateAttempt, CreateRun, ExecutionTargetId, FailAttempt, FailureClass, LeaseAttempt,
        LeaseId, RunCommand, StartRun, TaskId,
    };
    use tetonic_run::DurableRunSupervisor;

    #[tokio::test]
    async fn lease_proof_for_failover_winner_matches_second_lease() {
        let sup = Arc::new(DurableRunSupervisor::new(None));
        let run_id = RunId::new("run_r2");
        let task_id = TaskId::new("task_r2");
        let first = AttemptId::new("att_first");
        let created = sup
            .handle(RunCommand::CreateRun(CreateRun {
                envelope: command_envelope("t_create", None, "test"),
                session_id: None,
                run_id: run_id.clone(),
                root_task_id: task_id.clone(),
                root_binding: Default::default(),
                speculation: None,
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
                attempt_id: first.clone(),
                delivery_key: Some("turn".into()),
            }))
            .await
            .unwrap();
        let leased = sup
            .handle(RunCommand::LeaseAttempt(LeaseAttempt {
                envelope: command_envelope("t_lease", Some(made.sequence), "test"),
                run_id: run_id.clone(),
                attempt_id: first.clone(),
                lease_id: LeaseId::new("lease_0"),
                lease_epoch: 0,
                holder: ExecutionTargetId::local(),
                issued_at: 1,
                expires_at: 10_000,
                heartbeat_interval_secs: 30,
            }))
            .await
            .unwrap();
        let failed = sup
            .handle(RunCommand::FailAttempt(FailAttempt {
                envelope: command_envelope("t_fail", Some(leased.sequence), "test"),
                run_id: run_id.clone(),
                attempt_id: first.clone(),
                failure_class: FailureClass::WorkerUnavailable,
                reason: "hop failed".into(),
                lease_proof: None,
                timeout_kind: None,
            }))
            .await
            .unwrap();
        let second = AttemptId::new("att_second");
        let made2 = sup
            .handle(RunCommand::CreateAttempt(CreateAttempt {
                envelope: command_envelope("t_att2", Some(failed.sequence), "test"),
                run_id: run_id.clone(),
                task_id: task_id.clone(),
                attempt_id: second.clone(),
                delivery_key: Some("hop".into()),
            }))
            .await
            .unwrap();
        sup.handle(RunCommand::LeaseAttempt(LeaseAttempt {
            envelope: command_envelope("t_lease2", Some(made2.sequence), "test"),
            run_id: run_id.clone(),
            attempt_id: second.clone(),
            lease_id: LeaseId::new("lease_1"),
            lease_epoch: 0,
            holder: ExecutionTargetId::worker("worker_a"),
            issued_at: 2,
            expires_at: 10_000,
            heartbeat_interval_secs: 30,
        }))
        .await
        .unwrap();

        let bridge = SupervisorRunBridge::new(sup);
        let (proof, _) = bridge
            .lease_proof_for_attempt("run_r2", "att_second")
            .await
            .unwrap()
            .expect("winner lease proof");
        assert_eq!(proof.lease_id.0, "lease_1");
        assert_eq!(proof.holder, ExecutionTargetId::worker("worker_a"));
    }
}

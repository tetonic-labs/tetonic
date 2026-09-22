//! The No-V4 test (M1): WorkerTarget implements Infer/Embed only.
//!
//! A process-class job (`TestShard` / `IndexShard`) aimed at a worker must be refused at
//! the dispatch gate — not silently downgraded to local execution, and never handed to
//! an executor. Asserted at the production dispatch call site (`dispatch_sandboxed`).
//! `BrokerGatedProcessBroker::execute` is identity over the inner sandbox broker (M2 D-2)
//! and does not call `dispatch_sandboxed`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::sync::RwLock;
use tetonic_broker::*;
use tetonic_domain::ids::{ActionId, AgentId, AttemptId, RunId, SessionId, TaskId, WorkerId};
use tetonic_domain::sinks::{
    AuthorizedProcessRequest, AuthorizedServiceRequest, ManagedProcessHandle, ManagedProcessResult,
    ProcessBroker, ProcessBrokerError,
};
use tetonic_domain::{
    ActionKind, AuthorizedAction, ContentDigest, DataClass, EligibleTarget, IssuedCapability,
    PlacementReason, ProposedAction, TraceContext, TrustPlacementDecision,
    VerificationPolicyReference, VerificationRequirement, WorkerTrust,
};
use tetonic_fabric_protocol::{CapabilityRegistry, JobKind, WorkerCapabilities};
use tetonic_run::RunSupervisor;

/// Executor that records whether the broker ever handed it work.
#[derive(Default)]
struct SpyProcess {
    calls: AtomicUsize,
}

#[async_trait]
impl ProcessBroker for SpyProcess {
    async fn execute(
        &self,
        _request: AuthorizedProcessRequest,
    ) -> Result<ManagedProcessResult, ProcessBrokerError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ManagedProcessResult {
            success: true,
            output: "ok".into(),
            execution_id: tetonic_domain::ExecutionId::new("e1"),
        })
    }

    async fn start_service(
        &self,
        _request: AuthorizedServiceRequest,
    ) -> Result<Box<dyn ManagedProcessHandle>, ProcessBrokerError> {
        Err(ProcessBrokerError::ExecutionFailed("not used".into()))
    }
}

fn test_supervisor() -> Arc<dyn RunSupervisor> {
    Arc::new(tetonic_run::DurableRunSupervisor::new(None))
}

fn broker() -> DefaultComputeBroker {
    let budgets = Arc::new(HierarchicalBudgetLedger::new(BudgetLimits::default()));
    let queue = Arc::new(QueueManager::new(
        QueueLimits::default(),
        FairnessPolicy::default(),
    ));
    let admission = Arc::new(HierarchicalAdmissionController::new(budgets, queue));
    let persist: Arc<dyn ReservationStore> = Arc::new(InMemoryReservationStore::default());
    DefaultComputeBroker::new(admission, persist, None, Some(test_supervisor()))
}

/// Advertise `worker_id` as a healthy, eligible Infer worker. Without this the dispatch
/// gate would refuse any remote target for lack of a capability cache, and the test
/// could not tell "refused because Process is local-only" from "refused because stale".
fn registry_with_eligible_infer_worker(worker_id: &WorkerId) -> Arc<RwLock<CapabilityRegistry>> {
    let mut registry = CapabilityRegistry::new();
    let caps = WorkerCapabilities::legacy_infer_profile(
        worker_id.clone(),
        "boot",
        1,
        0,
        &["m".into()],
        &["m".into()],
        8192,
        4096,
        0,
        0,
        2,
    );
    registry
        .upsert_validated(caps, worker_id, 0, Utc::now())
        .expect("advertise infer worker");
    Arc::new(RwLock::new(registry))
}

/// A sandboxed verification job. `target` is the worker the caller is aiming at, if any.
fn test_shard_request(attempt: &str, target: Option<WorkerId>) -> ComputeRequest {
    let now = Utc::now();
    let decision = match &target {
        Some(worker_id) => TrustPlacementDecision::Eligible {
            targets: vec![EligibleTarget {
                worker_id: worker_id.clone(),
                trust: WorkerTrust::OwnerControlledEstate,
            }],
            required_verification: VerificationRequirement::None,
        },
        None => TrustPlacementDecision::LocalOnly {
            reason: PlacementReason::CapabilityUnavailable,
        },
    };
    ComputeRequest {
        run_id: RunId::new("r_nov4"),
        task_id: TaskId::new("t_nov4"),
        task_version: 1,
        attempt_id: AttemptId::new(attempt),
        job_kind: JobKind::TestShard,
        input_artifacts: vec![],
        input_digest: ContentDigest::new("sha256:in"),
        workspace_version: None,
        data_class: DataClass::RepositorySource,
        placement_decision: PlacementDecisionReference {
            decision_id: "plc_nov4".into(),
            issued_at: now,
            expires_at: now + Duration::minutes(10),
            policy_epoch: 1,
            decision,
        },
        resource_request: profile_for(&JobKind::TestShard).default_resources,
        deadline: DeadlinePolicy {
            queue_deadline: now + Duration::minutes(5),
            execution_deadline: now + Duration::hours(1),
        },
        retry_policy: RetryPolicyReference::default(),
        verification_policy: VerificationPolicyReference {
            policy_id: "structural".into(),
        },
        priority: ComputePriority::VerificationCritical,
        trace_context: TraceContext::default(),
        speculative: false,
        project_id: None,
        target_worker_id: target,
        fallback_order: vec![],
        scheduler_decision_id: None,
    }
}

fn process_request() -> AuthorizedProcessRequest {
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let action = ProposedAction {
        action_id: ActionId::new("act_nov4"),
        session_id: SessionId::new("s_nov4"),
        run_id: Some(RunId::new("r_nov4")),
        task_id: Some(TaskId::new("t_nov4")),
        attempt_id: None,
        agent_id: Some(AgentId::new("a0")),
        workspace_version: None,
        data_class: DataClass::RepositorySource,
        kind: ActionKind::ExecuteProcess,
        parameters: tetonic_domain::execution::CanonicalActionParameters {
            digest: "d_nov4".into(),
            executable_identity: Some("true".into()),
            resolved_path: None,
            arguments: vec![],
            shell_identity: None,
            shell_mode: None,
            script_bytes: None,
            working_directory: None,
            env_vars: None,
            stdin_source_classification: None,
            filesystem_access_scope: None,
            network_policy: None,
            resource_limits: None,
            process_class: Some(tetonic_domain::execution::ProcessClass::BuildVerification),
            sandbox_profile: None,
            expected_output_limits: None,
            schema_version: 1,
            tool_arguments: None,
        },
        requested_capabilities: Default::default(),
        trace_context: Default::default(),
    };
    AuthorizedProcessRequest {
        work_scope: Default::default(),
        authorized_action: AuthorizedAction {
            capability: IssuedCapability {
                capability_id: tetonic_domain::CapabilityId::new("cap_nov4"),
                session_id: action.session_id.clone(),
                run_id: action.run_id.clone(),
                task_id: action.task_id.clone(),
                attempt_id: action.attempt_id.clone(),
                agent_id: action.agent_id.clone(),
                action_kind: ActionKind::ExecuteProcess,
                canonical_parameter_digest: action.parameters.digest.clone(),
                workspace_version: None,
                data_classification: DataClass::RepositorySource,
                issuance_timestamp: now_secs,
                expiration: now_secs + 300,
                max_use_count: 1,
                current_use_count: 0,
                issuing_policy_version: "v2".into(),
                approval_record_id: None,
                revoked: false,
            },
            action,
        },
    }
}

#[tokio::test]
async fn worker_target_process_refuses() {
    let worker = WorkerId::new("w1");
    let broker = broker();
    broker.set_capability_registry(registry_with_eligible_infer_worker(&worker));
    let executor = Arc::new(SpyProcess::default());
    let outcome = broker
        .dispatch_sandboxed(
            test_shard_request("a_remote", Some(worker)),
            process_request(),
            executor.clone(),
        )
        .await;
    let Err(err) = outcome else {
        panic!("a process-class job aimed at a worker must be refused, not downgraded");
    };

    match err {
        ComputeBrokerError::WorkerTargetRefused {
            job_kind,
            worker_id,
        } => {
            assert_eq!(job_kind, "TestShard");
            assert_eq!(worker_id, "w1");
        }
        other => panic!("expected WorkerTargetRefused, got {other:?}"),
    }
    assert_eq!(
        executor.calls.load(Ordering::SeqCst),
        0,
        "refused dispatch must not reach any executor"
    );
    assert_eq!(broker.budgets().active_count(), 0);
}

/// The gate's local shortcut compares the target against `node_local` / `local` as strings.
/// A worker advertising one of those names must still be refused process work, or the
/// refusal is a naming convention rather than a structural rule (INV-EXEC-002).
#[tokio::test]
async fn worker_named_like_the_local_sentinel_still_refuses() {
    for name in ["node_local", "local", "LOCAL"] {
        let worker = WorkerId::new(name);
        let broker = broker();
        broker.set_capability_registry(registry_with_eligible_infer_worker(&worker));
        let executor = Arc::new(SpyProcess::default());
        let outcome = broker
            .dispatch_sandboxed(
                test_shard_request("a_sentinel", Some(worker)),
                process_request(),
                executor.clone(),
            )
            .await;

        match outcome {
            Err(ComputeBrokerError::WorkerTargetRefused { worker_id, .. }) => {
                assert_eq!(worker_id, name);
            }
            Err(other) => panic!("expected WorkerTargetRefused for {name}, got {other:?}"),
            Ok(_) => panic!("a worker named {name} bypassed the process refusal"),
        }
        assert_eq!(
            executor.calls.load(Ordering::SeqCst),
            0,
            "refused dispatch must not reach any executor"
        );
    }
}

#[tokio::test]
async fn local_target_process_still_dispatches() {
    let broker = broker();
    let executor = Arc::new(SpyProcess::default());
    let result = broker
        .dispatch_sandboxed(
            test_shard_request("a_local", None),
            process_request(),
            executor.clone(),
        )
        .await
        .expect("local process dispatch is the supported V3 path");

    assert!(result.success);
    assert_eq!(executor.calls.load(Ordering::SeqCst), 1);
    assert_eq!(broker.budgets().active_count(), 0);
}

#[test]
fn named_worker_embed_refused_when_ads_are_infer_only() {
    let worker = WorkerId::new("w1");
    let registry = registry_with_eligible_infer_worker(&worker);
    let now = Utc::now();
    let mut req = test_shard_request("a_embed", Some(worker));
    req.job_kind = JobKind::Embed;
    req.resource_request = profile_for(&JobKind::Embed).default_resources;
    req.priority = ComputePriority::Normal;
    let caps = registry.read().unwrap();
    match tetonic_broker::dispatch::revalidate_before_dispatch(&req, Some(&caps), now) {
        Err(ComputeBrokerError::WorkerTargetRefused {
            job_kind,
            worker_id,
        }) => {
            assert_eq!(job_kind, "Embed");
            assert_eq!(worker_id, "w1");
        }
        other => panic!("expected WorkerTargetRefused for Embed, got {other:?}"),
    }
}

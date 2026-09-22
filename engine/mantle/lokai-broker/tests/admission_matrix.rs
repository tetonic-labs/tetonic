//! M6-1 admission / budget / queue failure matrix.

#![allow(clippy::field_reassign_with_default)]

use chrono::{Duration, Utc};
use lokai_broker::*;
use lokai_domain::ids::{AttemptId, RunId, TaskId, WorkerId};
use lokai_domain::{
    ContentDigest, DataClass, PlacementReason, TraceContext, TrustPlacementDecision,
    VerificationPolicyReference,
};
use lokai_fabric_protocol::JobKind;
use lokai_run::RunSupervisor;
use std::sync::Arc;

fn test_supervisor() -> Arc<dyn RunSupervisor> {
    Arc::new(lokai_run::DurableRunSupervisor::new(None))
}

fn req(
    run: &str,
    task: &str,
    attempt: &str,
    priority: ComputePriority,
    resources: ResourceRequest,
) -> ComputeRequest {
    let now = Utc::now();
    ComputeRequest {
        run_id: RunId::new(run),
        task_id: TaskId::new(task),
        task_version: 1,
        attempt_id: AttemptId::new(attempt),
        job_kind: JobKind::Infer,
        input_artifacts: vec![],
        input_digest: ContentDigest::new("sha256:in"),
        workspace_version: None,
        data_class: DataClass::RepositorySource,
        placement_decision: PlacementDecisionReference {
            decision_id: "plc".into(),
            issued_at: now,
            expires_at: now + Duration::minutes(10),
            policy_epoch: 1,
            decision: TrustPlacementDecision::LocalOnly {
                reason: PlacementReason::CapabilityUnavailable,
            },
        },
        resource_request: resources,
        deadline: DeadlinePolicy {
            queue_deadline: now + Duration::minutes(5),
            execution_deadline: now + Duration::hours(1),
        },
        retry_policy: RetryPolicyReference::default(),
        verification_policy: VerificationPolicyReference {
            policy_id: "structural".into(),
        },
        priority,
        trace_context: TraceContext::default(),
        speculative: false,
        project_id: Some("proj".into()),
        target_worker_id: None,
        fallback_order: vec![],
        scheduler_decision_id: None,
    }
}

fn controller(limits: BudgetLimits, queue_limits: QueueLimits) -> HierarchicalAdmissionController {
    controller_with_fairness(limits, queue_limits, FairnessPolicy::default())
}

fn controller_with_fairness(
    limits: BudgetLimits,
    queue_limits: QueueLimits,
    fairness: FairnessPolicy,
) -> HierarchicalAdmissionController {
    let budgets = std::sync::Arc::new(HierarchicalBudgetLedger::new(limits));
    let queue = std::sync::Arc::new(QueueManager::new(queue_limits, fairness));
    HierarchicalAdmissionController::new(budgets, queue)
}

async fn evaluate(
    ctl: &HierarchicalAdmissionController,
    compute: ComputeRequest,
) -> AdmissionDecision {
    ctl.evaluate(AdmissionRequest {
        compute,
        run_canceled: false,
        task_state: None,
        expected_task_version: None,
        attempt_active: true,
        workspace_matches: true,
        artifacts_available: true,
        data_class_unchanged: true,
        now: Utc::now(),
    })
    .await
}

#[tokio::test]
async fn per_run_concurrency_limit() {
    let mut limits = BudgetLimits::default();
    limits.max_active_tasks_per_run = 1;
    let ctl = controller(limits, QueueLimits::default());
    let first = evaluate(
        &ctl,
        req(
            "r1",
            "t1",
            "a1",
            ComputePriority::Normal,
            ResourceRequest::infer_default(),
        ),
    )
    .await;
    assert!(matches!(first, AdmissionDecision::Admitted(_)));
    let second = evaluate(
        &ctl,
        req(
            "r1",
            "t2",
            "a2",
            ComputePriority::Normal,
            ResourceRequest::infer_default(),
        ),
    )
    .await;
    assert!(matches!(
        second,
        AdmissionDecision::Queued(_) | AdmissionDecision::Rejected(_)
    ));
}

#[tokio::test]
async fn per_worker_concurrency_limit() {
    let mut limits = BudgetLimits::default();
    limits.max_concurrent_inference = 1;
    let ctl = controller(limits, QueueLimits::default());
    let mut a = req(
        "r1",
        "t1",
        "a1",
        ComputePriority::Normal,
        ResourceRequest::infer_default(),
    );
    a.target_worker_id = Some(WorkerId::new("w1"));
    let mut b = req(
        "r2",
        "t2",
        "a2",
        ComputePriority::Normal,
        ResourceRequest::infer_default(),
    );
    b.target_worker_id = Some(WorkerId::new("w1"));
    assert!(matches!(
        evaluate(&ctl, a).await,
        AdmissionDecision::Admitted(_)
    ));
    assert!(matches!(
        evaluate(&ctl, b).await,
        AdmissionDecision::Queued(_) | AdmissionDecision::Rejected(_)
    ));
}

#[tokio::test]
async fn global_queue_saturation() {
    let mut limits = BudgetLimits::default();
    // Force reserve failure → queue path.
    limits.max_concurrent_inference = 0;
    let queue_limits = QueueLimits {
        global_max: 1,
        per_run_max: 10,
        per_worker_max: 10,
    };
    let ctl = controller_with_fairness(
        limits,
        queue_limits,
        FairnessPolicy {
            interactive_reserved_slots: 0,
            ..FairnessPolicy::default()
        },
    );
    let first = evaluate(
        &ctl,
        req(
            "r1",
            "t1",
            "a1",
            ComputePriority::Background,
            ResourceRequest::infer_default(),
        ),
    )
    .await;
    assert!(matches!(first, AdmissionDecision::Queued(_)));
    let second = evaluate(
        &ctl,
        req(
            "r1",
            "t2",
            "a2",
            ComputePriority::Background,
            ResourceRequest::infer_default(),
        ),
    )
    .await;
    assert!(matches!(second, AdmissionDecision::Rejected(_)));
}

#[tokio::test]
async fn token_budget_exhaustion() {
    let mut limits = BudgetLimits::default();
    limits.max_tokens_per_run = 100;
    let ctl = controller(limits, QueueLimits::default());
    let mut resources = ResourceRequest::infer_default();
    resources.total_token_budget = 80;
    assert!(matches!(
        evaluate(
            &ctl,
            req("r1", "t1", "a1", ComputePriority::Normal, resources.clone())
        )
        .await,
        AdmissionDecision::Admitted(_)
    ));
    resources.total_token_budget = 40;
    let second = evaluate(
        &ctl,
        req("r1", "t2", "a2", ComputePriority::Normal, resources),
    )
    .await;
    assert!(matches!(
        second,
        AdmissionDecision::Queued(_) | AdmissionDecision::Rejected(_)
    ));
}

#[tokio::test]
async fn ram_exhaustion() {
    let mut limits = BudgetLimits::default();
    limits.max_memory_bytes_global = 1024;
    limits.max_memory_bytes_per_run = 1024;
    limits.max_memory_bytes_per_attempt = 2048;
    let ctl = controller(limits, QueueLimits::default());
    let mut resources = ResourceRequest::infer_default();
    resources.memory_bytes = 512;
    assert!(matches!(
        evaluate(
            &ctl,
            req("r1", "t1", "a1", ComputePriority::Normal, resources.clone())
        )
        .await,
        AdmissionDecision::Admitted(_)
    ));
    resources.memory_bytes = 600;
    let second = evaluate(
        &ctl,
        req("r1", "t2", "a2", ComputePriority::Normal, resources),
    )
    .await;
    assert!(matches!(
        second,
        AdmissionDecision::Queued(_) | AdmissionDecision::Rejected(_)
    ));
}

#[tokio::test]
async fn vram_and_exclusive_gpu() {
    let mut limits = BudgetLimits::default();
    limits.max_vram_bytes_global = 8 * 1024;
    limits.max_vram_bytes_per_worker = 8 * 1024;
    limits.max_vram_bytes_per_attempt = 8 * 1024;
    let ctl = controller(limits, QueueLimits::default());
    let mut resources = ResourceRequest::infer_default();
    resources.vram_bytes = 4 * 1024;
    resources.gpu_devices = vec![GpuRequirement {
        device_index: Some(0),
        exclusive: true,
        min_vram_bytes: 4 * 1024,
    }];
    let mut a = req("r1", "t1", "a1", ComputePriority::Normal, resources.clone());
    a.target_worker_id = Some(WorkerId::new("gpu-host"));
    assert!(matches!(
        evaluate(&ctl, a).await,
        AdmissionDecision::Admitted(_)
    ));
    let mut b = req("r2", "t2", "a2", ComputePriority::Normal, resources);
    b.target_worker_id = Some(WorkerId::new("gpu-host"));
    let second = evaluate(&ctl, b).await;
    assert!(matches!(
        second,
        AdmissionDecision::Queued(_) | AdmissionDecision::Rejected(_)
    ));
}

#[tokio::test]
async fn cancellation_while_queued_and_running() {
    let mut limits = BudgetLimits::default();
    limits.max_concurrent_inference = 1;
    let budgets = std::sync::Arc::new(HierarchicalBudgetLedger::new(limits.clone()));
    let queue = std::sync::Arc::new(QueueManager::new(
        QueueLimits::default(),
        FairnessPolicy::default(),
    ));
    let ctl = HierarchicalAdmissionController::new(budgets.clone(), queue.clone());
    let first = evaluate(
        &ctl,
        req(
            "r1",
            "t1",
            "a1",
            ComputePriority::Normal,
            ResourceRequest::infer_default(),
        ),
    )
    .await;
    let AdmissionDecision::Admitted(res) = first else {
        panic!("expected admitted");
    };
    let queued = evaluate(
        &ctl,
        req(
            "r1",
            "t2",
            "a2",
            ComputePriority::Normal,
            ResourceRequest::infer_default(),
        ),
    )
    .await;
    assert!(matches!(queued, AdmissionDecision::Queued(_)));
    assert!(queue.remove(&AttemptId::new("a2")));
    budgets.release(&res.reservation_id, ReservationState::Canceled);
    assert_eq!(budgets.active_count(), 0);
}

#[tokio::test]
async fn reservation_expiration_and_restart_reconcile() {
    let mut limits = BudgetLimits::default();
    limits.reservation_ttl = std::time::Duration::from_millis(1);
    let budgets = HierarchicalBudgetLedger::new(limits);
    let now = Utc::now();
    let res = budgets
        .reserve(
            &RunId::new("r"),
            &TaskId::new("t"),
            &AttemptId::new("a"),
            None,
            ReservationTarget::Local,
            &ResourceRequest::infer_default(),
            false,
            now - Duration::seconds(10),
        )
        .unwrap();
    // Force expire by reconcile with now past expires_at
    let _ = budgets.transition(&res.reservation_id, ReservationState::Dispatched);
    let n = budgets.reconcile_uncertain(Utc::now());
    assert!(n >= 1);
    assert_eq!(budgets.active_count(), 0);
}

#[tokio::test]
async fn interactive_not_starved_by_background() {
    let queue = QueueManager::new(
        QueueLimits {
            global_max: 8,
            ..QueueLimits::default()
        },
        FairnessPolicy {
            max_consecutive_background: 1,
            interactive_reserved_slots: 2,
            age_boost_after_ms: 1,
        },
    );
    let now = Utc::now();
    for i in 0..3 {
        let r = req(
            "big",
            &format!("tb{i}"),
            &format!("ab{i}"),
            ComputePriority::Background,
            ResourceRequest::infer_default(),
        );
        queue.enqueue(&r, now).unwrap();
    }
    let interactive = req(
        "small",
        "ti",
        "ai",
        ComputePriority::Interactive,
        ResourceRequest::infer_default(),
    );
    queue.enqueue(&interactive, now).unwrap();
    // Interactive must be selected before background work despite later enqueue.
    let first = queue.pop_next(now).expect("queue non-empty");
    assert_eq!(first.0 .0, "ai");
    assert_eq!(first.3, ComputePriority::Interactive);
}

#[tokio::test]
async fn speculative_counts_against_budget() {
    let mut limits = BudgetLimits::default();
    limits.max_speculation_slots = 1;
    let ctl = controller(limits, QueueLimits::default());
    let mut a = req(
        "r1",
        "t1",
        "a1",
        ComputePriority::Normal,
        ResourceRequest::infer_default(),
    );
    a.speculative = true;
    let mut b = req(
        "r1",
        "t2",
        "a2",
        ComputePriority::Normal,
        ResourceRequest::infer_default(),
    );
    b.speculative = true;
    assert!(matches!(
        evaluate(&ctl, a).await,
        AdmissionDecision::Admitted(_)
    ));
    let second = evaluate(&ctl, b).await;
    assert!(matches!(
        second,
        AdmissionDecision::Queued(_) | AdmissionDecision::Rejected(_)
    ));
}

#[tokio::test]
async fn stale_placement_rejected() {
    let ctl = controller(BudgetLimits::default(), QueueLimits::default());
    let mut compute = req(
        "r1",
        "t1",
        "a1",
        ComputePriority::Normal,
        ResourceRequest::infer_default(),
    );
    compute.placement_decision.expires_at = Utc::now() - Duration::seconds(1);
    let decision = evaluate(&ctl, compute).await;
    match decision {
        AdmissionDecision::Rejected(r) => {
            assert_eq!(r.reason, AdmissionRejectionReason::PlacementExpired);
        }
        other => panic!("expected reject, got {other:?}"),
    }
}

#[tokio::test]
async fn durable_reservation_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let shared = lokai_memory::SharedStore::open(dir.path().join("lokai.db"), 1).unwrap();
    let persist = MemoryReservationStore::new(shared);
    let budgets = HierarchicalBudgetLedger::new(BudgetLimits::default());
    let res = budgets
        .reserve(
            &RunId::new("r"),
            &TaskId::new("t"),
            &AttemptId::new("a"),
            None,
            ReservationTarget::Local,
            &ResourceRequest::infer_default(),
            false,
            Utc::now(),
        )
        .unwrap();
    persist.upsert(&res).unwrap();
    let loaded = persist.get(&res.reservation_id.0).unwrap().unwrap();
    assert_eq!(loaded.attempt_id, res.attempt_id);
    persist
        .mark_released(&res.reservation_id.0, ReservationState::Released)
        .unwrap();
}

#[tokio::test]
async fn worker_loss_releases_capacity() {
    let budgets = HierarchicalBudgetLedger::new(BudgetLimits::default());
    let wid = WorkerId::new("w_lost");
    let res = budgets
        .reserve(
            &RunId::new("r"),
            &TaskId::new("t"),
            &AttemptId::new("a_dispatch"),
            None,
            ReservationTarget::Worker {
                worker_id: wid.clone(),
            },
            &ResourceRequest::infer_default(),
            false,
            Utc::now(),
        )
        .unwrap();
    let _ = budgets.transition(&res.reservation_id, ReservationState::Dispatched);
    assert_eq!(budgets.active_count(), 1);
    let n = budgets.release_for_worker(&wid, ReservationState::Canceled);
    assert_eq!(n, 1);
    assert_eq!(budgets.active_count(), 0);
}

#[tokio::test]
async fn estimate_below_actual_marks_over_budget() {
    let budgets = HierarchicalBudgetLedger::new(BudgetLimits::default());
    let mut small = ResourceRequest::infer_default();
    small.memory_bytes = 64 * 1024 * 1024;
    small.total_token_budget = 1_000;
    let res = budgets
        .reserve(
            &RunId::new("r"),
            &TaskId::new("t"),
            &AttemptId::new("a_over"),
            None,
            ReservationTarget::Local,
            &small,
            false,
            Utc::now(),
        )
        .unwrap();
    let _ = budgets.transition(&res.reservation_id, ReservationState::Running);
    let actual = ReservedResources {
        memory_bytes: small.memory_bytes * 8,
        vram_bytes: 0,
        process_slots: 0,
        inference_slots: 1,
        token_budget: small.total_token_budget * 10,
        temporary_storage_bytes: 0,
        exclusive_gpu_indices: vec![],
    };
    let err = budgets
        .enforce_actual_usage(&AttemptId::new("a_over"), &actual)
        .unwrap_err();
    assert!(matches!(err, BudgetReject::OverBudget));
    assert_eq!(budgets.active_count(), 0);
}

#[tokio::test]
async fn capability_expiry_blocks_dispatch_revalidation() {
    use lokai_broker::dispatch::revalidate_before_dispatch;
    use lokai_fabric_protocol::{CapabilityRegistry, WorkerCapabilities};

    let mut compute = req(
        "r1",
        "t1",
        "a1",
        ComputePriority::Normal,
        ResourceRequest::infer_default(),
    );
    compute.target_worker_id = Some(WorkerId::new("w1"));
    compute.placement_decision.decision = TrustPlacementDecision::Eligible {
        targets: vec![lokai_domain::EligibleTarget {
            worker_id: WorkerId::new("w1"),
            trust: lokai_domain::WorkerTrust::OwnerControlledEstate,
        }],
        required_verification: lokai_domain::VerificationRequirement::None,
    };
    let now = Utc::now();
    let wid = WorkerId::new("w1");
    let mut registry = CapabilityRegistry::new();
    let caps = WorkerCapabilities::legacy_infer_profile(
        wid.clone(),
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
    registry.upsert_validated(caps, &wid, 0, now).unwrap();
    let later = now + Duration::days(2);
    compute.placement_decision.expires_at = later + Duration::minutes(5);
    assert!(matches!(
        revalidate_before_dispatch(&compute, Some(&registry), later),
        Err(ComputeBrokerError::CapabilityStale { .. })
    ));
}

#[tokio::test]
async fn local_sandboxed_test_shard_via_broker() {
    use async_trait::async_trait;
    use lokai_domain::sinks::{
        AuthorizedProcessRequest, AuthorizedServiceRequest, ManagedProcessHandle,
        ManagedProcessResult, ProcessBroker, ProcessBrokerError,
    };
    use lokai_domain::{
        ActionId, ActionKind, AgentId, AuthorizedAction, IssuedCapability, ProposedAction,
        SessionId,
    };
    use std::sync::Arc;

    struct StubProcess;
    #[async_trait]
    impl ProcessBroker for StubProcess {
        async fn execute(
            &self,
            _request: AuthorizedProcessRequest,
        ) -> Result<ManagedProcessResult, ProcessBrokerError> {
            Ok(ManagedProcessResult {
                success: true,
                output: "ok".into(),
                execution_id: lokai_domain::ExecutionId::new("e1"),
            })
        }
        async fn start_service(
            &self,
            _request: AuthorizedServiceRequest,
        ) -> Result<Box<dyn ManagedProcessHandle>, ProcessBrokerError> {
            Err(ProcessBrokerError::ExecutionFailed("no".into()))
        }
    }

    let budgets = Arc::new(HierarchicalBudgetLedger::new(BudgetLimits::default()));
    let queue = Arc::new(QueueManager::new(
        QueueLimits::default(),
        FairnessPolicy::default(),
    ));
    let admission = Arc::new(HierarchicalAdmissionController::new(budgets, queue));
    let persist: Arc<dyn ReservationStore> = Arc::new(InMemoryReservationStore::default());
    let broker = DefaultComputeBroker::new(admission, persist, None, Some(test_supervisor()));
    let mut compute = req(
        "r1",
        "t_test",
        "a_test",
        ComputePriority::VerificationCritical,
        profile_for(&JobKind::TestShard).default_resources,
    );
    compute.job_kind = JobKind::TestShard;
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let action = ProposedAction {
        action_id: ActionId::new("a"),
        session_id: SessionId::new("s"),
        run_id: Some(RunId::new("r1")),
        task_id: Some(TaskId::new("t_test")),
        attempt_id: None,
        agent_id: Some(AgentId::new("a0")),
        workspace_version: None,
        data_class: DataClass::RepositorySource,
        kind: ActionKind::ExecuteProcess,
        parameters: lokai_domain::execution::CanonicalActionParameters {
            digest: "d".into(),
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
            process_class: Some(lokai_domain::execution::ProcessClass::BuildVerification),
            sandbox_profile: None,
            expected_output_limits: None,
            schema_version: 1,
            tool_arguments: None,
        },
        requested_capabilities: Default::default(),
        trace_context: Default::default(),
    };
    let process = AuthorizedProcessRequest {
        work_scope: Default::default(),
        authorized_action: AuthorizedAction {
            capability: IssuedCapability {
                capability_id: lokai_domain::CapabilityId::new("cap"),
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
    };
    let result = broker
        .dispatch_sandboxed(compute, process, Arc::new(StubProcess))
        .await
        .expect("test shard dispatch");
    assert!(result.success);
    assert_eq!(broker.budgets().active_count(), 0);
}

#[tokio::test]
async fn submit_without_supervisor_fails_closed() {
    let budgets = Arc::new(HierarchicalBudgetLedger::new(BudgetLimits::default()));
    let queue = Arc::new(QueueManager::new(
        QueueLimits::default(),
        FairnessPolicy::default(),
    ));
    let admission = Arc::new(HierarchicalAdmissionController::new(budgets, queue));
    let persist: Arc<dyn ReservationStore> = Arc::new(InMemoryReservationStore::default());
    let broker = DefaultComputeBroker::new(admission, persist, None, None);
    let err = broker
        .submit(req(
            "r1",
            "t1",
            "a1",
            ComputePriority::Normal,
            profile_for(&JobKind::Infer).default_resources,
        ))
        .await
        .expect_err("submit must fail closed without supervisor");
    assert!(matches!(err, ComputeBrokerError::SupervisorRequired));
}

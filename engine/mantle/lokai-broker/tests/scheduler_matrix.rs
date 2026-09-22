//! M6-2 scheduler / fallback / circuit / speculation matrix.

#![allow(clippy::field_reassign_with_default)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use lokai_broker::*;
use lokai_domain::ids::{AttemptId, RunId, TaskId, WorkerId};
use lokai_domain::{DataClass, SpeculationConfig, WorkerTrust};
use lokai_fabric_protocol::{CapabilityRegistry, JobKind, SandboxCapabilities, WorkerCapabilities};
use lokai_inference::{ChatRequest, FabricSnapshot, Message, NodeInfo, LOCAL_NODE_ID};
use lokai_run::RunSupervisor;

fn trust_map(snap: &FabricSnapshot, trust: WorkerTrust) -> HashMap<String, WorkerTrust> {
    snap.nodes
        .iter()
        .filter(|n| n.id != LOCAL_NODE_ID)
        .map(|n| (n.id.clone(), trust))
        .collect()
}

fn test_supervisor() -> Arc<dyn RunSupervisor> {
    Arc::new(lokai_run::DurableRunSupervisor::new(None))
}

fn local_fast() -> CandidateInputs {
    CandidateInputs {
        target: ExecutionTargetId::Local,
        admission_delay_ms: 20,
        queue_delay_ms: 20,
        connection_setup_ms: 0,
        input_transfer_ms: 0,
        cold_start_ms: 0,
        execution_ms: 2_000,
        result_transfer_ms: 0,
        verification_ms: 20,
        queue_depth: 0,
        transfer_bytes: 0,
        cold_start: false,
        uncertainty: UncertaintyModel {
            samples: 20,
            rolling_mae_ms: 10,
            cold_floor_ms: 25,
            ..UncertaintyModel::default()
        },
    }
}

fn remote_faster() -> CandidateInputs {
    CandidateInputs {
        target: ExecutionTargetId::Worker {
            worker_id: WorkerId::new("w_fast"),
        },
        admission_delay_ms: 20,
        queue_delay_ms: 20,
        connection_setup_ms: 30,
        input_transfer_ms: 40,
        cold_start_ms: 0,
        execution_ms: 400,
        result_transfer_ms: 20,
        verification_ms: 20,
        queue_depth: 0,
        // Above small-task threshold so min_remote_speedup (not _small) applies.
        transfer_bytes: 128 * 1024,
        cold_start: false,
        uncertainty: UncertaintyModel {
            samples: 20,
            rolling_mae_ms: 10,
            cold_floor_ms: 25,
            ..UncertaintyModel::default()
        },
    }
}

#[test]
fn remote_clearly_faster_selects_remote() {
    let cfg = SchedulerConfig {
        mode: SchedulerMode::Weighted,
        min_remote_speedup: 1.25,
        policy_penalty_ms: 50,
        ..SchedulerConfig::default()
    };
    let d = decide(DecideInput {
        run_id: RunId::new("r"),
        task_id: TaskId::new("t"),
        attempt_id: AttemptId::new("a"),
        data_class: DataClass::RepositorySource,
        local_only: false,
        candidates: vec![local_fast(), remote_faster()],
        circuits: None,
        config: &cfg,
    });
    assert!(matches!(
        d.selected_target,
        Some(ExecutionTargetId::Worker { .. })
    ));
    assert_eq!(d.reason, SchedulerReason::RemoteFaster);
}

#[test]
fn local_clearly_faster_stays_local() {
    let cfg = SchedulerConfig {
        mode: SchedulerMode::Weighted,
        ..SchedulerConfig::default()
    };
    let mut remote = remote_faster();
    remote.execution_ms = 5_000;
    remote.cold_start_ms = 2_000;
    let d = decide(DecideInput {
        run_id: RunId::new("r"),
        task_id: TaskId::new("t"),
        attempt_id: AttemptId::new("a"),
        data_class: DataClass::RepositorySource,
        local_only: false,
        candidates: vec![local_fast(), remote],
        circuits: None,
        config: &cfg,
    });
    assert_eq!(d.selected_target, Some(ExecutionTargetId::Local));
}

#[test]
fn unknown_worker_gets_large_uncertainty() {
    let u = UncertaintyModel {
        samples: 0,
        cold_floor_ms: 400,
        ..UncertaintyModel::default()
    };
    assert!(u.margin_ms(true) >= 400);
}

#[test]
fn secret_never_offloads() {
    let cfg = SchedulerConfig::default();
    let d = decide(DecideInput {
        run_id: RunId::new("r"),
        task_id: TaskId::new("t"),
        attempt_id: AttemptId::new("a"),
        data_class: DataClass::Secret,
        local_only: false,
        candidates: vec![local_fast(), remote_faster()],
        circuits: None,
        config: &cfg,
    });
    assert_eq!(d.selected_target, Some(ExecutionTargetId::Local));
    assert_eq!(d.reason, SchedulerReason::SecretLocalOnly);
}

#[test]
fn feature_flag_local_first() {
    let cfg = SchedulerConfig {
        mode: SchedulerMode::LocalFirst,
        ..SchedulerConfig::default()
    };
    let d = decide(DecideInput {
        run_id: RunId::new("r"),
        task_id: TaskId::new("t"),
        attempt_id: AttemptId::new("a"),
        data_class: DataClass::RepositorySource,
        local_only: false,
        candidates: vec![local_fast(), remote_faster()],
        circuits: None,
        config: &cfg,
    });
    assert_eq!(d.reason, SchedulerReason::FeatureFlagLocalFirst);
    assert_eq!(d.selected_target, Some(ExecutionTargetId::Local));
}

#[test]
fn large_transfer_keeps_local() {
    let cfg = SchedulerConfig {
        mode: SchedulerMode::Weighted,
        max_transfer_ratio: 0.01,
        min_remote_speedup: 1.01,
        ..SchedulerConfig::default()
    };
    let mut remote = remote_faster();
    remote.transfer_bytes = 50_000_000;
    remote.execution_ms = 10;
    let d = decide(DecideInput {
        run_id: RunId::new("r"),
        task_id: TaskId::new("t"),
        attempt_id: AttemptId::new("a"),
        data_class: DataClass::RepositorySource,
        local_only: false,
        candidates: vec![local_fast(), remote],
        circuits: None,
        config: &cfg,
    });
    assert_eq!(d.reason, SchedulerReason::TransferTooLarge);
}

#[test]
fn circuit_breaker_opens_and_half_opens() {
    let reg = CircuitBreakerRegistry::new(2, Duration::from_millis(20), 1);
    assert!(reg.allows_dispatch("w1"));
    reg.record_failure("w1");
    assert!(reg.allows_dispatch("w1"));
    reg.record_failure("w1");
    assert!(!reg.allows_dispatch("w1"));
    assert_eq!(reg.state("w1"), CircuitState::Open);
    std::thread::sleep(Duration::from_millis(25));
    assert_eq!(reg.state("w1"), CircuitState::HalfOpen);
    reg.record_success("w1");
    assert_eq!(reg.state("w1"), CircuitState::Closed);
}

#[test]
fn circuit_open_excludes_worker_from_decide() {
    let reg = CircuitBreakerRegistry::new(1, Duration::from_secs(60), 1);
    reg.record_failure("w_fast");
    let cfg = SchedulerConfig {
        mode: SchedulerMode::Weighted,
        min_remote_speedup: 1.1,
        ..SchedulerConfig::default()
    };
    let d = decide(DecideInput {
        run_id: RunId::new("r"),
        task_id: TaskId::new("t"),
        attempt_id: AttemptId::new("a"),
        data_class: DataClass::RepositorySource,
        local_only: false,
        candidates: vec![local_fast(), remote_faster()],
        circuits: Some(&reg),
        config: &cfg,
    });
    assert_eq!(d.selected_target, Some(ExecutionTargetId::Local));
}

#[test]
fn speculation_denied_for_test_shard() {
    let cfg = SpeculationConfig {
        allowed: true,
        max_simultaneous_attempts: 2,
        require_result_agreement: false,
    };
    assert!(speculation_allowed(&JobKind::TestShard, &cfg).is_err());
    assert!(speculation_allowed(&JobKind::Infer, &cfg).is_ok());
    let off = SpeculationConfig::default();
    assert!(speculation_allowed(&JobKind::Infer, &off).is_err());
}

#[tokio::test]
async fn speculation_launched_counts_budget() {
    let mut limits = BudgetLimits::default();
    limits.max_speculation_slots = 1;
    let budgets = Arc::new(HierarchicalBudgetLedger::new(limits));
    let queue = Arc::new(QueueManager::new(
        QueueLimits::default(),
        FairnessPolicy::default(),
    ));
    let admission = Arc::new(HierarchicalAdmissionController::new(budgets, queue));
    let persist: Arc<dyn ReservationStore> = Arc::new(InMemoryReservationStore::default());
    let broker = DefaultComputeBroker::new(admission, persist, None, Some(test_supervisor()));
    broker.set_speculation_config(SpeculationConfig {
        allowed: true,
        max_simultaneous_attempts: 2,
        require_result_agreement: false,
    });

    let now = Utc::now();
    let mut primary = ComputeRequest {
        run_id: RunId::new("r_spec"),
        task_id: TaskId::new("t1"),
        task_version: 1,
        attempt_id: AttemptId::new("a_primary"),
        job_kind: JobKind::Infer,
        input_artifacts: vec![],
        input_digest: lokai_domain::ContentDigest::new("d"),
        workspace_version: None,
        data_class: DataClass::RepositorySource,
        placement_decision: PlacementDecisionReference {
            decision_id: "plc".into(),
            issued_at: now,
            expires_at: now + chrono::Duration::minutes(5),
            policy_epoch: 0,
            decision: lokai_domain::TrustPlacementDecision::LocalOnly {
                reason: lokai_domain::PlacementReason::CapabilityUnavailable,
            },
        },
        resource_request: ResourceRequest::infer_default(),
        deadline: DeadlinePolicy::default(),
        retry_policy: RetryPolicyReference::default(),
        verification_policy: lokai_domain::VerificationPolicyReference {
            policy_id: "structural".into(),
        },
        priority: ComputePriority::Normal,
        trace_context: Default::default(),
        speculative: false,
        project_id: None,
        target_worker_id: None,
        fallback_order: vec!["local".into(), "w2".into()],
        scheduler_decision_id: None,
    };
    let mut speculative = primary.clone();
    speculative.attempt_id = AttemptId::new("a_spec");
    speculative.task_id = TaskId::new("t1b");
    speculative.speculative = true;

    assert!(matches!(
        broker.submit(primary.clone()).await.unwrap().status,
        ComputeStatus::Reserved
    ));
    assert!(matches!(
        broker.submit(speculative).await.unwrap().status,
        ComputeStatus::Reserved
    ));
    // Third speculative exceeds max_speculation_slots=1.
    primary.attempt_id = AttemptId::new("a_spec2");
    primary.task_id = TaskId::new("t2");
    primary.speculative = true;
    let third = broker.submit(primary).await.unwrap();
    assert!(
        matches!(
            third.status,
            ComputeStatus::Queued | ComputeStatus::Rejected { .. }
        ),
        "expected speculation budget to reject/queue third speculative admit, got {:?}",
        third.status
    );
}

#[test]
fn fallback_matrix_and_next() {
    assert_eq!(
        fallback_action(FallbackFailureClass::RemoteRejectBeforeStart),
        FallbackAction::NextEligibleOrLocal
    );
    assert_eq!(
        fallback_action(FallbackFailureClass::PermanentPolicy),
        FallbackAction::FailExplicit
    );
    let order = vec![
        ExecutionTargetId::Worker {
            worker_id: WorkerId::new("a"),
        },
        ExecutionTargetId::Local,
    ];
    let next = next_fallback(
        &order,
        &ExecutionTargetId::Worker {
            worker_id: WorkerId::new("a"),
        },
    );
    assert_eq!(next, Some(ExecutionTargetId::Local));
}

#[test]
fn secret_scan_failure_never_falls_back_to_another_target() {
    use lokai_inference::InferenceError;
    assert_eq!(
        fallback_action(classify_inference_error(
            &InferenceError::SecretScanFailed {
                reason: "scanner down".into(),
            }
        )),
        FallbackAction::FailExplicit
    );
    assert_eq!(
        fallback_action(classify_inference_error(
            &InferenceError::RemoteSecretDenied {
                reason: "pem-private-key".into(),
            }
        )),
        FallbackAction::FailExplicit
    );
}

#[test]
fn scheduler_decision_persists_without_payload() {
    let store = InMemorySchedulerStore::new();
    let cfg = SchedulerConfig::default();
    let d = decide(DecideInput {
        run_id: RunId::new("r"),
        task_id: TaskId::new("t"),
        attempt_id: AttemptId::new("a"),
        data_class: DataClass::RepositorySource,
        local_only: true,
        candidates: vec![local_fast()],
        circuits: None,
        config: &cfg,
    });
    store.upsert(&d).unwrap();
    let loaded = store.get(&d.decision_id.0).unwrap().unwrap();
    assert_eq!(loaded.decision_id.0, d.decision_id.0);
    assert!(!store.list_pending().unwrap().is_empty());
}

#[test]
fn queue_threshold_filters_candidate() {
    let cfg = SchedulerConfig {
        mode: SchedulerMode::Weighted,
        max_queue_depth: 1,
        min_remote_speedup: 1.1,
        ..SchedulerConfig::default()
    };
    let mut remote = remote_faster();
    remote.queue_depth = 50;
    let d = decide(DecideInput {
        run_id: RunId::new("r"),
        task_id: TaskId::new("t"),
        attempt_id: AttemptId::new("a"),
        data_class: DataClass::RepositorySource,
        local_only: false,
        candidates: vec![local_fast(), remote],
        circuits: None,
        config: &cfg,
    });
    assert_eq!(d.selected_target, Some(ExecutionTargetId::Local));
}

#[test]
fn quarantine_excludes_worker_from_schedule_infer_chat() {
    let now = Utc::now();
    let mut caps = CapabilityRegistry::new();
    let wid = WorkerId::new("w_quarantined");
    let mut wc = WorkerCapabilities::legacy_infer_profile(
        wid.clone(),
        "boot",
        1,
        0,
        &["qwen:7b".into()],
        &["qwen:7b".into()],
        8192,
        4096,
        0,
        0,
        2,
    );
    wc.sandbox = SandboxCapabilities {
        process_tree: lokai_fabric_protocol::ControlSupport::Enforced,
        ..SandboxCapabilities::default()
    };
    caps.upsert_validated(wc, &wid, 0, now).unwrap();
    assert!(caps.force_quarantine("w_quarantined"));

    let compute = {
        let now = Utc::now();
        ComputeRequest {
            run_id: RunId::new("r"),
            task_id: TaskId::new("t"),
            task_version: 1,
            attempt_id: AttemptId::new("a"),
            job_kind: JobKind::Infer,
            input_artifacts: vec![],
            input_digest: lokai_domain::ContentDigest::new("d"),
            workspace_version: None,
            data_class: DataClass::RepositorySource,
            placement_decision: PlacementDecisionReference {
                decision_id: "plc".into(),
                issued_at: now,
                expires_at: now + chrono::Duration::minutes(5),
                policy_epoch: 0,
                decision: lokai_domain::TrustPlacementDecision::LocalOnly {
                    reason: lokai_domain::PlacementReason::CapabilityUnavailable,
                },
            },
            resource_request: ResourceRequest::infer_default(),
            deadline: DeadlinePolicy::default(),
            retry_policy: RetryPolicyReference::default(),
            verification_policy: lokai_domain::VerificationPolicyReference {
                policy_id: "structural".into(),
            },
            priority: ComputePriority::Normal,
            trace_context: Default::default(),
            speculative: false,
            project_id: None,
            target_worker_id: None,
            fallback_order: vec![],
            scheduler_decision_id: None,
        }
    };
    let chat = ChatRequest {
        model: "qwen:7b".into(),
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
    let snap = FabricSnapshot {
        nodes: vec![
            NodeInfo {
                id: LOCAL_NODE_ID.into(),
                label: "local".into(),
                vram_total_mb: 0,
                vram_free_mb: 0,
                resident_models: vec!["qwen:7b".into()],
                queue_depth: 0,
                healthy: true,
                models_verified: true,
                capacity: None,
                legacy_v1_chat_only: false,
                negotiated_protocol_version: None,
            },
            NodeInfo {
                id: "w_quarantined".into(),
                label: "w_quarantined".into(),
                vram_total_mb: 0,
                vram_free_mb: 0,
                resident_models: vec!["qwen:7b".into()],
                queue_depth: 0,
                healthy: true,
                models_verified: true,
                capacity: None,
                legacy_v1_chat_only: false,
                negotiated_protocol_version: None,
            },
        ],
        effective_concurrency: 2,
        generated_at: now,
    };
    let circuits = CircuitBreakerRegistry::default();
    let cfg = SchedulerConfig {
        mode: SchedulerMode::Weighted,
        min_remote_speedup: 1.01,
        ..SchedulerConfig::default()
    };
    let decision = schedule_infer_chat(ScheduleInferChat {
        compute_req: &compute,
        snap: &snap,
        chat: &chat,
        circuits: &circuits,
        config: &cfg,
        caps: Some(&caps),
        calibration: None,
        worker_trust_by_node: trust_map(&snap, WorkerTrust::OwnerControlledEstate),
        policy_epoch: 0,
        project_policy: Default::default(),
    });
    assert!(
        !decision
            .candidates
            .iter()
            .any(|c| matches!(&c.target, ExecutionTargetId::Worker { worker_id } if worker_id.0 == "w_quarantined")),
        "quarantined worker must not be a candidate"
    );
    assert_eq!(
        decision.reason,
        SchedulerReason::Quarantined,
        "expected Quarantined when only remote was quarantined"
    );
}

#[test]
fn small_task_min_speedup_protects_local() {
    let cfg = SchedulerConfig {
        mode: SchedulerMode::Weighted,
        min_remote_speedup: 1.05,
        min_remote_speedup_small: 10.0,
        small_task_transfer_bytes: 64 * 1024,
        ..SchedulerConfig::default()
    };
    let mut local = local_fast();
    local.execution_ms = 80;
    let mut remote = remote_faster();
    remote.execution_ms = 50;
    remote.transfer_bytes = 100;
    let d = decide(DecideInput {
        run_id: RunId::new("r"),
        task_id: TaskId::new("t"),
        attempt_id: AttemptId::new("a"),
        data_class: DataClass::RepositorySource,
        local_only: false,
        candidates: vec![local, remote],
        circuits: None,
        config: &cfg,
    });
    assert!(matches!(
        d.reason,
        SchedulerReason::MinSpeedupNotMet
            | SchedulerReason::LocalFaster
            | SchedulerReason::EqualWithUncertainty
    ));
    assert_eq!(d.selected_target, Some(ExecutionTargetId::Local));
}

#[test]
fn equal_estimate_with_uncertainty_prefers_local() {
    let cfg = SchedulerConfig {
        mode: SchedulerMode::Weighted,
        min_remote_speedup: 1.01,
        policy_penalty_ms: 0,
        ..SchedulerConfig::default()
    };
    let mut local = local_fast();
    local.execution_ms = 500;
    local.uncertainty = UncertaintyModel {
        samples: 20,
        rolling_mae_ms: 80,
        cold_floor_ms: 25,
        ..UncertaintyModel::default()
    };
    let mut remote = remote_faster();
    remote.execution_ms = 500;
    remote.connection_setup_ms = 0;
    remote.input_transfer_ms = 0;
    remote.result_transfer_ms = 0;
    remote.transfer_bytes = 128 * 1024;
    remote.uncertainty = UncertaintyModel {
        samples: 20,
        rolling_mae_ms: 80,
        cold_floor_ms: 25,
        ..UncertaintyModel::default()
    };
    let d = decide(DecideInput {
        run_id: RunId::new("r"),
        task_id: TaskId::new("t"),
        attempt_id: AttemptId::new("a"),
        data_class: DataClass::RepositorySource,
        local_only: false,
        candidates: vec![local, remote],
        circuits: None,
        config: &cfg,
    });
    assert_eq!(d.selected_target, Some(ExecutionTargetId::Local));
    assert!(matches!(
        d.reason,
        SchedulerReason::EqualWithUncertainty
            | SchedulerReason::LocalFaster
            | SchedulerReason::MinSpeedupNotMet
            | SchedulerReason::TransferTooLarge
    ));
}

#[test]
fn warm_local_beats_cold_remote() {
    let cfg = SchedulerConfig {
        mode: SchedulerMode::Weighted,
        min_remote_speedup: 1.1,
        ..SchedulerConfig::default()
    };
    let mut local = local_fast();
    local.execution_ms = 400;
    local.cold_start = false;
    let mut remote = remote_faster();
    remote.execution_ms = 200;
    remote.cold_start = true;
    remote.cold_start_ms = 2_000;
    remote.uncertainty = UncertaintyModel {
        samples: 0,
        cold_floor_ms: 400,
        cold_model_extra_ms: 500,
        ..UncertaintyModel::default()
    };
    let d = decide(DecideInput {
        run_id: RunId::new("r"),
        task_id: TaskId::new("t"),
        attempt_id: AttemptId::new("a"),
        data_class: DataClass::RepositorySource,
        local_only: false,
        candidates: vec![local, remote],
        circuits: None,
        config: &cfg,
    });
    assert_eq!(d.selected_target, Some(ExecutionTargetId::Local));
}

#[test]
fn artifact_cached_remotely_reduces_transfer() {
    let cfg = SchedulerConfig {
        mode: SchedulerMode::Weighted,
        min_remote_speedup: 1.2,
        policy_penalty_ms: 50,
        ..SchedulerConfig::default()
    };
    let mut local = local_fast();
    local.execution_ms = 2_500;
    let mut remote = remote_faster();
    remote.execution_ms = 500;
    remote.input_transfer_ms = 0; // artifact already cached
    remote.transfer_bytes = 128 * 1024;
    let d = decide(DecideInput {
        run_id: RunId::new("r"),
        task_id: TaskId::new("t"),
        attempt_id: AttemptId::new("a"),
        data_class: DataClass::RepositorySource,
        local_only: false,
        candidates: vec![local, remote],
        circuits: None,
        config: &cfg,
    });
    assert!(matches!(
        d.selected_target,
        Some(ExecutionTargetId::Worker { .. })
    ));
}

#[test]
fn remote_reject_and_transport_failure_map_to_next_eligible() {
    assert_eq!(
        fallback_action(FallbackFailureClass::RemoteRejectBeforeStart),
        FallbackAction::NextEligibleOrLocal
    );
    assert_eq!(
        fallback_action(FallbackFailureClass::TransientTransport),
        FallbackAction::RetryThenNextThenLocal { max_same_worker: 1 }
    );
    let err = lokai_inference::InferenceError::Provider("connection reset".into());
    let class = classify_inference_error(&err);
    assert!(matches!(
        class,
        FallbackFailureClass::WorkerLoss
            | FallbackFailureClass::TransientTransport
            | FallbackFailureClass::RemoteRejectBeforeStart
    ));
    let transport = lokai_inference::InferenceError::Provider("transport timeout".into());
    assert_eq!(
        classify_inference_error(&transport),
        FallbackFailureClass::TransientTransport
    );
}

#[test]
fn worker_loss_lists_continue_attempts() {
    let persist: Arc<dyn ReservationStore> = Arc::new(InMemoryReservationStore::default());
    let budgets = Arc::new(HierarchicalBudgetLedger::new(BudgetLimits::default()));
    let queue = Arc::new(QueueManager::new(
        QueueLimits::default(),
        FairnessPolicy::default(),
    ));
    let admission = Arc::new(HierarchicalAdmissionController::new(budgets, queue));
    let broker = DefaultComputeBroker::new(admission, persist, None, None);
    // Without in-flight attempts targeting the worker, continue list is empty.
    let continue_ids = broker.on_worker_lost("w_missing");
    assert!(continue_ids.is_empty());
}

#[test]
fn worker_loss_and_hop_skip_keep_failover_open() {
    // In-flight chat_with_failover must continue after worker loss / hop skip —
    // never FailExplicit (daemon no longer discards a parallel continue).
    assert_eq!(
        fallback_action(FallbackFailureClass::WorkerLoss),
        FallbackAction::NewAttemptAfterLeaseExpire
    );
    assert_ne!(
        fallback_action(FallbackFailureClass::WorkerLoss),
        FallbackAction::FailExplicit
    );
    let lost = lokai_inference::InferenceError::Preempted {
        node_id: "w1".into(),
    };
    assert_eq!(
        classify_inference_error(&lost),
        FallbackFailureClass::WorkerLoss
    );
    let hop_skip = lokai_inference::InferenceError::Provider(
        "failover hop ineligible: worker_quarantined".into(),
    );
    assert_eq!(
        classify_inference_error(&hop_skip),
        FallbackFailureClass::CapabilityExpired
    );
    assert_eq!(
        fallback_action(FallbackFailureClass::CapabilityExpired),
        FallbackAction::NextEligibleOrLocal
    );
}

#[test]
fn secret_under_local_overload_stays_local() {
    let cfg = SchedulerConfig {
        mode: SchedulerMode::Weighted,
        min_remote_speedup: 1.01,
        ..SchedulerConfig::default()
    };
    let mut local = local_fast();
    local.queue_delay_ms = 50_000;
    local.execution_ms = 10_000;
    let d = decide(DecideInput {
        run_id: RunId::new("r"),
        task_id: TaskId::new("t"),
        attempt_id: AttemptId::new("a"),
        data_class: DataClass::Secret,
        local_only: false,
        candidates: vec![local, remote_faster()],
        circuits: None,
        config: &cfg,
    });
    assert_eq!(d.selected_target, Some(ExecutionTargetId::Local));
    assert_eq!(d.reason, SchedulerReason::SecretLocalOnly);
}

#[test]
fn durable_scheduler_decision_round_trip() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let db = dir.path().join("sched.db");
    let shared = lokai_memory::SharedStore::open(&db, 1).expect("open");
    let mem = MemorySchedulerStore::new(shared);
    let cfg = SchedulerConfig::default();
    let mut d = decide(DecideInput {
        run_id: RunId::new("r"),
        task_id: TaskId::new("t"),
        attempt_id: AttemptId::new("a"),
        data_class: DataClass::RepositorySource,
        local_only: true,
        candidates: vec![local_fast()],
        circuits: None,
        config: &cfg,
    });
    d.pending = true;
    mem.upsert(&d).unwrap();
    let loaded = mem.get(&d.decision_id.0).unwrap().unwrap();
    assert_eq!(loaded.decision_id.0, d.decision_id.0);
    assert!(loaded.pending);
    assert!(!mem.list_pending().unwrap().is_empty());
    // Restart reconcile: mark pending false
    let mut cleared = loaded;
    cleared.pending = false;
    mem.upsert(&cleared).unwrap();
    assert!(mem.list_pending().unwrap().is_empty());
}

#[test]
fn placement_denied_external_trust_excludes_remote() {
    let now = Utc::now();
    let mut caps = CapabilityRegistry::new();
    let wid = WorkerId::new("w_ext");
    let mut wc = WorkerCapabilities::legacy_infer_profile(
        wid.clone(),
        "boot",
        1,
        0,
        &["qwen:7b".into()],
        &["qwen:7b".into()],
        8192,
        4096,
        0,
        0,
        2,
    );
    wc.sandbox = SandboxCapabilities {
        process_tree: lokai_fabric_protocol::ControlSupport::Enforced,
        ..SandboxCapabilities::default()
    };
    caps.upsert_validated(wc, &wid, 0, now).unwrap();

    let compute = {
        let now = Utc::now();
        ComputeRequest {
            run_id: RunId::new("r"),
            task_id: TaskId::new("t"),
            task_version: 1,
            attempt_id: AttemptId::new("a"),
            job_kind: JobKind::Infer,
            input_artifacts: vec![],
            input_digest: lokai_domain::ContentDigest::new("d"),
            workspace_version: None,
            data_class: DataClass::RepositorySource,
            placement_decision: PlacementDecisionReference {
                decision_id: "plc".into(),
                issued_at: now,
                expires_at: now + chrono::Duration::minutes(5),
                policy_epoch: 0,
                decision: lokai_domain::TrustPlacementDecision::LocalOnly {
                    reason: lokai_domain::PlacementReason::CapabilityUnavailable,
                },
            },
            resource_request: ResourceRequest::infer_default(),
            deadline: DeadlinePolicy::default(),
            retry_policy: RetryPolicyReference::default(),
            verification_policy: lokai_domain::VerificationPolicyReference {
                policy_id: "structural".into(),
            },
            priority: ComputePriority::Normal,
            trace_context: Default::default(),
            speculative: false,
            project_id: None,
            target_worker_id: None,
            fallback_order: vec![],
            scheduler_decision_id: None,
        }
    };
    let chat = ChatRequest {
        model: "qwen:7b".into(),
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
    let snap = FabricSnapshot {
        nodes: vec![
            NodeInfo {
                id: LOCAL_NODE_ID.into(),
                label: "local".into(),
                vram_total_mb: 0,
                vram_free_mb: 0,
                resident_models: vec!["qwen:7b".into()],
                queue_depth: 0,
                healthy: true,
                models_verified: true,
                capacity: None,
                legacy_v1_chat_only: false,
                negotiated_protocol_version: None,
            },
            NodeInfo {
                id: "w_ext".into(),
                label: "w_ext".into(),
                vram_total_mb: 0,
                vram_free_mb: 0,
                resident_models: vec!["qwen:7b".into()],
                queue_depth: 0,
                healthy: true,
                models_verified: true,
                capacity: None,
                legacy_v1_chat_only: false,
                negotiated_protocol_version: None,
            },
        ],
        effective_concurrency: 2,
        generated_at: now,
    };
    let circuits = CircuitBreakerRegistry::default();
    let cfg = SchedulerConfig {
        mode: SchedulerMode::Weighted,
        min_remote_speedup: 1.01,
        ..SchedulerConfig::default()
    };
    let decision = schedule_infer_chat(ScheduleInferChat {
        compute_req: &compute,
        snap: &snap,
        chat: &chat,
        circuits: &circuits,
        config: &cfg,
        caps: Some(&caps),
        calibration: None,
        worker_trust_by_node: trust_map(&snap, WorkerTrust::ExternalUntrusted),
        policy_epoch: 0,
        project_policy: Default::default(),
    });
    assert!(
        !decision
            .candidates
            .iter()
            .any(|c| matches!(&c.target, ExecutionTargetId::Worker { .. })),
        "external untrusted must not yield remote candidates for repository source"
    );
}

#[test]
fn broker_speculation_race_cancels_loser_late_result() {
    // Production session naming from speculative_race_sessions + AJR cancel_session
    // (same call race_speculative_infer uses). Fabric accept calls reg.validate.
    use lokai_inference::ActiveJobRegistry;

    let reg = ActiveJobRegistry::new();
    let base = "sess_race_e2e";
    let (primary_sess, spec_sess) = speculative_race_sessions(base);
    assert_eq!(primary_sess, "sess_race_e2e:primary");
    assert_eq!(spec_sess, "sess_race_e2e:spec");

    let primary_job = "job_primary_race";
    let spec_job = "job_spec_race";
    let primary = reg.begin_attempt(primary_job, Some(&primary_sess));
    let speculative = reg.begin_attempt_with_id(spec_job, Some(&spec_sess), "att_leased_loser");
    assert_eq!(speculative, "att_leased_loser");
    assert!(reg.validate(primary_job, &primary).is_ok());
    // Winner settled → cancel loser session (production race path).
    reg.cancel_session(&spec_sess);
    assert!(
        reg.validate(spec_job, "att_leased_loser").is_err(),
        "late speculative loser must be rejected after cancel_session"
    );
    // Winner remains valid once; duplicate late winner also rejected.
    assert!(reg.validate(primary_job, &primary).is_err());
}

#[test]
fn tail_latency_triggers_speculation_predicate() {
    let mut remote = remote_faster();
    remote.uncertainty = UncertaintyModel {
        samples: 2,
        rolling_mae_ms: 400,
        cold_floor_ms: 300,
        cold_model_extra_ms: 500,
    };
    let cfg = SchedulerConfig {
        mode: SchedulerMode::Weighted,
        min_remote_speedup: 1.1,
        policy_penalty_ms: 0,
        ..SchedulerConfig::default()
    };
    let d = decide(DecideInput {
        run_id: RunId::new("r_tail"),
        task_id: TaskId::new("t"),
        attempt_id: AttemptId::new("a"),
        data_class: DataClass::RepositorySource,
        local_only: false,
        candidates: vec![local_fast(), remote],
        circuits: None,
        config: &cfg,
    });
    if matches!(d.selected_target, Some(ExecutionTargetId::Worker { .. })) {
        assert!(
            should_speculate_for_tail(&d),
            "high-uncertainty remote selection should trigger tail speculation"
        );
    } else {
        // Local preferred — still exercise the helper on a synthetic remote pick.
        let mut synthetic = d.clone();
        synthetic.selected_target = Some(ExecutionTargetId::Worker {
            worker_id: WorkerId::new("w_fast"),
        });
        synthetic.uncertainty_margin_ms = 200;
        assert!(should_speculate_for_tail(&synthetic));
    }
}

#[test]
fn trust_downgrade_hop_skips_via_revalidate_hop_placement() {
    let mut reg = CapabilityRegistry::new();
    let wid = WorkerId::new("w1");
    let now = Utc::now();
    let mut caps = WorkerCapabilities::legacy_infer_profile(
        wid.clone(),
        "boot",
        1,
        0,
        &["qwen:7b".into()],
        &["qwen:7b".into()],
        8192,
        4096,
        0,
        0,
        2,
    );
    caps.generated_at = now;
    caps.valid_until = now + chrono::Duration::hours(1);
    reg.upsert_validated(caps, &wid, 0, now).unwrap();

    let mut compute = {
        let now = Utc::now();
        ComputeRequest {
            run_id: RunId::new("r"),
            task_id: TaskId::new("t"),
            task_version: 1,
            attempt_id: AttemptId::new("a"),
            job_kind: JobKind::Infer,
            input_artifacts: vec![],
            input_digest: lokai_domain::ContentDigest::new("d"),
            workspace_version: None,
            data_class: DataClass::RepositorySource,
            placement_decision: PlacementDecisionReference {
                decision_id: "p".into(),
                issued_at: now,
                expires_at: now + chrono::Duration::minutes(5),
                policy_epoch: 0,
                decision: lokai_domain::TrustPlacementDecision::LocalOnly {
                    reason: lokai_domain::PlacementReason::CapabilityUnavailable,
                },
            },
            resource_request: ResourceRequest::infer_default(),
            deadline: DeadlinePolicy::default(),
            retry_policy: RetryPolicyReference::default(),
            verification_policy: lokai_domain::VerificationPolicyReference {
                policy_id: "structural".into(),
            },
            priority: ComputePriority::Normal,
            trace_context: Default::default(),
            speculative: false,
            project_id: None,
            target_worker_id: None,
            fallback_order: vec![],
            scheduler_decision_id: None,
        }
    };
    let chat = ChatRequest {
        model: "qwen:7b".into(),
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
    let target = ExecutionTargetId::Worker {
        worker_id: WorkerId::new("w1"),
    };
    let skip = revalidate_hop_placement(
        &mut compute,
        &chat,
        &target,
        Some(&reg),
        WorkerTrust::ExternalUntrusted,
        Default::default(),
    );
    assert!(matches!(skip, HopPlacementOutcome::Skip { .. }));
}

#[test]
fn capability_expire_excludes_from_schedule_infer_chat() {
    let now = Utc::now();
    let mut caps = CapabilityRegistry::new();
    let wid = WorkerId::new("w_exp");
    let mut wc = WorkerCapabilities::legacy_infer_profile(
        wid.clone(),
        "boot",
        1,
        0,
        &["qwen:7b".into()],
        &["qwen:7b".into()],
        8192,
        4096,
        0,
        0,
        2,
    );
    wc.generated_at = now - chrono::Duration::hours(2);
    wc.valid_until = now - chrono::Duration::minutes(1);
    caps.upsert_validated(wc, &wid, 0, now - chrono::Duration::hours(2))
        .unwrap();

    let compute = {
        let now = Utc::now();
        ComputeRequest {
            run_id: RunId::new("r"),
            task_id: TaskId::new("t"),
            task_version: 1,
            attempt_id: AttemptId::new("a"),
            job_kind: JobKind::Infer,
            input_artifacts: vec![],
            input_digest: lokai_domain::ContentDigest::new("d"),
            workspace_version: None,
            data_class: DataClass::RepositorySource,
            placement_decision: PlacementDecisionReference {
                decision_id: "p".into(),
                issued_at: now,
                expires_at: now + chrono::Duration::minutes(5),
                policy_epoch: 0,
                decision: lokai_domain::TrustPlacementDecision::LocalOnly {
                    reason: lokai_domain::PlacementReason::CapabilityUnavailable,
                },
            },
            resource_request: ResourceRequest::infer_default(),
            deadline: DeadlinePolicy::default(),
            retry_policy: RetryPolicyReference::default(),
            verification_policy: lokai_domain::VerificationPolicyReference {
                policy_id: "structural".into(),
            },
            priority: ComputePriority::Normal,
            trace_context: Default::default(),
            speculative: false,
            project_id: None,
            target_worker_id: None,
            fallback_order: vec![],
            scheduler_decision_id: None,
        }
    };
    let chat = ChatRequest {
        model: "qwen:7b".into(),
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
    let snap = FabricSnapshot {
        nodes: vec![NodeInfo {
            id: "w_exp".into(),
            label: "w_exp".into(),
            vram_total_mb: 8192,
            vram_free_mb: 4096,
            resident_models: vec!["qwen:7b".into()],
            queue_depth: 0,
            healthy: true,
            models_verified: true,
            capacity: None,
            legacy_v1_chat_only: false,
            negotiated_protocol_version: None,
        }],
        effective_concurrency: 1,
        generated_at: now,
    };
    let circuits = CircuitBreakerRegistry::default();
    let cfg = SchedulerConfig::default();
    let decision = schedule_infer_chat(ScheduleInferChat {
        compute_req: &compute,
        snap: &snap,
        chat: &chat,
        circuits: &circuits,
        config: &cfg,
        caps: Some(&caps),
        calibration: None,
        worker_trust_by_node: trust_map(&snap, WorkerTrust::OwnerControlledEstate),
        policy_epoch: 0,
        project_policy: Default::default(),
    });
    assert!(
        !decision
            .candidates
            .iter()
            .any(|c| matches!(&c.target, ExecutionTargetId::Worker { worker_id } if worker_id.0 == "w_exp")),
        "expired capability must exclude remote from schedule"
    );
}

//! Deterministic M6-2 designated-workload benches (25% / ≤5%).
//!
//! Compares coordinator-modeled **observed** completion under weighted scheduling
//! vs `LOKAI_SCHEDULER=local_first` using ground-truth CandidateInputs component
//! sums (no uncertainty margin, no wall-clock noise, no live fabric).

use tetonic_broker::*;
use tetonic_domain::ids::{AttemptId, RunId, TaskId, WorkerId};
use tetonic_domain::DataClass;

/// Ground-truth coordinator-observed finish: sum of delay components, no margin.
fn observed_finish_ms(c: &CandidateInputs) -> u64 {
    c.admission_delay_ms
        .saturating_add(c.queue_delay_ms)
        .saturating_add(c.connection_setup_ms)
        .saturating_add(c.input_transfer_ms)
        .saturating_add(c.cold_start_ms)
        .saturating_add(c.execution_ms)
        .saturating_add(c.result_transfer_ms)
        .saturating_add(c.verification_ms)
}

fn parallel_task(i: usize) -> (CandidateInputs, CandidateInputs) {
    let local = CandidateInputs {
        target: ExecutionTargetId::Local,
        admission_delay_ms: 10,
        queue_delay_ms: 50 + (i as u64 % 5) * 20,
        connection_setup_ms: 0,
        input_transfer_ms: 0,
        cold_start_ms: 0,
        execution_ms: 3_000 + (i as u64 % 3) * 500,
        result_transfer_ms: 0,
        verification_ms: 20,
        queue_depth: 0,
        transfer_bytes: 0,
        cold_start: false,
        uncertainty: UncertaintyModel {
            samples: 30,
            rolling_mae_ms: 20,
            cold_floor_ms: 25,
            ..UncertaintyModel::default()
        },
    };
    let remote = CandidateInputs {
        target: ExecutionTargetId::Worker {
            worker_id: WorkerId::new(format!("w_par_{i}")),
        },
        admission_delay_ms: 10,
        queue_delay_ms: 20,
        connection_setup_ms: 25,
        input_transfer_ms: 40,
        cold_start_ms: 0,
        execution_ms: 700 + (i as u64 % 3) * 50,
        result_transfer_ms: 30,
        verification_ms: 20,
        queue_depth: 0,
        transfer_bytes: 128 * 1024,
        cold_start: false,
        uncertainty: UncertaintyModel {
            samples: 30,
            rolling_mae_ms: 20,
            cold_floor_ms: 25,
            ..UncertaintyModel::default()
        },
    };
    (local, remote)
}

fn small_local_task(i: usize) -> (CandidateInputs, CandidateInputs) {
    let local = CandidateInputs {
        target: ExecutionTargetId::Local,
        admission_delay_ms: 2,
        queue_delay_ms: 2,
        connection_setup_ms: 0,
        input_transfer_ms: 0,
        cold_start_ms: 0,
        execution_ms: 80 + (i as u64 % 4) * 10,
        result_transfer_ms: 0,
        verification_ms: 2,
        queue_depth: 0,
        transfer_bytes: 0,
        cold_start: false,
        uncertainty: UncertaintyModel {
            samples: 40,
            rolling_mae_ms: 5,
            cold_floor_ms: 25,
            ..UncertaintyModel::default()
        },
    };
    // Remote looks slightly faster raw but fails min-speedup / transfer policy.
    let remote = CandidateInputs {
        target: ExecutionTargetId::Worker {
            worker_id: WorkerId::new(format!("w_small_{i}")),
        },
        admission_delay_ms: 5,
        queue_delay_ms: 5,
        connection_setup_ms: 20,
        input_transfer_ms: 30,
        cold_start_ms: 0,
        execution_ms: 40,
        result_transfer_ms: 20,
        verification_ms: 5,
        queue_depth: 0,
        transfer_bytes: 8 * 1024,
        cold_start: false,
        uncertainty: UncertaintyModel {
            samples: 40,
            rolling_mae_ms: 5,
            cold_floor_ms: 25,
            ..UncertaintyModel::default()
        },
    };
    (local, remote)
}

fn suite_observed_total_ms(
    mode: SchedulerMode,
    tasks: &[(CandidateInputs, CandidateInputs)],
) -> u64 {
    let cfg = SchedulerConfig {
        mode,
        min_remote_speedup: 1.25,
        min_remote_speedup_small: 1.5,
        policy_penalty_ms: 50,
        ..SchedulerConfig::default()
    };
    let mut total = 0u64;
    for (i, (local, remote)) in tasks.iter().enumerate() {
        let candidates = vec![local.clone(), remote.clone()];
        let d = decide(DecideInput {
            run_id: RunId::new(format!("r{i}")),
            task_id: TaskId::new(format!("t{i}")),
            attempt_id: AttemptId::new(format!("a{i}")),
            data_class: DataClass::RepositorySource,
            local_only: false,
            candidates: candidates.clone(),
            circuits: None,
            config: &cfg,
        });
        let selected = d.selected_target.as_ref().expect("must select a target");
        let observed = candidates
            .iter()
            .find(|c| &c.target == selected)
            .map(observed_finish_ms)
            .unwrap_or(u64::MAX);
        total = total.saturating_add(observed);
    }
    total
}

#[test]
fn designated_parallel_workload_improves_at_least_25_percent() {
    let tasks: Vec<_> = (0..12).map(parallel_task).collect();
    let local_first = suite_observed_total_ms(SchedulerMode::LocalFirst, &tasks);
    let weighted = suite_observed_total_ms(SchedulerMode::Weighted, &tasks);
    assert!(
        local_first > 0 && weighted > 0,
        "totals must be positive: local_first={local_first} weighted={weighted}"
    );
    // weighted must be ≤ 75% of local_first (≥25% improvement on observed finish).
    let threshold = (local_first as f64) * 0.75;
    assert!(
        (weighted as f64) <= threshold,
        "parallel suite: weighted={weighted} local_first={local_first} (need ≤{threshold:.0})"
    );
}

#[test]
fn small_local_workload_regresses_at_most_5_percent() {
    let tasks: Vec<_> = (0..20).map(small_local_task).collect();
    let local_first = suite_observed_total_ms(SchedulerMode::LocalFirst, &tasks);
    let weighted = suite_observed_total_ms(SchedulerMode::Weighted, &tasks);
    assert!(
        local_first > 0 && weighted > 0,
        "totals must be positive: local_first={local_first} weighted={weighted}"
    );
    // weighted at most 5% slower than local_first on observed finish.
    let ceiling = (local_first as f64) * 1.05;
    assert!(
        (weighted as f64) <= ceiling,
        "small suite: weighted={weighted} local_first={local_first} (need ≤{ceiling:.0})"
    );
}

//! M6-3 timing / sampling / storage acceptance matrix.

use std::thread;
use std::time::Duration;

use lokai_telemetry::{
    admit_trace_write, critical_path_ms, format_critical_path_report, is_safe_metric_label,
    retention_for_outcome, saturating_ms_between, CoordinatorObservedTiming, RetentionClass,
    TraceSampler, TraceStorageBudget, WorkerLocalDurations,
};

#[test]
fn coordinator_e2e_without_worker_spans() {
    let mut c = CoordinatorObservedTiming::start("trace_a", Some("sched_a".into()));
    c.mark_admitted();
    thread::sleep(Duration::from_millis(2));
    c.mark_dispatched();
    c.mark_result_received();
    c.mark_verified();
    c.mark_accepted();
    let report = critical_path_ms(&c, None);
    assert!(report.missing_worker_spans);
    assert!(report.e2e_ms >= 2);
    assert!(report.segments.queue_wait_ms.is_some());
    assert!(report.segments.verification_ms.is_some());
}

#[test]
fn separate_stage_durations_with_worker_overlay() {
    let mut c = CoordinatorObservedTiming::start("trace_b", None);
    c.mark_admitted();
    c.mark_dispatched();
    c.transfer.send_ms = Some(11);
    c.transfer.recv_ms = Some(13);
    c.mark_result_received();
    c.mark_verified();
    c.mark_accepted();
    let worker = WorkerLocalDurations {
        queue_ms: Some(4),
        execute_ms: Some(55),
        ..Default::default()
    };
    let segs = c.segments_ms(Some(&worker));
    assert_eq!(segs.transfer_input_ms, Some(11));
    assert_eq!(segs.transfer_output_ms, Some(13));
    assert_eq!(segs.execution_ms, Some(55));
    assert!(segs.e2e_ms.is_some());
}

#[test]
fn clock_skew_style_instant_order_never_negative() {
    let a = std::time::Instant::now();
    thread::sleep(Duration::from_millis(1));
    let b = std::time::Instant::now();
    assert_eq!(saturating_ms_between(b, a), 0);
}

#[test]
fn security_denials_always_retained_at_zero_sample_rate() {
    let s = TraceSampler::new();
    assert_eq!(
        retention_for_outcome("result_rejected"),
        RetentionClass::AlwaysRetain
    );
    assert!(s.should_persist(RetentionClass::AlwaysRetain, 0.0));
    assert!(!s.should_persist(RetentionClass::Sampled, 0.0));
}

#[test]
fn trace_storage_respects_recovery_reservation() {
    let budget = TraceStorageBudget {
        available_bytes: 10_000,
        trace_quota_bytes: 8_000,
        reserved_recovery_bytes: 4_000,
    };
    assert!(admit_trace_write(&budget, 100, 50).is_ok());
    assert!(admit_trace_write(&budget, 5_500, 600).is_err());
}

#[test]
fn high_cardinality_labels_rejected() {
    assert!(!is_safe_metric_label("path"));
    assert!(!is_safe_metric_label("filename"));
    assert!(is_safe_metric_label("scheduler_reason"));
}

#[test]
fn emit_safe_metric_refuses_filename() {
    lokai_telemetry::emit_safe_metric("filename", "/tmp/secret.rs");
    lokai_telemetry::emit_safe_metric("job_kind", "infer");
}

#[test]
fn worker_overlay_does_not_change_coordinator_e2e() {
    let mut c = CoordinatorObservedTiming::start("trace_skew", None);
    c.mark_admitted();
    c.mark_dispatched();
    c.mark_result_received();
    c.mark_accepted();
    let without = critical_path_ms(&c, None);
    let worker = WorkerLocalDurations {
        queue_ms: Some(9_999),
        execute_ms: Some(9_999),
        ..Default::default()
    };
    let with = critical_path_ms(&c, Some(&worker));
    assert_eq!(without.e2e_ms, with.e2e_ms);
    assert_eq!(with.segments.execution_ms, Some(9_999));
    assert!(!format_critical_path_report(&with).contains("prompt"));
}

#[test]
fn attached_worker_used_when_arg_missing() {
    let mut c = CoordinatorObservedTiming::start("trace_attach", None);
    c.mark_accepted();
    c.attach_worker(WorkerLocalDurations {
        execute_ms: Some(12),
        ..Default::default()
    });
    let report = critical_path_ms(&c, None);
    assert!(!report.missing_worker_spans);
    assert_eq!(report.segments.execution_ms, Some(12));
}

#[test]
fn sampling_under_high_event_rate_keeps_rejects() {
    let s = TraceSampler::new();
    let mut sampled_kept = 0u32;
    for _ in 0..10_000 {
        if s.should_persist(RetentionClass::Sampled, 0.1) {
            sampled_kept += 1;
        }
    }
    assert!(
        (800..1_200).contains(&sampled_kept),
        "10% sample of 10000 should be ~1000, got {sampled_kept}"
    );
    for _ in 0..50 {
        assert!(s.should_persist(RetentionClass::AlwaysRetain, 0.0));
    }
}

#[test]
fn oversized_trace_write_blocked() {
    let budget = TraceStorageBudget {
        available_bytes: 1_000,
        trace_quota_bytes: 200,
        reserved_recovery_bytes: 100,
    };
    assert!(admit_trace_write(&budget, 0, 50).is_ok());
    assert!(admit_trace_write(&budget, 0, 500).is_err());
}

#[test]
fn cancelled_outcomes_always_retained() {
    assert_eq!(
        retention_for_outcome("canceled"),
        RetentionClass::AlwaysRetain
    );
    assert_eq!(
        retention_for_outcome("superseded"),
        RetentionClass::AlwaysRetain
    );
}

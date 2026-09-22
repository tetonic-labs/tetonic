//! Safe compute/scheduler span names and attribute keys (M6-3 thin).
//!
//! Attributes must never include prompts, source, secrets, or raw model output.

/// Span names for the compute broker path.
pub mod names {
    pub const COMPUTE_SUBMIT: &str = "compute.submit";
    pub const ADMISSION_EVALUATE: &str = "admission.evaluate";
    pub const SCHEDULER_DECIDE: &str = "scheduler.decide";
    pub const RESOURCE_RESERVE: &str = "resource.reserve";
    pub const RESOURCE_RELEASE: &str = "resource.release";
    pub const QUEUE_WAIT: &str = "queue.wait";
    pub const TRANSFER_INPUT: &str = "transfer.input";
    pub const TRANSFER_OUTPUT: &str = "transfer.output";
    pub const WORKER_EXECUTE: &str = "worker.execute";
    pub const VERIFICATION: &str = "verification.local";
    pub const RESULT_ACCEPT: &str = "result.accept";
    pub const E2E_COORDINATOR: &str = "e2e.coordinator";
    /// Time waiting for the memory writer queue or a free read-pool connection (H2-2).
    pub const STORE_WAIT: &str = "store.wait";
}

/// Safe attribute keys (bounded, non-payload).
pub mod attrs {
    pub const JOB_KIND: &str = "job_kind";
    pub const DATA_CLASS: &str = "data_class";
    pub const ADMISSION_OUTCOME: &str = "admission_outcome";
    pub const SCHEDULER_REASON: &str = "scheduler_reason";
    pub const TARGET_TYPE: &str = "target_type";
    pub const SPECULATIVE: &str = "speculative";
    pub const DURATION_MS: &str = "duration_ms";
    pub const SCHEDULER_DECISION_ID: &str = "scheduler_decision_id";
    pub const RESERVATION_ID: &str = "reservation_id";
    /// `"read"` or `"write"` — store wait op (H2-2).
    pub const STORE_OP: &str = "store_op";
}

/// Emit a debug span event for a named compute stage with only safe fields.
#[allow(clippy::too_many_arguments)]
pub fn record_compute_stage(
    span_name: &str,
    job_kind: Option<&str>,
    admission_outcome: Option<&str>,
    scheduler_reason: Option<&str>,
    target_type: Option<&str>,
    scheduler_decision_id: Option<&str>,
    reservation_id: Option<&str>,
    duration_ms: Option<u64>,
    speculative: bool,
) {
    tracing::debug!(
        target: "lokai_compute_trace",
        span_name = span_name,
        job_kind = job_kind.unwrap_or(""),
        admission_outcome = admission_outcome.unwrap_or(""),
        scheduler_reason = scheduler_reason.unwrap_or(""),
        target_type = target_type.unwrap_or(""),
        scheduler_decision_id = scheduler_decision_id.unwrap_or(""),
        reservation_id = reservation_id.unwrap_or(""),
        duration_ms = duration_ms.unwrap_or(0),
        speculative,
        "compute stage"
    );
}

/// Aggregate metric point. High-cardinality label names are dropped.
pub fn emit_safe_metric(label: &str, value: &str) {
    if !crate::storage::is_safe_metric_label(label) {
        tracing::debug!(
            target: "lokai_metrics",
            refused_label = true,
            "dropped high-cardinality metric label"
        );
        return;
    }
    tracing::debug!(target: "lokai_metrics", label, value, "metric");
}

/// Export store queue / pool wait so contention is visible before it is an incident (H2-2).
///
/// `op` must be `"read"` or `"write"`. Duration is also bucketed into a safe metric value
/// so dashboards can count waits without high-cardinality free-form labels.
pub fn record_store_wait(op: &str, wait_ms: u64) {
    let op = match op {
        "read" | "write" => op,
        _ => "other",
    };
    record_compute_stage(
        names::STORE_WAIT,
        None,
        None,
        None,
        Some(op),
        None,
        None,
        Some(wait_ms),
        false,
    );
    let bucket = match wait_ms {
        0..=1 => "0_1ms",
        2..=10 => "2_10ms",
        11..=50 => "11_50ms",
        51..=200 => "51_200ms",
        201..=1000 => "201_1000ms",
        _ => "gt_1000ms",
    };
    emit_safe_metric("store_op", &format!("{op}:{bucket}"));
}

/// Emit separate queue / transfer / execution / verification / e2e stages.
pub fn record_stage_segments(
    segments: &crate::timing::StageSegments,
    scheduler_decision_id: Option<&str>,
    speculative: bool,
    missing_worker_spans: bool,
) {
    let id = scheduler_decision_id;
    if let Some(ms) = segments.queue_wait_ms {
        record_compute_stage(
            names::QUEUE_WAIT,
            Some("infer"),
            None,
            None,
            None,
            id,
            None,
            Some(ms),
            speculative,
        );
    }
    if let Some(ms) = segments.transfer_input_ms {
        record_compute_stage(
            names::TRANSFER_INPUT,
            Some("infer"),
            None,
            None,
            Some("remote"),
            id,
            None,
            Some(ms),
            speculative,
        );
    }
    if let Some(ms) = segments.transfer_output_ms {
        record_compute_stage(
            names::TRANSFER_OUTPUT,
            Some("infer"),
            None,
            None,
            Some("remote"),
            id,
            None,
            Some(ms),
            speculative,
        );
    }
    if let Some(ms) = segments.execution_ms {
        record_compute_stage(
            names::WORKER_EXECUTE,
            Some("infer"),
            None,
            None,
            None,
            id,
            None,
            Some(ms),
            speculative,
        );
    } else if missing_worker_spans {
        tracing::debug!(
            target: "lokai_compute_trace",
            span_name = names::WORKER_EXECUTE,
            missing = true,
            scheduler_decision_id = id.unwrap_or(""),
            "worker spans missing; coordinator e2e remains valid"
        );
    }
    if let Some(ms) = segments.verification_ms {
        record_compute_stage(
            names::VERIFICATION,
            Some("infer"),
            None,
            None,
            None,
            id,
            None,
            Some(ms),
            speculative,
        );
    }
    if let Some(ms) = segments.result_accept_ms {
        record_compute_stage(
            names::RESULT_ACCEPT,
            Some("infer"),
            Some("accepted"),
            None,
            None,
            id,
            None,
            Some(ms),
            speculative,
        );
    }
    if let Some(ms) = segments.e2e_ms {
        record_compute_stage(
            names::E2E_COORDINATOR,
            Some("infer"),
            None,
            None,
            None,
            id,
            None,
            Some(ms),
            speculative,
        );
    }
}

/// Always-retain security / result rejection outcomes (never dropped by sampling).
pub fn record_retained_outcome(outcome: &str, duration_ms: Option<u64>) {
    let class = crate::sampling::retention_for_outcome(outcome);
    let gate = crate::storage::global_trace_gate();
    if !gate.should_emit(outcome, class, 64) {
        return;
    }
    tracing::info!(
        target: "lokai_compute_trace",
        span_name = names::RESULT_ACCEPT,
        admission_outcome = outcome,
        duration_ms = duration_ms.unwrap_or(0),
        retention = "always_retain",
        "retained security or result outcome"
    );
}

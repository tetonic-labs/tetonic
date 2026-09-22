//! Coordinator-observed timing and critical-path math (M6-3).
//!
//! Critical path uses only coordinator [`Instant`] deltas. Worker-reported
//! durations are optional overlays and must never be subtracted from wall clocks.

use std::time::Instant;

use serde::{Deserialize, Serialize};

/// Worker-local durations (monotonic on the worker). Never wall-clock stamps.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerLocalDurations {
    pub queue_ms: Option<u64>,
    pub execute_ms: Option<u64>,
    pub model_load_ms: Option<u64>,
    pub serialize_out_ms: Option<u64>,
}

/// Coordinator-side transfer segment durations (monotonic on coordinator).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferDurations {
    pub serialize_in_ms: Option<u64>,
    pub send_ms: Option<u64>,
    pub recv_ms: Option<u64>,
    pub input_bytes: Option<u64>,
    pub output_bytes: Option<u64>,
}

/// Separate stage durations for M6-3 AC (coordinator Instant where possible).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageSegments {
    pub queue_wait_ms: Option<u64>,
    pub transfer_input_ms: Option<u64>,
    pub transfer_output_ms: Option<u64>,
    pub execution_ms: Option<u64>,
    pub verification_ms: Option<u64>,
    pub result_accept_ms: Option<u64>,
    pub e2e_ms: Option<u64>,
}

/// Monotonic marks on the coordinator process only.
#[derive(Debug, Clone)]
pub struct CoordinatorObservedTiming {
    pub trace_id: String,
    pub scheduler_decision_id: Option<String>,
    pub submit_at: Instant,
    pub admitted_at: Option<Instant>,
    pub dispatched_at: Option<Instant>,
    pub result_received_at: Option<Instant>,
    pub verified_at: Option<Instant>,
    pub accepted_at: Option<Instant>,
    pub transfer: TransferDurations,
    /// Client-measured verification (accept_remote_result Instant), not clock math.
    pub verification_observed_ms: Option<u64>,
    /// Worker-local overlay attached after result accept.
    pub worker: Option<WorkerLocalDurations>,
}

impl CoordinatorObservedTiming {
    pub fn start(trace_id: impl Into<String>, scheduler_decision_id: Option<String>) -> Self {
        Self {
            trace_id: trace_id.into(),
            scheduler_decision_id,
            submit_at: Instant::now(),
            admitted_at: None,
            dispatched_at: None,
            result_received_at: None,
            verified_at: None,
            accepted_at: None,
            transfer: TransferDurations::default(),
            verification_observed_ms: None,
            worker: None,
        }
    }

    pub fn attach_worker(&mut self, worker: WorkerLocalDurations) {
        self.worker = Some(worker);
    }

    pub fn mark_admitted(&mut self) {
        self.admitted_at = Some(Instant::now());
    }

    pub fn mark_dispatched(&mut self) {
        self.dispatched_at = Some(Instant::now());
    }

    pub fn mark_result_received(&mut self) {
        self.result_received_at = Some(Instant::now());
    }

    pub fn mark_verified(&mut self) {
        self.verified_at = Some(Instant::now());
    }

    pub fn mark_accepted(&mut self) {
        self.accepted_at = Some(Instant::now());
    }

    fn delta_ms(from: Instant, to: Instant) -> u64 {
        to.saturating_duration_since(from).as_millis() as u64
    }

    /// End-to-end ms from submit to accept (or now if not accepted).
    pub fn e2e_ms(&self) -> u64 {
        let end = self.accepted_at.unwrap_or_else(Instant::now);
        Self::delta_ms(self.submit_at, end)
    }

    pub fn segments_ms(&self, worker: Option<&WorkerLocalDurations>) -> StageSegments {
        let queue_wait_ms = match (self.admitted_at, self.dispatched_at) {
            (Some(a), Some(d)) => Some(Self::delta_ms(a, d)),
            _ => None,
        };
        let verification_ms = match (self.result_received_at, self.verified_at) {
            (Some(r), Some(v)) => Some(Self::delta_ms(r, v)),
            _ => self.verification_observed_ms,
        };
        let worker = worker.or(self.worker.as_ref());
        let result_accept_ms = match (self.verified_at, self.accepted_at) {
            (Some(v), Some(a)) => Some(Self::delta_ms(v, a)),
            (None, Some(a)) => self.result_received_at.map(|r| Self::delta_ms(r, a)),
            _ => None,
        };
        StageSegments {
            queue_wait_ms,
            transfer_input_ms: self.transfer.send_ms.or(self.transfer.serialize_in_ms),
            transfer_output_ms: self.transfer.recv_ms,
            execution_ms: worker.and_then(|w| w.execute_ms),
            verification_ms,
            result_accept_ms,
            e2e_ms: Some(self.e2e_ms()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CriticalPathReport {
    pub e2e_ms: u64,
    pub missing_worker_spans: bool,
    pub segments: StageSegments,
}

/// Critical path from coordinator Instant only. Missing worker spans do not
/// invalidate e2e. Never subtract worker wall clocks from coordinator clocks.
pub fn critical_path_ms(
    coord: &CoordinatorObservedTiming,
    worker: Option<&WorkerLocalDurations>,
) -> CriticalPathReport {
    let segments = coord.segments_ms(worker);
    let overlay = worker.or(coord.worker.as_ref());
    CriticalPathReport {
        e2e_ms: coord.e2e_ms(),
        missing_worker_spans: overlay.is_none()
            || overlay.is_some_and(|w| w.execute_ms.is_none() && w.queue_ms.is_none()),
        segments,
    }
}

/// Saturating non-negative duration helper for any two Instant marks.
pub fn saturating_ms_between(earlier: Instant, later: Instant) -> u64 {
    later.saturating_duration_since(earlier).as_millis() as u64
}

/// Local critical-path summary (no payloads). Safe for logs and `fabric/status`.
pub fn format_critical_path_report(report: &CriticalPathReport) -> String {
    format!(
        "e2e_ms={} queue_ms={:?} transfer_in_ms={:?} transfer_out_ms={:?} exec_ms={:?} verify_ms={:?} accept_ms={:?} missing_worker={}",
        report.e2e_ms,
        report.segments.queue_wait_ms,
        report.segments.transfer_input_ms,
        report.segments.transfer_output_ms,
        report.segments.execution_ms,
        report.segments.verification_ms,
        report.segments.result_accept_ms,
        report.missing_worker_spans
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn missing_worker_spans_do_not_invalidate_e2e() {
        let mut c = CoordinatorObservedTiming::start("t1", Some("sched_1".into()));
        c.mark_admitted();
        thread::sleep(Duration::from_millis(2));
        c.mark_dispatched();
        c.mark_result_received();
        c.mark_verified();
        c.mark_accepted();
        let report = critical_path_ms(&c, None);
        assert!(report.missing_worker_spans);
        assert!(report.e2e_ms >= 2);
        assert!(report.segments.e2e_ms.is_some());
    }

    #[test]
    fn segments_are_separate_when_marks_present() {
        let mut c = CoordinatorObservedTiming::start("t2", None);
        c.mark_admitted();
        thread::sleep(Duration::from_millis(1));
        c.mark_dispatched();
        c.transfer.send_ms = Some(5);
        c.transfer.recv_ms = Some(7);
        c.mark_result_received();
        thread::sleep(Duration::from_millis(1));
        c.mark_verified();
        c.mark_accepted();
        let worker = WorkerLocalDurations {
            queue_ms: Some(3),
            execute_ms: Some(40),
            ..Default::default()
        };
        let segs = c.segments_ms(Some(&worker));
        assert!(segs.queue_wait_ms.is_some());
        assert_eq!(segs.transfer_input_ms, Some(5));
        assert_eq!(segs.transfer_output_ms, Some(7));
        assert_eq!(segs.execution_ms, Some(40));
        assert!(segs.verification_ms.is_some());
        assert!(segs.e2e_ms.is_some());
    }

    #[test]
    fn saturating_duration_never_negative() {
        let a = Instant::now();
        thread::sleep(Duration::from_millis(1));
        let b = Instant::now();
        // later before earlier → 0, not underflow.
        assert_eq!(saturating_ms_between(b, a), 0);
        assert!(saturating_ms_between(a, b) >= 1);
    }
}

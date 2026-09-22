//! Admission and scheduler metrics without payload content (M6-1 / M6-3).

use std::sync::atomic::{AtomicU64, Ordering};

use crate::admission::AdmissionRejectionReason;

#[derive(Default)]
pub struct AdmissionMetrics {
    pub admitted: AtomicU64,
    pub queued: AtomicU64,
    pub rejected: AtomicU64,
    pub canceled: AtomicU64,
    pub reservation_leaks: AtomicU64,
}

impl AdmissionMetrics {
    pub fn record_admitted(&self) {
        self.admitted.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_queued(&self) {
        self.queued.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_rejected(&self, reason: &AdmissionRejectionReason) {
        self.rejected.fetch_add(1, Ordering::Relaxed);
        lokai_telemetry::emit_safe_metric("admission_outcome", reason.as_str());
        tracing::debug!(
            target: "lokai_admission_metrics",
            reason = reason.as_str(),
            "admission rejected"
        );
    }

    pub fn record_canceled(&self) {
        self.canceled.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_leak(&self) {
        self.reservation_leaks.fetch_add(1, Ordering::Relaxed);
    }
}

/// Scheduler decision counters (M6-3 hooks / M6-2). No high-cardinality labels.
#[derive(Default)]
pub struct SchedulerMetrics {
    pub decisions: AtomicU64,
    pub offload: AtomicU64,
    pub local_preferred: AtomicU64,
    pub fallback: AtomicU64,
    pub circuit_open: AtomicU64,
    pub speculation_denied: AtomicU64,
    /// Sum of |actual_ms - predicted_finish_ms| for mean absolute error.
    pub prediction_error_abs_sum_ms: AtomicU64,
    pub prediction_samples: AtomicU64,
    /// Speculative races launched (M6-3 cost suite).
    pub speculation_launched: AtomicU64,
    pub speculation_won_primary: AtomicU64,
    pub speculation_won_speculative: AtomicU64,
    /// Sum of loser-leg coordinator ms (extra cost).
    pub speculation_extra_ms_sum: AtomicU64,
    /// Sum of max(0, slower_leg_ms - winner_ms) when speculation helped.
    pub speculation_latency_saved_ms_sum: AtomicU64,
    pub queue_depth_samples: AtomicU64,
    pub queue_depth_sum: AtomicU64,
}

impl SchedulerMetrics {
    pub fn record_decision(&self) {
        self.decisions.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_offload(&self) {
        self.offload.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_local_preferred(&self) {
        self.local_preferred.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_fallback(&self) {
        self.fallback.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_circuit_open(&self) {
        self.circuit_open.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_speculation_denied(&self) {
        self.speculation_denied.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_prediction_error(&self, abs_error_ms: u64) {
        self.prediction_error_abs_sum_ms
            .fetch_add(abs_error_ms, Ordering::Relaxed);
        self.prediction_samples.fetch_add(1, Ordering::Relaxed);
    }

    /// Mean absolute prediction error in ms, when at least one sample exists.
    pub fn mean_abs_prediction_error_ms(&self) -> Option<u64> {
        let n = self.prediction_samples.load(Ordering::Relaxed);
        if n == 0 {
            return None;
        }
        let sum = self.prediction_error_abs_sum_ms.load(Ordering::Relaxed);
        Some(sum / n)
    }

    pub fn prediction_sample_count(&self) -> u64 {
        self.prediction_samples.load(Ordering::Relaxed)
    }

    pub fn record_speculation_launched(&self) {
        self.speculation_launched.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_speculation_race(&self, winner_primary: bool, winner_ms: u64, loser_ms: u64) {
        if winner_primary {
            self.speculation_won_primary.fetch_add(1, Ordering::Relaxed);
        } else {
            self.speculation_won_speculative
                .fetch_add(1, Ordering::Relaxed);
        }
        self.speculation_extra_ms_sum
            .fetch_add(loser_ms, Ordering::Relaxed);
        if loser_ms > winner_ms {
            self.speculation_latency_saved_ms_sum
                .fetch_add(loser_ms - winner_ms, Ordering::Relaxed);
        }
    }

    pub fn record_queue_depth(&self, depth: u64) {
        self.queue_depth_samples.fetch_add(1, Ordering::Relaxed);
        self.queue_depth_sum.fetch_add(depth, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mean_abs_prediction_error_from_samples() {
        let m = SchedulerMetrics::default();
        assert_eq!(m.mean_abs_prediction_error_ms(), None);
        m.record_prediction_error(100);
        m.record_prediction_error(50);
        assert_eq!(m.prediction_sample_count(), 2);
        assert_eq!(m.mean_abs_prediction_error_ms(), Some(75));
    }
}

//! Bounded trace storage that respects recovery-space reservation (M6-3).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use thiserror::Error;

use crate::sampling::{RetentionClass, TraceSampler};

/// Budget derived from domain `StorageLimits` fields (no domain crate dep).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceStorageBudget {
    pub available_bytes: u64,
    pub trace_quota_bytes: u64,
    pub reserved_recovery_bytes: u64,
}

impl TraceStorageBudget {
    /// Defaults aligned with `StorageLimits::default()` trace/recovery fields.
    pub fn defaults() -> Self {
        Self {
            available_bytes: 10 * 1024 * 1024 * 1024,
            trace_quota_bytes: 500 * 1024 * 1024,
            reserved_recovery_bytes: 50 * 1024 * 1024,
        }
    }

    pub fn from_quotas(
        available_bytes: u64,
        trace_quota_bytes: u64,
        reserved_recovery_bytes: u64,
    ) -> Self {
        Self {
            available_bytes,
            trace_quota_bytes,
            reserved_recovery_bytes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TraceStorageError {
    #[error("trace write would invade recovery reservation")]
    InvadesRecovery,
    #[error("trace quota exceeded")]
    QuotaExceeded,
}

/// Admit a write only when it fits in the trace quota and leaves recovery reserved.
pub fn admit_trace_write(
    budget: &TraceStorageBudget,
    used_trace_bytes: u64,
    write_bytes: u64,
) -> Result<(), TraceStorageError> {
    let after = used_trace_bytes.saturating_add(write_bytes);
    if after > budget.trace_quota_bytes {
        return Err(TraceStorageError::QuotaExceeded);
    }
    // Never consume the recovery reservation from the available pool.
    let usable = budget
        .available_bytes
        .saturating_sub(budget.reserved_recovery_bytes);
    if after > usable {
        return Err(TraceStorageError::InvadesRecovery);
    }
    Ok(())
}

/// Process-wide gate used by the local sink formatter and retained outcomes.
#[derive(Debug)]
pub struct TraceWriteGate {
    budget: TraceStorageBudget,
    used: AtomicU64,
    sampler: TraceSampler,
    sample_rate: f64,
}

impl TraceWriteGate {
    pub fn new(budget: TraceStorageBudget, sample_rate: f64) -> Self {
        Self {
            budget,
            used: AtomicU64::new(0),
            sampler: TraceSampler::new(),
            sample_rate: sample_rate.clamp(0.0, 1.0),
        }
    }

    pub fn used_bytes(&self) -> u64 {
        self.used.load(Ordering::Relaxed)
    }

    /// Sampling + storage admission. AlwaysRetain still needs storage room;
    /// if storage is exhausted, security outcomes still emit (recovery reserved
    /// separately — we only skip Sampled when over budget).
    pub fn should_emit(&self, outcome: &str, class: RetentionClass, write_bytes: u64) -> bool {
        let _ = outcome;
        if !self.sampler.should_persist(class, self.sample_rate) {
            return false;
        }
        let used = self.used.load(Ordering::Relaxed);
        match admit_trace_write(&self.budget, used, write_bytes) {
            Ok(()) => {
                self.used.fetch_add(write_bytes, Ordering::Relaxed);
                true
            }
            Err(_) => matches!(class, RetentionClass::AlwaysRetain),
        }
    }
}

fn global_gate_slot() -> &'static Mutex<TraceWriteGate> {
    static GATE: OnceLock<Mutex<TraceWriteGate>> = OnceLock::new();
    GATE.get_or_init(|| Mutex::new(TraceWriteGate::new(TraceStorageBudget::defaults(), 1.0)))
}

/// Install budget/sample rate for the process (daemon/CLI init).
pub fn configure_trace_gate(budget: TraceStorageBudget, sample_rate: f64) {
    if let Ok(mut g) = global_gate_slot().lock() {
        *g = TraceWriteGate::new(budget, sample_rate);
    }
}

/// Shared gate for formatter + retained outcome helpers.
pub fn global_trace_gate() -> std::sync::MutexGuard<'static, TraceWriteGate> {
    global_gate_slot().lock().unwrap_or_else(|e| e.into_inner())
}

/// Labels allowed on aggregate metrics (no paths/filenames/ids).
pub const SAFE_METRIC_LABELS: &[&str] = &[
    "job_kind",
    "data_class",
    "admission_outcome",
    "scheduler_reason",
    "target_type",
    "speculative",
    "outcome",
    "store_op",
];

pub fn is_safe_metric_label(name: &str) -> bool {
    SAFE_METRIC_LABELS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_write_that_invades_recovery() {
        let b = TraceStorageBudget {
            available_bytes: 1000,
            trace_quota_bytes: 900,
            reserved_recovery_bytes: 400,
        };
        // usable = 600; 500+200 = 700 > 600
        assert!(matches!(
            admit_trace_write(&b, 500, 200),
            Err(TraceStorageError::InvadesRecovery)
        ));
    }

    #[test]
    fn allows_write_inside_quota_and_usable() {
        let b = TraceStorageBudget {
            available_bytes: 1000,
            trace_quota_bytes: 900,
            reserved_recovery_bytes: 200,
        };
        assert!(admit_trace_write(&b, 100, 50).is_ok());
    }

    #[test]
    fn rejects_high_cardinality_labels() {
        assert!(!is_safe_metric_label("filename"));
        assert!(!is_safe_metric_label("worker_id"));
        assert!(is_safe_metric_label("job_kind"));
        assert!(is_safe_metric_label("store_op"));
    }

    #[test]
    fn record_store_wait_accepts_read_and_write() {
        crate::spans::record_store_wait("write", 25);
        crate::spans::record_store_wait("read", 0);
    }

    #[test]
    fn gate_drops_sampled_when_over_quota() {
        let gate = TraceWriteGate::new(
            TraceStorageBudget {
                available_bytes: 200,
                trace_quota_bytes: 100,
                reserved_recovery_bytes: 50,
            },
            1.0,
        );
        assert!(gate.should_emit("token_delta", RetentionClass::Sampled, 80));
        assert!(!gate.should_emit("token_delta", RetentionClass::Sampled, 80));
        assert!(gate.should_emit("result_rejected", RetentionClass::AlwaysRetain, 80));
    }
}

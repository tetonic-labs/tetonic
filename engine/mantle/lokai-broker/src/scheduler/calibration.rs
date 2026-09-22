//! Per-worker prediction-error calibration for uncertainty margins (M6-2 / M6-3).

use std::collections::HashMap;
use std::sync::Mutex;

use crate::scheduler::estimate::UncertaintyModel;
use crate::scheduler::types::ExecutionTargetId;

#[derive(Debug, Clone, Default)]
pub struct WorkerMae {
    pub samples: u32,
    pub rolling_mae_ms: u64,
}

/// Rolling MAE feedback used when building [`UncertaintyModel`] for schedule candidates.
#[derive(Default)]
pub struct PredictionCalibration {
    inner: Mutex<HashMap<String, WorkerMae>>,
}

impl PredictionCalibration {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn key_for(target: &ExecutionTargetId) -> String {
        target.as_label()
    }

    pub fn record(&self, worker_key: &str, abs_error_ms: u64) {
        let Ok(mut g) = self.inner.lock() else {
            return;
        };
        let entry = g.entry(worker_key.to_string()).or_default();
        let n = u64::from(entry.samples);
        let next_n = n.saturating_add(1);
        entry.rolling_mae_ms = if next_n == 0 {
            abs_error_ms
        } else {
            (entry.rolling_mae_ms.saturating_mul(n))
                .saturating_add(abs_error_ms)
                .checked_div(next_n)
                .unwrap_or(abs_error_ms)
        };
        entry.samples = entry.samples.saturating_add(1);
    }

    /// Record MAE by worker, local/remote, and cold/warm (M6-3 dimensions).
    pub fn record_attempt(&self, target: &ExecutionTargetId, cold_start: bool, abs_error_ms: u64) {
        self.record(&target.as_label(), abs_error_ms);
        self.record(
            if matches!(target, ExecutionTargetId::Local) {
                "dim:local"
            } else {
                "dim:remote"
            },
            abs_error_ms,
        );
        self.record(
            if cold_start { "dim:cold" } else { "dim:warm" },
            abs_error_ms,
        );
    }

    pub fn snapshot(&self, worker_key: &str) -> Option<WorkerMae> {
        self.inner
            .lock()
            .ok()
            .and_then(|g| g.get(worker_key).cloned())
    }

    /// Build uncertainty for a candidate; falls back to cold floors when history is sparse.
    pub fn uncertainty_for(&self, worker_key: &str, cold_start: bool) -> UncertaintyModel {
        let snap = self.snapshot(worker_key);
        match snap {
            Some(w) if w.samples > 0 => UncertaintyModel {
                samples: w.samples,
                rolling_mae_ms: w.rolling_mae_ms.max(1),
                cold_floor_ms: 250,
                cold_model_extra_ms: if cold_start { 500 } else { 0 },
            },
            _ if worker_key == "local" || worker_key.eq_ignore_ascii_case("node_local") => {
                UncertaintyModel {
                    samples: 20,
                    rolling_mae_ms: 40,
                    ..UncertaintyModel::default()
                }
            }
            _ => UncertaintyModel {
                samples: 0,
                rolling_mae_ms: 0,
                cold_floor_ms: 300,
                cold_model_extra_ms: 500,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolling_mae_updates_and_feeds_uncertainty() {
        let cal = PredictionCalibration::new();
        cal.record("w1", 100);
        cal.record("w1", 50);
        let snap = cal.snapshot("w1").expect("snap");
        assert_eq!(snap.samples, 2);
        assert_eq!(snap.rolling_mae_ms, 75);
        let u = cal.uncertainty_for("w1", false);
        assert_eq!(u.samples, 2);
        assert_eq!(u.rolling_mae_ms, 75);
        cal.record("dim:remote", 80);
        cal.record("dim:cold", 90);
        assert_eq!(cal.snapshot("dim:remote").unwrap().samples, 1);
        assert_eq!(cal.snapshot("dim:cold").unwrap().rolling_mae_ms, 90);
    }
}

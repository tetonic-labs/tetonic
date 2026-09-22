//! Trace sampling with always-retain security classes (M6-3).

use std::sync::atomic::{AtomicU64, Ordering};

/// Retention class for a trace event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionClass {
    /// Security / integrity / admission denials — never dropped by sampling.
    AlwaysRetain,
    /// Ordinary progress events — subject to sample_rate.
    Sampled,
}

/// Classify common security-critical outcomes.
pub fn retention_for_outcome(outcome: &str) -> RetentionClass {
    let o = outcome.to_ascii_lowercase();
    if o.contains("deny")
        || o.contains("reject")
        || o.contains("revok")
        || o.contains("supersed")
        || o.contains("canceled")
        || o.contains("cancelled")
        || o.contains("verification_fail")
        || o.contains("admission_reject")
        || o.contains("security")
    {
        RetentionClass::AlwaysRetain
    } else {
        RetentionClass::Sampled
    }
}

/// Deterministic sampler using a counter (tests + production without RNG).
#[derive(Debug, Default)]
pub struct TraceSampler {
    counter: AtomicU64,
}

impl TraceSampler {
    pub fn new() -> Self {
        Self::default()
    }

    /// `sample_rate` in [0.0, 1.0]. AlwaysRetain always returns true.
    pub fn should_persist(&self, class: RetentionClass, sample_rate: f64) -> bool {
        match class {
            RetentionClass::AlwaysRetain => true,
            RetentionClass::Sampled => {
                if sample_rate >= 1.0 {
                    return true;
                }
                if sample_rate <= 0.0 {
                    return false;
                }
                let n = self.counter.fetch_add(1, Ordering::Relaxed);
                let threshold = (sample_rate * 10_000.0) as u64;
                (n % 10_000) < threshold
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_retain_survives_zero_sample_rate() {
        let s = TraceSampler::new();
        assert!(s.should_persist(RetentionClass::AlwaysRetain, 0.0));
        assert!(!s.should_persist(RetentionClass::Sampled, 0.0));
    }

    #[test]
    fn security_outcomes_always_retain() {
        assert_eq!(
            retention_for_outcome("admission_reject"),
            RetentionClass::AlwaysRetain
        );
        assert_eq!(
            retention_for_outcome("result_rejected"),
            RetentionClass::AlwaysRetain
        );
        assert_eq!(
            retention_for_outcome("token_delta"),
            RetentionClass::Sampled
        );
    }
}

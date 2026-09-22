//! Probe outcome counters for coordinator diagnostics (M5-2).

use crate::capability_probes::CapabilityProbeResult;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CapabilityProbeMetrics {
    pub probes_passed: u64,
    pub probes_failed: u64,
    pub workers_quarantined: u64,
    pub workers_degraded: u64,
    pub cache_inserts: u64,
    pub cache_updates: u64,
}

impl CapabilityProbeMetrics {
    pub fn record_probe_results(
        &mut self,
        results: &[CapabilityProbeResult],
        quarantined: bool,
        degraded: bool,
    ) {
        for r in results {
            if r.passed {
                self.probes_passed += 1;
            } else {
                self.probes_failed += 1;
            }
        }
        if quarantined {
            self.workers_quarantined += 1;
        }
        if degraded {
            self.workers_degraded += 1;
        }
    }
}

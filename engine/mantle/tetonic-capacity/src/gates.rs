//! Pass/fail gates for bench reports (floor tier defaults).

use serde::{Deserialize, Serialize};

use crate::bench::{BenchReport, BenchSuite};
use crate::profile::{observed_gpu_pct, ObservedPlacement};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GatePolicy {
    pub raw_short_warn_s: f64,
    pub raw_short_fail_s: f64,
    pub gpu_pct_warn: f32,
    pub gpu_pct_fail: f32,
    /// Legacy serialized field. Ignored: model-used VRAM cannot establish total
    /// hardware capacity and must not bypass placement checks.
    #[serde(default)]
    pub gpu_gate_min_vram_mb: u32,
}

impl Default for GatePolicy {
    fn default() -> Self {
        Self {
            raw_short_warn_s: 15.0,
            raw_short_fail_s: 30.0,
            gpu_pct_warn: 100.0,
            gpu_pct_fail: 100.0,
            gpu_gate_min_vram_mb: 10_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GateFailure {
    pub gate: String,
    pub message: String,
    pub severity: GateSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GateSeverity {
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GateVerdict {
    pub passed: bool,
    pub failures: Vec<GateFailure>,
}

pub fn evaluate_gates(report: &BenchReport, policy: &GatePolicy) -> GateVerdict {
    let mut failures = Vec::new();

    if let Some(short) = report.suites.get(&BenchSuite::RawShort) {
        if short.wall_s > policy.raw_short_fail_s {
            failures.push(GateFailure {
                gate: "raw_short".into(),
                message: format!(
                    "wall {:.1}s exceeds fail threshold {:.1}s",
                    short.wall_s, policy.raw_short_fail_s
                ),
                severity: GateSeverity::Fail,
            });
        } else if short.wall_s > policy.raw_short_warn_s {
            failures.push(GateFailure {
                gate: "raw_short".into(),
                message: format!(
                    "wall {:.1}s exceeds warn threshold {:.1}s",
                    short.wall_s, policy.raw_short_warn_s
                ),
                severity: GateSeverity::Warn,
            });
        }
    } else {
        failures.push(GateFailure {
            gate: "raw_short".into(),
            message: "suite not run".into(),
            severity: GateSeverity::Fail,
        });
    }

    if let Some(obs) = &report.observed {
        evaluate_gpu_gate(obs, policy, &mut failures);
    } else {
        failures.push(GateFailure {
            gate: "gpu_processor".into(),
            message: "requested model placement was not measured".into(),
            severity: GateSeverity::Fail,
        });
    }

    let passed = !failures.iter().any(|f| f.severity == GateSeverity::Fail);
    GateVerdict { passed, failures }
}

fn evaluate_gpu_gate(
    observed: &ObservedPlacement,
    policy: &GatePolicy,
    failures: &mut Vec<GateFailure>,
) {
    let Some(pct) = observed_gpu_pct(observed) else {
        failures.push(GateFailure {
            gate: "gpu_processor".into(),
            message: "gpu_processor_pct not measured (structured /api/ps or NVML)".into(),
            severity: GateSeverity::Fail,
        });
        return;
    };
    // Used bytes are not installed capacity. A spilling runner can consume few
    // GPU bytes on a large GPU; that must not disable its placement gate.
    if pct < policy.gpu_pct_fail {
        failures.push(GateFailure {
            gate: "gpu_processor".into(),
            message: format!(
                "GPU share {:.0}% below fail threshold {:.0}%",
                pct, policy.gpu_pct_fail
            ),
            severity: GateSeverity::Fail,
        });
    } else if pct < policy.gpu_pct_warn {
        failures.push(GateFailure {
            gate: "gpu_processor".into(),
            message: format!(
                "GPU share {:.0}% below warn threshold {:.0}%",
                pct, policy.gpu_pct_warn
            ),
            severity: GateSeverity::Warn,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::SuiteResult;
    use chrono::Utc;

    fn sample_observed(gpu_pct: f32) -> ObservedPlacement {
        ObservedPlacement {
            gpu_processor_pct: Some(gpu_pct),
            processor_split: None,
            vram_used_mb: 18_000,
            resident_model: "qwen3.6-estate".into(),
            load_wall_s: 10.0,
            measured_at: Utc::now(),
        }
    }

    #[test]
    fn little_gpu_usage_and_missing_placement_do_not_bypass_checks() {
        let mut observed = sample_observed(10.0);
        observed.vram_used_mb = 200;
        let mut failures = Vec::new();
        evaluate_gpu_gate(&observed, &GatePolicy::default(), &mut failures);
        assert!(failures.iter().any(|f| f.severity == GateSeverity::Fail));
        observed.gpu_processor_pct = None;
        failures.clear();
        evaluate_gpu_gate(&observed, &GatePolicy::default(), &mut failures);
        assert!(failures.iter().any(|f| f.severity == GateSeverity::Fail));
        let report = BenchReport {
            schema_version: 1,
            model: "m".into(),
            ollama_base: "local".into(),
            suites: [(
                BenchSuite::RawShort,
                SuiteResult {
                    wall_s: 1.0,
                    prompt_tokens: 1,
                    eval_tokens: 1,
                    prefill_tps: 1.0,
                    decode_tps: 1.0,
                },
            )]
            .into_iter()
            .collect(),
            observed: None,
            gates: None,
        };
        assert!(!evaluate_gates(&report, &GatePolicy::default()).passed);
    }

    #[test]
    fn fails_slow_raw_short() {
        let report = BenchReport {
            schema_version: 1,
            model: "m".into(),
            ollama_base: "http://127.0.0.1:11434".into(),
            suites: [(
                BenchSuite::RawShort,
                SuiteResult {
                    wall_s: 101.0,
                    prompt_tokens: 1,
                    eval_tokens: 1,
                    prefill_tps: 1.0,
                    decode_tps: 1.0,
                },
            )]
            .into_iter()
            .collect(),
            observed: Some(sample_observed(100.0)),
            gates: None,
        };
        let v = evaluate_gates(&report, &GatePolicy::default());
        assert!(!v.passed);
    }

    #[test]
    fn passes_healthy_floor() {
        let report = BenchReport {
            schema_version: 1,
            model: "m".into(),
            ollama_base: "http://127.0.0.1:11434".into(),
            suites: [(
                BenchSuite::RawShort,
                SuiteResult {
                    wall_s: 8.0,
                    prompt_tokens: 1,
                    eval_tokens: 1,
                    prefill_tps: 100.0,
                    decode_tps: 40.0,
                },
            )]
            .into_iter()
            .collect(),
            observed: Some(sample_observed(100.0)),
            gates: None,
        };
        let v = evaluate_gates(&report, &GatePolicy::default());
        assert!(v.passed);
        assert!(v.failures.is_empty());
    }
}

//! Eval result honesty: Incomplete/Failed never count as pass (R07 / R30).

use crate::result::{EvaluationResult, RunStatus};

/// Force non-pass when the run did not complete cleanly.
pub fn normalize_pass(mut result: EvaluationResult) -> EvaluationResult {
    if !matches!(result.run_status, RunStatus::Completed) {
        result.passed = false;
        if result.failure_classification.is_none() {
            result.failure_classification = Some(match result.run_status {
                RunStatus::Incomplete => "incomplete".into(),
                RunStatus::Failed => "failed".into(),
                RunStatus::Completed => unreachable!(),
            });
        }
    }
    result
}

/// Suite pass-rate floor: only Completed+passed scenarios count as passes.
pub fn scenario_counts_as_pass(r: &EvaluationResult) -> bool {
    r.passed && matches!(r.run_status, RunStatus::Completed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::{ResourceUsage, TimingBreakdown};

    fn sample(status: RunStatus, passed: bool) -> EvaluationResult {
        EvaluationResult {
            scenario_id: "x".into(),
            run_status: status,
            timing: TimingBreakdown {
                wall_clock_duration_ms: 1,
                model_inference_duration_ms: 0,
                tool_execution_duration_ms: 0,
            },
            resource_usage: ResourceUsage {
                tool_call_count: 0,
                model_call_count: 0,
                token_usage_prompt: 0,
                token_usage_completion: 0,
            },
            patch_digest: None,
            output_digest: None,
            trace_correlation_id: "t".into(),
            statistics: None,
            passed,
            failure_classification: None,
        }
    }

    #[test]
    fn incomplete_cannot_satisfy_pass() {
        let r = normalize_pass(sample(RunStatus::Incomplete, true));
        assert!(!r.passed);
        assert!(!scenario_counts_as_pass(&r));
        assert_eq!(r.failure_classification.as_deref(), Some("incomplete"));
    }

    #[test]
    fn completed_pass_still_counts() {
        let r = normalize_pass(sample(RunStatus::Completed, true));
        assert!(r.passed);
        assert!(scenario_counts_as_pass(&r));
    }
}

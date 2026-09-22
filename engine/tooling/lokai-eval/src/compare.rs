use crate::result::EvaluationResult;
use crate::suite::SuiteResult;
use std::path::PathBuf;

pub async fn run(baseline_path: PathBuf, candidate_path: PathBuf) -> anyhow::Result<()> {
    tracing::info!(
        "Comparing baseline {} to candidate {}",
        baseline_path.display(),
        candidate_path.display()
    );

    let baseline_raw = std::fs::read_to_string(&baseline_path)?;
    let candidate_raw = std::fs::read_to_string(&candidate_path)?;
    let baseline_val: serde_json::Value = serde_json::from_str(&baseline_raw)?;
    let candidate_val: serde_json::Value = serde_json::from_str(&candidate_raw)?;

    let (has_regression, report) = if baseline_val.get("scenarios").is_some() {
        compare_suites(
            serde_json::from_value(baseline_val)?,
            serde_json::from_value(candidate_val)?,
        )
    } else {
        compare_singles(
            serde_json::from_value(baseline_val)?,
            serde_json::from_value(candidate_val)?,
        )
    };

    if has_regression {
        tracing::error!("{}", report);
        anyhow::bail!("evaluation regression against baseline");
    }
    tracing::info!("No regressions detected.");
    println!("{report}");
    Ok(())
}

fn compare_singles(baseline: EvaluationResult, candidate: EvaluationResult) -> (bool, String) {
    let mut has_regression = false;
    let mut report = String::from("Regression Report:\n");
    if candidate.passed != baseline.passed {
        report.push_str(&format!(
            "- Test pass state changed: Baseline={}, Candidate={}\n",
            baseline.passed, candidate.passed
        ));
        if !candidate.passed && baseline.passed {
            has_regression = true;
        }
    }
    if let (Some(base_stats), Some(cand_stats)) = (&baseline.statistics, &candidate.statistics) {
        if cand_stats.pass_rate + f64::EPSILON < base_stats.pass_rate {
            let drop = base_stats.pass_rate - cand_stats.pass_rate;
            report.push_str(&format!(
                "- Statistical pass rate dropped by {:.2}%\n",
                drop * 100.0
            ));
            if drop > 0.05 {
                has_regression = true;
            }
        }
    }
    (has_regression, report)
}

fn compare_suites(baseline: SuiteResult, candidate: SuiteResult) -> (bool, String) {
    let mut has_regression = false;
    let mut report = String::from("Regression Report:\n");
    report.push_str(&format!(
        "- pass_rate baseline={:.2} candidate={:.2} floor={:.2}\n",
        baseline.pass_rate, candidate.pass_rate, candidate.floor
    ));
    if !candidate.passed && baseline.passed {
        has_regression = true;
        report.push_str("- candidate failed the pass-rate floor while baseline passed\n");
    }
    if candidate.pass_rate + f64::EPSILON < baseline.pass_rate {
        has_regression = true;
        report.push_str("- candidate pass rate dropped vs baseline\n");
    }
    // Improvements are accepted (not a regression). Baseline update is explicit.
    if candidate.pass_rate > baseline.pass_rate + f64::EPSILON {
        report.push_str("- candidate improved pass rate (update baseline explicitly)\n");
    }
    for b in &baseline.scenarios {
        if let Some(c) = candidate
            .scenarios
            .iter()
            .find(|s| s.scenario_id == b.scenario_id)
        {
            if b.passed && !c.passed {
                has_regression = true;
                report.push_str(&format!("- {} flipped pass → fail\n", b.scenario_id));
            }
        }
    }
    (has_regression, report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::{ResourceUsage, RunStatus, TimingBreakdown};

    fn eval_result(id: &str, passed: bool) -> EvaluationResult {
        EvaluationResult {
            scenario_id: id.into(),
            run_status: RunStatus::Completed,
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
    fn regressed_prompt_fails_compare() {
        let baseline = SuiteResult {
            suite: "pr".into(),
            floor: 1.0,
            pass_rate: 1.0,
            passed: true,
            scenarios: vec![eval_result("01-small-single-lang", true)],
        };
        let candidate = SuiteResult {
            suite: "pr".into(),
            floor: 1.0,
            pass_rate: 0.0,
            passed: false,
            scenarios: vec![eval_result("01-small-single-lang", false)],
        };
        let (regressed, _) = compare_suites(baseline, candidate);
        assert!(regressed);
    }

    #[test]
    fn improved_candidate_is_accepted() {
        let baseline = SuiteResult {
            suite: "pr".into(),
            floor: 1.0,
            pass_rate: 0.5,
            passed: false,
            scenarios: vec![
                eval_result("01-small-single-lang", true),
                eval_result("07-synthetic-sensitive", false),
            ],
        };
        let candidate = SuiteResult {
            suite: "pr".into(),
            floor: 1.0,
            pass_rate: 1.0,
            passed: true,
            scenarios: vec![
                eval_result("01-small-single-lang", true),
                eval_result("07-synthetic-sensitive", true),
            ],
        };
        let (regressed, report) = compare_suites(baseline, candidate);
        assert!(!regressed);
        assert!(report.contains("improved"));
    }
}

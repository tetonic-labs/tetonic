use crate::gate::STATISTICAL_TRIALS;
use crate::manifest::EvaluationManifest;
use crate::result::{
    EvaluationResult, ResourceUsage, RunStatus, StatisticalSummary, TimingBreakdown,
};
use crate::traits::{AgentOrchestrator, CorpusProvider, SandboxProvider};
use std::time::Instant;

pub async fn run(
    manifest: EvaluationManifest,
    corpus: &dyn CorpusProvider,
    sandbox: &dyn SandboxProvider,
    orchestrator: &dyn AgentOrchestrator,
) -> anyhow::Result<EvaluationResult> {
    tracing::info!(
        "Starting statistical evaluation for scenario: {}",
        manifest.scenario_id
    );
    let start_time = Instant::now();

    let sample_count = STATISTICAL_TRIALS;
    let mut durations = Vec::new();
    let mut passes = 0u32;
    let mut trials = Vec::with_capacity(sample_count as usize);

    let target_pass = manifest
        .expected_pass_threshold
        .unwrap_or(crate::gate::PASS_RATE_FLOOR);
    anyhow::ensure!(
        target_pass.is_finite() && (0.0..=1.0).contains(&target_pass),
        "invalid statistical pass threshold"
    );
    let mut run_status = RunStatus::Completed;
    let mut terminal_failure = None;
    let mut resource_usage = ResourceUsage {
        tool_call_count: 0,
        model_call_count: 0,
        token_usage_prompt: 0,
        token_usage_completion: 0,
    };
    let mut inference_ms = 0u64;
    let mut tool_ms = 0u64;
    for i in 0..sample_count {
        // Share execution deadlines, completion checks and grading with the
        // deterministic runner so statistical mode cannot bypass those gates.
        let trial =
            crate::deterministic::run(manifest.clone(), corpus, sandbox, orchestrator).await?;
        if trial.run_status == RunStatus::Failed {
            run_status = RunStatus::Failed;
        } else if trial.run_status == RunStatus::Incomplete && run_status != RunStatus::Failed {
            run_status = RunStatus::Incomplete;
        }
        if trial.run_status != RunStatus::Completed && terminal_failure.is_none() {
            terminal_failure = Some(format!(
                "trial {} {:?}: {}",
                i + 1,
                trial.run_status,
                trial
                    .failure_classification
                    .as_deref()
                    .unwrap_or("execution did not complete")
            ));
        }
        if crate::honesty::scenario_counts_as_pass(&trial) {
            passes += 1;
        }
        durations.push(trial.timing.wall_clock_duration_ms as f64);
        inference_ms = inference_ms.saturating_add(trial.timing.model_inference_duration_ms);
        tool_ms = tool_ms.saturating_add(trial.timing.tool_execution_duration_ms);
        resource_usage.tool_call_count = resource_usage
            .tool_call_count
            .saturating_add(trial.resource_usage.tool_call_count);
        resource_usage.model_call_count = resource_usage
            .model_call_count
            .saturating_add(trial.resource_usage.model_call_count);
        resource_usage.token_usage_prompt = resource_usage
            .token_usage_prompt
            .saturating_add(trial.resource_usage.token_usage_prompt);
        resource_usage.token_usage_completion = resource_usage
            .token_usage_completion
            .saturating_add(trial.resource_usage.token_usage_completion);
        trials.push(trial);
    }

    let pass_rate = passes as f64 / sample_count as f64;
    let mean_duration = durations.iter().sum::<f64>() / durations.len() as f64;
    let mut sorted_durations = durations.clone();
    sorted_durations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median_duration = sorted_durations[sorted_durations.len() / 2];
    let variance = durations
        .iter()
        .map(|v| {
            let diff = mean_duration - *v;
            diff * diff
        })
        .sum::<f64>()
        / durations.len() as f64;
    let standard_deviation = variance.sqrt();
    let margin_of_error = 1.96 * (standard_deviation / (sample_count as f64).sqrt());
    let passed = run_status == RunStatus::Completed && pass_rate >= target_pass;

    let stats = StatisticalSummary {
        trials,
        sample_count,
        pass_rate,
        mean_duration_ms: mean_duration,
        median_duration_ms: median_duration,
        standard_deviation_ms: standard_deviation,
        confidence_interval_95_lower: mean_duration - margin_of_error,
        confidence_interval_95_upper: mean_duration + margin_of_error,
        baseline_comparison: None,
        is_sample_size_sufficient: sample_count >= 5,
    };

    Ok(EvaluationResult {
        scenario_id: manifest.scenario_id.clone(),
        run_status,
        timing: TimingBreakdown {
            wall_clock_duration_ms: start_time.elapsed().as_millis() as u64,
            model_inference_duration_ms: inference_ms,
            tool_execution_duration_ms: tool_ms,
        },
        resource_usage,
        patch_digest: None,
        output_digest: None,
        trace_correlation_id: format!("trace-{}", uuid::Uuid::new_v4()),
        statistics: Some(stats),
        passed,
        failure_classification: terminal_failure.or_else(|| {
            if !passed {
                Some("Statistical pass rate below threshold".to_string())
            } else {
                None
            }
        }),
    })
}

#[cfg(test)]
mod tests;

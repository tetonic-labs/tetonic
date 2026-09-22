use crate::graders::{self, GraderContext};
use crate::honesty::normalize_pass;
use crate::manifest::EvaluationManifest;
use crate::result::{EvaluationResult, RunStatus};
use crate::traits::{AgentOrchestrator, CorpusProvider, SandboxProvider};

pub async fn run(
    manifest: EvaluationManifest,
    corpus: &dyn CorpusProvider,
    sandbox: &dyn SandboxProvider,
    orchestrator: &dyn AgentOrchestrator,
) -> anyhow::Result<EvaluationResult> {
    tracing::info!(
        "Starting deterministic evaluation for scenario: {}",
        manifest.scenario_id
    );

    let snapshot_id = manifest
        .repository_snapshot_id
        .as_deref()
        .unwrap_or("default");
    let workspace = corpus.mount_snapshot(snapshot_id).await?;
    let evaluation = async {
        sandbox.setup_sandbox(&workspace, &manifest).await?;

        let wall = std::time::Duration::from_secs(
            manifest.limits.max_wall_clock_duration_secs.max(1) as u64,
        );
        let exec_result =
            match tokio::time::timeout(wall, orchestrator.run_task(&workspace, &manifest)).await {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => return Err(e),
                Err(_elapsed) => crate::traits::AgentExecutionResult {
                    patch_digest: None,
                    output_digest: None,
                    resource_usage: crate::result::ResourceUsage {
                        tool_call_count: 0,
                        model_call_count: 0,
                        token_usage_prompt: 0,
                        token_usage_completion: 0,
                    },
                    timing: crate::result::TimingBreakdown {
                        wall_clock_duration_ms: wall.as_millis() as u64,
                        model_inference_duration_ms: 0,
                        tool_execution_duration_ms: 0,
                    },
                    touched_files: Vec::new(),
                    outbound_texts: Vec::new(),
                    found_secrets: false,
                    run_status: RunStatus::Incomplete,
                    failure_classification: Some("wall_clock_limit".into()),
                },
            };

        let interrupted = !matches!(exec_result.run_status, RunStatus::Completed);
        let ctx = GraderContext {
            output_digest: exec_result.output_digest.clone(),
            touched_files: exec_result.touched_files.clone(),
            found_secrets: exec_result.found_secrets,
            workspace: workspace.clone(),
        };
        let grade_failure = if interrupted {
            Some("execution_incomplete")
        } else {
            graders::grade(&manifest.graders, &ctx).await?
        };
        let grader_pass = grade_failure.is_none();

        let result = normalize_pass(EvaluationResult {
            scenario_id: manifest.scenario_id.clone(),
            run_status: if grade_failure == Some("manual_review_required") {
                RunStatus::Incomplete
            } else {
                exec_result.run_status
            },
            timing: exec_result.timing,
            resource_usage: exec_result.resource_usage,
            patch_digest: exec_result.patch_digest,
            output_digest: exec_result.output_digest,
            trace_correlation_id: format!("trace-{}", uuid::Uuid::new_v4()),
            statistics: None,
            passed: grader_pass,
            failure_classification: exec_result.failure_classification.or_else(|| {
                if grader_pass {
                    None
                } else {
                    grade_failure.map(str::to_owned)
                }
            }),
        });

        tracing::info!(
            "Deterministic evaluation completed in {} ms (passed={} status={:?})",
            result.timing.wall_clock_duration_ms,
            result.passed,
            result.run_status
        );
        Ok::<_, anyhow::Error>(result)
    }
    .await;
    let cleanup = corpus.unmount_snapshot(&workspace).await;
    match (evaluation, cleanup) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error.context("evaluation workspace cleanup failed")),
        (Err(error), Err(cleanup)) => {
            Err(error.context(format!("workspace cleanup also failed: {cleanup}")))
        }
    }
}

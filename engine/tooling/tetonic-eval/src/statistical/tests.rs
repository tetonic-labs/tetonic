use super::*;
use crate::traits::AgentExecutionResult;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct Fixture {
    mounts: AtomicUsize,
    unmounts: AtomicUsize,
    setup_error: bool,
}
#[async_trait]
impl CorpusProvider for Fixture {
    async fn mount_snapshot(&self, _: &str) -> anyhow::Result<PathBuf> {
        self.mounts.fetch_add(1, Ordering::SeqCst);
        Ok(PathBuf::from("synthetic-fixture"))
    }
    async fn unmount_snapshot(&self, _: &Path) -> anyhow::Result<()> {
        self.unmounts.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
#[async_trait]
impl SandboxProvider for Fixture {
    async fn setup_sandbox(&self, _: &Path, _: &EvaluationManifest) -> anyhow::Result<()> {
        anyhow::ensure!(!self.setup_error, "injected setup failure");
        Ok(())
    }
}
struct Executor {
    statuses: Vec<RunStatus>,
    calls: AtomicUsize,
    error: bool,
    stall: bool,
}
#[async_trait(?Send)]
impl AgentOrchestrator for Executor {
    async fn run_task(
        &self,
        _: &Path,
        _: &EvaluationManifest,
    ) -> anyhow::Result<AgentExecutionResult> {
        anyhow::ensure!(!self.error, "injected execution failure");
        if self.stall {
            std::future::pending::<()>().await;
        }
        let i = self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(AgentExecutionResult {
            patch_digest: None,
            output_digest: Some("correct".into()),
            resource_usage: ResourceUsage {
                tool_call_count: 2,
                model_call_count: 3,
                token_usage_prompt: 4,
                token_usage_completion: 5,
            },
            timing: TimingBreakdown {
                wall_clock_duration_ms: 10,
                model_inference_duration_ms: 6,
                tool_execution_duration_ms: 4,
            },
            touched_files: vec![],
            outbound_texts: vec![],
            found_secrets: false,
            run_status: self.statuses[i % self.statuses.len()].clone(),
            failure_classification: None,
        })
    }
}
fn executor(statuses: Vec<RunStatus>) -> Executor {
    Executor {
        statuses,
        calls: AtomicUsize::new(0),
        error: false,
        stall: false,
    }
}
fn manifest() -> EvaluationManifest {
    let mut value: serde_json::Value =
        serde_json::from_str(include_str!("../../examples/statistical_manifest.json")).unwrap();
    value["graders"] = serde_json::json!([]);
    let mut m: EvaluationManifest = serde_json::from_value(value).unwrap();
    m.graders = vec![crate::manifest::GraderType::ExactMatch {
        expected_patch_digest: "correct".into(),
    }];
    m.expected_pass_threshold = Some(0.0); // Even a permissive threshold cannot approve incomplete work.
    m.limits.max_wall_clock_duration_secs = 1;
    m
}
#[tokio::test]
async fn terminal_failures_never_count_as_success_with_matching_output() {
    for status in [RunStatus::Failed, RunStatus::Incomplete] {
        let fixture = Fixture::default();
        let result = run(
            manifest(),
            &fixture,
            &fixture,
            &executor(vec![status.clone()]),
        )
        .await
        .unwrap();
        assert_eq!(result.run_status, status);
        assert!(!result.passed);
        assert_eq!(result.statistics.unwrap().pass_rate, 0.0);
        assert_eq!(
            fixture.unmounts.load(Ordering::SeqCst),
            STATISTICAL_TRIALS as usize
        );
    }
}
#[tokio::test]
async fn successful_trials_aggregate_actual_resources_and_timing() {
    let fixture = Fixture::default();
    let result = run(
        manifest(),
        &fixture,
        &fixture,
        &executor(vec![RunStatus::Completed]),
    )
    .await
    .unwrap();
    assert!(result.passed);
    assert_eq!(result.run_status, RunStatus::Completed);
    assert_eq!(
        result.resource_usage.model_call_count,
        3 * STATISTICAL_TRIALS
    );
    assert_eq!(
        result.resource_usage.token_usage_completion,
        5 * STATISTICAL_TRIALS
    );
    assert_eq!(
        result.timing.model_inference_duration_ms,
        6 * u64::from(STATISTICAL_TRIALS)
    );
    assert_eq!(result.statistics.unwrap().pass_rate, 1.0);
}
#[tokio::test]
async fn mixed_trials_preserve_failed_status_and_real_pass_rate() {
    let fixture = Fixture::default();
    let result = run(
        manifest(),
        &fixture,
        &fixture,
        &executor(vec![
            RunStatus::Completed,
            RunStatus::Incomplete,
            RunStatus::Failed,
        ]),
    )
    .await
    .unwrap();
    assert_eq!(result.run_status, RunStatus::Failed);
    assert!(!result.passed);
    let encoded = serde_json::to_vec(&result).unwrap();
    let restored: EvaluationResult = serde_json::from_slice(&encoded).unwrap();
    let statistics = restored.statistics.unwrap();
    let rate = statistics.pass_rate;
    assert!(rate > 0.0 && rate < 1.0);
    assert_eq!(statistics.trials.len(), STATISTICAL_TRIALS as usize);
    assert_eq!(statistics.trials[0].run_status, RunStatus::Completed);
    assert_eq!(statistics.trials[1].run_status, RunStatus::Incomplete);
    assert_eq!(statistics.trials[2].run_status, RunStatus::Failed);
    assert!(statistics
        .trials
        .iter()
        .all(|trial| trial.statistics.is_none()));
    assert_eq!(
        statistics.trials[0].output_digest.as_deref(),
        Some("correct")
    );
    let passes = statistics
        .trials
        .iter()
        .filter(|trial| crate::honesty::scenario_counts_as_pass(trial))
        .count();
    assert_eq!(rate, passes as f64 / statistics.trials.len() as f64);
    let completion_tokens: u32 = statistics
        .trials
        .iter()
        .map(|trial| trial.resource_usage.token_usage_completion)
        .sum();
    assert_eq!(
        completion_tokens,
        restored.resource_usage.token_usage_completion
    );
}
#[tokio::test]
async fn setup_and_execution_errors_still_unmount() {
    for setup_error in [false, true] {
        let fixture = Fixture {
            setup_error,
            ..Default::default()
        };
        let mut exec = executor(vec![RunStatus::Completed]);
        exec.error = true;
        assert!(run(manifest(), &fixture, &fixture, &exec).await.is_err());
        assert_eq!(fixture.mounts.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.unmounts.load(Ordering::SeqCst), 1);
    }
}
#[tokio::test]
async fn stalled_trial_is_incomplete_and_cleans_up() {
    let fixture = Fixture::default();
    let mut exec = executor(vec![RunStatus::Completed]);
    exec.stall = true;
    let result = crate::deterministic::run(manifest(), &fixture, &fixture, &exec)
        .await
        .unwrap();
    assert_eq!(result.run_status, RunStatus::Incomplete);
    assert!(!result.passed);
    assert_eq!(fixture.unmounts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn unattended_manual_review_and_empty_grading_never_pass() {
    for manual in [true, false] {
        let fixture = Fixture::default();
        let mut manifest = manifest();
        manifest.graders = if manual {
            vec![crate::manifest::GraderType::ManualReview {
                rubric: "Human approval required".into(),
            }]
        } else {
            vec![]
        };
        let result = crate::deterministic::run(
            manifest,
            &fixture,
            &fixture,
            &executor(vec![RunStatus::Completed]),
        )
        .await
        .unwrap();
        assert!(!result.passed);
        assert_eq!(
            result.failure_classification.as_deref(),
            Some(if manual {
                "manual_review_required"
            } else {
                "no_graders"
            })
        );
        if manual {
            assert_eq!(result.run_status, RunStatus::Incomplete);
        }
        assert_eq!(fixture.unmounts.load(Ordering::SeqCst), 1);
    }
}

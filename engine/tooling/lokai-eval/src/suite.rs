use crate::gate::{self, PASS_RATE_FLOOR};
use crate::manifest::{EvaluationManifest, EvaluationMode};
use crate::result::EvaluationResult;
use crate::traits::{AgentOrchestrator, CorpusProvider, SandboxProvider};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiteResult {
    pub suite: String,
    pub floor: f64,
    pub pass_rate: f64,
    pub passed: bool,
    pub scenarios: Vec<EvaluationResult>,
}

pub async fn run_ids(
    corpus_root: &std::path::Path,
    ids: &[&str],
    suite_name: &str,
    mode: EvaluationMode,
    corpus: &dyn CorpusProvider,
    sandbox: &dyn SandboxProvider,
    orchestrator: &dyn AgentOrchestrator,
) -> anyhow::Result<SuiteResult> {
    let mut scenarios = Vec::new();
    for id in ids {
        let manifest: EvaluationManifest = crate::corpus::load_manifest(corpus_root, id)?;
        let result = match mode {
            EvaluationMode::Deterministic => {
                manifest.validate_deterministic()?;
                crate::deterministic::run(manifest, corpus, sandbox, orchestrator).await?
            }
            EvaluationMode::Statistical => {
                crate::statistical::run(manifest, corpus, sandbox, orchestrator).await?
            }
        };
        scenarios.push(result);
    }
    let passed_count = scenarios
        .iter()
        .filter(|s| crate::honesty::scenario_counts_as_pass(s))
        .count();
    let pass_rate = if scenarios.is_empty() {
        0.0
    } else {
        passed_count as f64 / scenarios.len() as f64
    };
    let floor = PASS_RATE_FLOOR;
    Ok(SuiteResult {
        suite: suite_name.to_string(),
        floor,
        pass_rate,
        passed: pass_rate + f64::EPSILON >= floor,
        scenarios,
    })
}

pub fn parse_subset(name: &str) -> anyhow::Result<&'static [&'static str]> {
    gate::subset_ids(name)
}

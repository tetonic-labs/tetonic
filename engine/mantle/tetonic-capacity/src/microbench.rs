//! Ollama microbench probes (raw-short gate).

use crate::bench::{BenchReport, BenchSuite, SuiteResult};
use crate::client::{ClientError, InferenceClient};
use crate::gates::{evaluate_gates, GatePolicy};

pub const RAW_SHORT_PROMPT: &str = "Reply with exactly the word: ready.";

#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("client: {0}")]
    Client(#[from] ClientError),
}

pub struct ChatOnceResult {
    pub wall_s: f64,
    pub prompt_tokens: u32,
    pub eval_tokens: u32,
    pub prefill_tps: f64,
    pub decode_tps: f64,
}

pub async fn warmup(
    client: &dyn InferenceClient,
    model: &str,
    num_ctx: u32,
) -> Result<(), ProbeError> {
    client.chat_once(model, "hi", 1, Some(num_ctx)).await?;
    Ok(())
}

pub async fn run_raw_short(
    client: &dyn InferenceClient,
    model: &str,
    num_ctx: u32,
) -> Result<(SuiteResult, Option<crate::profile::ObservedPlacement>), ProbeError> {
    let data = client
        .chat_once(model, RAW_SHORT_PROMPT, 64, Some(num_ctx))
        .await?;
    let short = SuiteResult {
        wall_s: data.wall_s,
        prompt_tokens: data.prompt_tokens,
        eval_tokens: data.eval_tokens,
        prefill_tps: data.prefill_tps,
        decode_tps: data.decode_tps,
    };
    let observed = client.fetch_observed_placement(model).await;
    Ok((short, observed))
}

pub fn bench_report_from_short(
    model: &str,
    ollama_base: &str,
    short: SuiteResult,
    observed: Option<crate::profile::ObservedPlacement>,
    policy: &GatePolicy,
) -> BenchReport {
    let mut suites = std::collections::HashMap::new();
    suites.insert(BenchSuite::RawShort, short);
    let report = BenchReport {
        schema_version: 1,
        model: model.to_string(),
        ollama_base: ollama_base.to_string(),
        suites,
        observed,
        gates: None,
    };
    let verdict = evaluate_gates(&report, policy);
    report.with_gates(verdict)
}

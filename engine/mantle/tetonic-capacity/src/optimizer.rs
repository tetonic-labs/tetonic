//! Adaptive placement-first optimizer.

use std::cmp::Ordering;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::Arc;

use chrono::Utc;
use tetonic_inference::DEFAULT_MODEL;
use tetonic_memory::{new_id, SharedStore};

use crate::applier::{apply_bench_tag, apply_recipe, estate_model_name, ollama_delete};
use crate::bench::BenchReport;
use crate::bench::BenchSuite;
use crate::bench::SuiteResult;
use crate::client::InferenceClient;
use crate::defaults::LOCAL_NODE_ID;
use crate::detect::detect_hardware;
use crate::gates::GatePolicy;
use crate::job::BENCH_MODEL_TAG;
use crate::job::{JobState, OptimizeDepth, OptimizeOptions, OptimizeOutcome, OptimizeProgress};
use crate::microbench::{bench_report_from_short, run_raw_short, warmup};
use crate::profile::{
    observed_gpu_pct, HardwareSnapshot, InferenceRecipe, ObservedPlacement, ProfileMetrics,
    ProfileSource, RuntimeProfile, TierRole, SCHEMA_VERSION,
};
use crate::store::ProfileStore;

const GPU_TARGET_PCT: f32 = 50.0;
/// Ollama treats 999 as "offload all layers to GPU".
const NUM_GPU_MAX: u32 = 999;
const MAX_CANDIDATES_FULL: usize = 5;
const MAX_CANDIDATES_QUICK: usize = 3;

#[cfg(test)]
#[path = "optimizer_residency_tests.rs"]
mod residency_tests;

#[derive(Debug, thiserror::Error)]
pub enum OptimizeError {
    #[error("cancelled")]
    Cancelled,
    #[error("no tool-capable models installed")]
    NoModels,
    #[error("no profile passed gates")]
    NoViableProfile,
    #[error("{0}")]
    Other(String),
}

struct BenchOutcome {
    short: SuiteResult,
    observed: ObservedPlacement,
    report: BenchReport,
}

/// Highest `num_gpu` in `[1, NUM_GPU_MAX]` where `probe(n)` is true, assuming higher
/// layer counts need more VRAM (if `n` fails, `n+1` fails).
#[cfg_attr(not(test), allow(dead_code))]
pub fn max_feasible_num_gpu(mut probe: impl FnMut(u32) -> bool) -> Option<u32> {
    if probe(NUM_GPU_MAX) {
        return Some(NUM_GPU_MAX);
    }
    let mut low = 1u32;
    let mut high = NUM_GPU_MAX - 1;
    let mut best = None;
    while low <= high {
        let mid = low + (high - low) / 2;
        if probe(mid) {
            best = Some(mid);
            low = mid + 1;
        } else {
            high = mid.saturating_sub(1);
        }
    }
    best
}

pub fn context_values(depth: OptimizeDepth) -> Vec<u32> {
    match depth {
        OptimizeDepth::Quick => vec![4096],
        OptimizeDepth::Full => vec![8192, 4096],
    }
}

pub fn should_skip_smaller_context(
    num_ctx: u32,
    num_gpu: u32,
    report: &BenchReport,
    observed: &ObservedPlacement,
) -> bool {
    num_ctx > 4096
        && num_gpu == NUM_GPU_MAX
        && report.gates.as_ref().is_some_and(|g| g.passed)
        && observed_gpu_pct(observed).is_some_and(|p| p >= GPU_TARGET_PCT)
}

pub fn passes_quick_pick(report: &BenchReport, observed: &ObservedPlacement) -> bool {
    report.gates.as_ref().is_some_and(|g| g.passed)
        && observed_gpu_pct(observed).is_some_and(|p| p >= GPU_TARGET_PCT)
}

pub async fn run_optimize(
    client: Arc<dyn InferenceClient>,
    store: &SharedStore,
    job_id: &str,
    opts: OptimizeOptions,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(OptimizeProgress),
) -> Result<OptimizeOutcome, OptimizeError> {
    let mut emit = |phase: &str, percent: u32, message: &str| {
        on_progress(OptimizeProgress {
            job_id: job_id.to_string(),
            phase: phase.into(),
            percent,
            message: message.into(),
        });
    };

    if cancel.load(AtomicOrdering::Relaxed) {
        return Err(OptimizeError::Cancelled);
    }

    emit("detect", 5, "Detecting hardware");
    if !client.reachable().await {
        return Err(OptimizeError::Other("Ollama not reachable".into()));
    }
    let version = client.version().await;
    let hw = detect_hardware(version);

    emit("detect", 10, "Listing tool-capable models (largest first)");
    let max_models = match opts.depth {
        OptimizeDepth::Quick => MAX_CANDIDATES_QUICK,
        OptimizeDepth::Full => MAX_CANDIDATES_FULL,
    };
    let candidates = tool_capable_candidates_sorted(client.as_ref(), &opts, max_models).await?;
    if candidates.is_empty() {
        return Err(OptimizeError::NoModels);
    }

    let ctx_values = context_values(opts.depth);
    let policy = GatePolicy::default();
    let mut best: Option<(RuntimeProfile, i64)> = None;
    let est_total = estimate_bench_steps(candidates.len(), ctx_values.len());
    let mut bench_step = 0usize;
    let mut quick_done = false;

    for base in &candidates {
        let mut skip_smaller_ctx = false;
        for &num_ctx in &ctx_values {
            if skip_smaller_ctx {
                break;
            }
            if cancel.load(AtomicOrdering::Relaxed) {
                let _ = ollama_delete(client.as_ref(), BENCH_MODEL_TAG).await;
                return Err(OptimizeError::Cancelled);
            }

            bench_step += 1;
            let pct = 10 + ((bench_step * 70) / est_total.max(1)) as u32;
            emit(
                "benchmark",
                pct.min(80),
                &format!("{base} ctx={num_ctx} (gpu search)"),
            );

            let Some((num_gpu, outcome)) = search_best_num_gpu(
                client.as_ref(),
                base,
                num_ctx,
                &policy,
                cancel,
                &mut |msg| {
                    bench_step += 1;
                    let pct = 10 + ((bench_step * 70) / est_total.max(1)) as u32;
                    emit("benchmark", pct.min(80), msg);
                },
            )
            .await
            else {
                continue;
            };

            let score = score_candidate(&outcome.report, num_ctx);
            let profile = build_candidate_profile(
                &hw,
                base,
                num_ctx,
                num_gpu,
                &outcome.short,
                &outcome.observed,
                outcome.report.gates.as_ref().is_some_and(|g| g.passed),
            );
            if best.as_ref().map_or(true, |(_, s)| score > *s) {
                best = Some((profile, score));
            }

            if should_skip_smaller_context(num_ctx, num_gpu, &outcome.report, &outcome.observed) {
                skip_smaller_ctx = true;
            }

            if opts.depth == OptimizeDepth::Quick
                && passes_quick_pick(&outcome.report, &outcome.observed)
            {
                quick_done = true;
                break;
            }
        }
        if quick_done {
            break;
        }
    }

    let _ = ollama_delete(client.as_ref(), BENCH_MODEL_TAG).await;

    let Some((mut profile, _)) = best else {
        return Err(OptimizeError::NoViableProfile);
    };

    if cancel.load(AtomicOrdering::Relaxed) {
        return Err(OptimizeError::Cancelled);
    }

    emit(
        "apply",
        85,
        &format!("Creating {}", profile.recipe.estate_model),
    );
    profile.id = new_id("profile");
    profile.recipe.estate_model = estate_model_name(&profile.recipe.base_model);
    profile.label = format!(
        "{} @ {} ctx",
        profile.recipe.estate_model, profile.recipe.num_ctx
    );
    profile.source = ProfileSource::Reoptimize;
    profile.created_at = Utc::now();

    let path = apply_recipe(client.as_ref(), &profile.recipe, &profile.id)
        .await
        .map_err(|e| OptimizeError::Other(e.to_string()))?;
    profile.recipe.modelfile_path = Some(path.display().to_string());

    // Candidate measurements cannot certify an applied tag that failed to load.
    profile.gates_passed = false;
    if warmup(
        client.as_ref(),
        &profile.recipe.estate_model,
        profile.recipe.num_ctx,
    )
    .await
    .is_ok()
    {
        if let Ok((short, observed)) = run_raw_short(
            client.as_ref(),
            &profile.recipe.estate_model,
            profile.recipe.num_ctx,
        )
        .await
        {
            profile.metrics = ProfileMetrics {
                raw_short_wall_s: short.wall_s,
                raw_short_tps: short.decode_tps,
                ..Default::default()
            };
            let report = bench_report_from_short(
                &profile.recipe.estate_model,
                client.base_url(),
                short,
                observed.clone(),
                &policy,
            );
            profile.gates_passed = report.gates.as_ref().is_some_and(|g| g.passed);
            if let Some(obs) = observed {
                profile.observed = obs;
            }
        }
    }

    emit("persist", 95, "Saving runtime profile");
    let applied = store
        .write({
            let profile = profile.clone();
            let id = profile.id.clone();
            move |db| {
                let ps = ProfileStore::new(db);
                ps.append(&profile)
                    .map_err(|e| OptimizeError::Other(e.to_string()))?;

                if opts.auto_apply && profile.gates_passed {
                    ps.activate(LOCAL_NODE_ID, TierRole::Coder, &id)
                        .map_err(|e| OptimizeError::Other(e.to_string()))?;
                    Ok(Some(id))
                } else {
                    Ok(None)
                }
            }
        })
        .await
        .map_err(|e| OptimizeError::Other(e.to_string()))??;

    emit("done", 100, "Optimize complete");

    Ok(OptimizeOutcome {
        job_id: job_id.to_string(),
        state: JobState::Succeeded,
        profile_ids: vec![profile.id.clone()],
        applied_profile_id: applied,
        error: None,
    })
}

fn estimate_bench_steps(candidates: usize, ctx_slots: usize) -> usize {
    let gpu_probes = (NUM_GPU_MAX as f64).log2().ceil() as usize + 1;
    candidates * ctx_slots * gpu_probes.max(1)
}

async fn search_best_num_gpu(
    client: &dyn InferenceClient,
    base: &str,
    num_ctx: u32,
    policy: &GatePolicy,
    cancel: &AtomicBool,
    on_probe: &mut impl FnMut(&str),
) -> Option<(u32, BenchOutcome)> {
    if cancel.load(AtomicOrdering::Relaxed) {
        return None;
    }
    on_probe(&format!("{base} ctx={num_ctx} num_gpu={NUM_GPU_MAX}"));
    if let Some(outcome) = try_bench_config(client, base, num_ctx, NUM_GPU_MAX, policy).await {
        return Some((NUM_GPU_MAX, outcome));
    }
    let mut best: Option<(u32, BenchOutcome)> = None;
    let mut low = 1u32;
    let mut high = NUM_GPU_MAX - 1;

    while low <= high {
        if cancel.load(AtomicOrdering::Relaxed) {
            return None;
        }
        let mid = low + (high - low) / 2;
        on_probe(&format!("{base} ctx={num_ctx} num_gpu={mid}"));

        if let Some(outcome) = try_bench_config(client, base, num_ctx, mid, policy).await {
            best = Some((mid, outcome));
            if mid == NUM_GPU_MAX {
                break;
            }
            low = mid + 1;
        } else if mid == 0 {
            break;
        } else {
            high = mid - 1;
        }
    }
    best
}

async fn try_bench_config(
    client: &dyn InferenceClient,
    base: &str,
    num_ctx: u32,
    num_gpu: u32,
    policy: &GatePolicy,
) -> Option<BenchOutcome> {
    // A tag rewrite alone does not prove its old runner released its memory.
    if client.unload_model(BENCH_MODEL_TAG).await.is_err() {
        return None;
    }
    if apply_bench_tag(client, BENCH_MODEL_TAG, base, num_ctx, num_gpu)
        .await
        .is_err()
    {
        return None;
    }
    let measured = async {
        warmup(client, BENCH_MODEL_TAG, num_ctx).await?;
        run_raw_short(client, BENCH_MODEL_TAG, num_ctx).await
    }
    .await;
    // Also release after timeout, spill, malformed response or failed warm-up.
    if client.unload_model(BENCH_MODEL_TAG).await.is_err() {
        return None;
    }
    let (short, observed) = measured.ok()?;
    let observed = observed?;
    let report = bench_report_from_short(
        BENCH_MODEL_TAG,
        client.base_url(),
        short.clone(),
        Some(observed.clone()),
        policy,
    );
    if !report.gates.as_ref().is_some_and(|g| g.passed) {
        return None;
    }
    Some(BenchOutcome {
        short,
        observed,
        report,
    })
}

async fn tool_capable_candidates_sorted(
    client: &dyn InferenceClient,
    opts: &OptimizeOptions,
    max_models: usize,
) -> Result<Vec<String>, OptimizeError> {
    let mut raw = if !opts.base_models.is_empty() {
        opts.base_models.clone()
    } else {
        tool_capable_installed(client).await?
    };
    if raw.is_empty() {
        return Ok(vec![]);
    }

    let mut sized: Vec<(String, f64)> = Vec::with_capacity(raw.len());
    for m in &raw {
        let param_b = client
            .model_info(m)
            .await
            .ok()
            .and_then(|info| info.param_b)
            .unwrap_or(0.0);
        sized.push((m.clone(), param_b));
    }
    sized.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
    raw = sized.into_iter().map(|(m, _)| m).collect();
    raw.truncate(max_models);
    Ok(raw)
}

async fn tool_capable_installed(
    client: &dyn InferenceClient,
) -> Result<Vec<String>, OptimizeError> {
    let installed = client
        .list_models()
        .await
        .map_err(|e| OptimizeError::Other(e.to_string()))?;
    let mut out = Vec::new();
    for m in &installed {
        if crate::applier::is_estate_or_bench_tag(m) {
            continue;
        }
        let caps = client.model_capabilities(m).await.unwrap_or_default();
        if caps.is_empty() || caps.iter().any(|c| c == "tools") {
            out.push(m.clone());
        }
    }
    if out.is_empty() && installed.iter().any(|m| m == DEFAULT_MODEL) {
        out.push(DEFAULT_MODEL.to_string());
    }
    Ok(out)
}

fn score_candidate(report: &BenchReport, num_ctx: u32) -> i64 {
    let mut score = 0i64;
    if report.gates.as_ref().is_some_and(|g| g.passed) {
        score += 1000;
    }
    if let Some(s) = report.suites.get(&BenchSuite::RawShort) {
        score -= (s.wall_s * 10.0) as i64;
    }
    if let Some(obs) = &report.observed {
        if let Some(pct) = observed_gpu_pct(obs) {
            score += (pct * 5.0) as i64;
        }
    }
    score -= (num_ctx / 4096) as i64;
    score
}

fn build_candidate_profile(
    hw: &HardwareSnapshot,
    base_model: &str,
    num_ctx: u32,
    num_gpu: u32,
    short: &SuiteResult,
    observed: &ObservedPlacement,
    gates_passed: bool,
) -> RuntimeProfile {
    RuntimeProfile {
        schema_version: SCHEMA_VERSION,
        id: "pending".into(),
        label: base_model.into(),
        created_at: Utc::now(),
        node_id: LOCAL_NODE_ID.into(),
        role: TierRole::Coder,
        source: ProfileSource::Reoptimize,
        hardware: hw.clone(),
        recipe: InferenceRecipe {
            base_model: base_model.to_string(),
            estate_model: estate_model_name(base_model),
            num_ctx,
            num_gpu: Some(num_gpu),
            keep_alive: "30m".into(),
            modelfile_path: None,
            env_hints: vec![],
            draft_model: None,
            draft_count: None,
        },
        observed: observed.clone(),
        metrics: ProfileMetrics {
            raw_short_wall_s: short.wall_s,
            raw_short_tps: short.decode_tps,
            ..Default::default()
        },
        gates_passed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::{BenchReport, SuiteResult};
    use crate::profile::ObservedPlacement;
    use chrono::Utc;

    #[test]
    fn max_feasible_num_gpu_binary_search() {
        let feasible = |n: u32| n <= 40;
        assert_eq!(max_feasible_num_gpu(feasible), Some(40));
    }

    #[test]
    fn max_feasible_full_offload() {
        let feasible = |n: u32| n == NUM_GPU_MAX;
        assert_eq!(max_feasible_num_gpu(feasible), Some(NUM_GPU_MAX));
    }

    #[test]
    fn max_feasible_none_fit() {
        let feasible = |_n: u32| false;
        assert_eq!(max_feasible_num_gpu(feasible), None);
    }

    #[test]
    fn context_values_order_largest_first_for_full() {
        assert_eq!(context_values(OptimizeDepth::Full), vec![8192, 4096]);
        assert_eq!(context_values(OptimizeDepth::Quick), vec![4096]);
    }

    #[test]
    fn skip_smaller_ctx_when_large_passes_at_full_gpu() {
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
                    prefill_tps: 1.0,
                    decode_tps: 40.0,
                },
            )]
            .into_iter()
            .collect(),
            observed: Some(ObservedPlacement {
                gpu_processor_pct: Some(85.0),
                processor_split: None,
                vram_used_mb: 18_000,
                resident_model: "m".into(),
                load_wall_s: 0.0,
                measured_at: Utc::now(),
            }),
            gates: Some(crate::gates::GateVerdict {
                passed: true,
                failures: vec![],
            }),
        };
        let obs = report.observed.as_ref().unwrap();
        assert!(should_skip_smaller_context(8192, NUM_GPU_MAX, &report, obs));
        assert!(!should_skip_smaller_context(
            4096,
            NUM_GPU_MAX,
            &report,
            obs
        ));
    }

    #[test]
    fn score_prefers_passing_gates() {
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
                    prefill_tps: 1.0,
                    decode_tps: 40.0,
                },
            )]
            .into_iter()
            .collect(),
            observed: Some(ObservedPlacement {
                gpu_processor_pct: Some(85.0),
                processor_split: None,
                vram_used_mb: 18_000,
                resident_model: "m".into(),
                load_wall_s: 0.0,
                measured_at: Utc::now(),
            }),
            gates: Some(crate::gates::GateVerdict {
                passed: true,
                failures: vec![],
            }),
        };
        assert!(score_candidate(&report, 4096) > 900);
    }
}

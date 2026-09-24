//! Inference fabric (Phase A slice).
//!
//! Defines the [`InferenceProvider`] trait the engine depends on, and a
//! single-node [`OllamaProvider`] behind it, plus opt-in [`hosted`] adapters.
//! Production HTTP goes through [`tetonic_egress::EgressGuard`]: local/fabric
//! enrollment and explicit hosted endpoint grants are separate permissions.
//!
//! Full fabric (pooled/cluster providers, `FabricSnapshot`, model tiers) is
//! additive on this trait — see contracts/inference-fabric-v1.md.

/// Default Ollama model for CLI, daemon, and bench (tool-capable production default).
pub const DEFAULT_MODEL: &str = "qwen3.5:latest";

mod attempt;
mod compute_registry;
mod dispatch;
mod fabric;
mod fabric_node_provider;
pub mod hosted;
mod placement;
mod placement_engine;
mod pooled;
mod residency;
mod worker_eligibility;
pub mod decoupled;

pub use decoupled::{
    CircuitState, DecoupledInferenceRouter, EndpointLease, EndpointTier, InferenceEndpoint,
};

pub use fabric_node_provider::FabricNodeProvider;

pub use attempt::{ActiveJobRegistry, AttemptBindingFlags, StaleResultError};
pub use compute_registry::{AuthorizedComputeTarget, ComputeTargetRegistry, TrustAssignmentRecord};
pub use dispatch::{
    aggregate_chat_classification, dispatch_request_for_chat, dispatch_request_for_job,
    evaluate_remote_dispatch, evaluate_remote_job_dispatch, plan_remote_attempt,
    redact_chat_request_for_remote, redact_messages_for_failover, stamp_request_classification,
    RedactionManifest, RemoteAttemptPlan,
};
pub use fabric::{
    new_fabric_attempt_id, new_fabric_job_id, AuditEnvelope, ComputeReceipt, FabricJob,
    FabricJobResult, FabricSnapshot, GenUsageSerde, JobPriority, JobStatus, NodeCapacityHealth,
    NodeInfo, ReceiptDirection, ReceiptKind, SampleOptions, LOCAL_NODE_ID,
};
pub use placement::{DispatchPlacementReport, DispatchPlacementSink};
pub use placement_engine::{
    evaluate_placement, evaluate_typed_job_placement, placement_request_from_chat,
    placement_request_from_job, NETWORK_DENY_ALL_CAPABILITY,
};
pub use pooled::PooledProvider;
pub use tetonic_domain::{
    Classification, ClassificationSource, DataClass, DisclosureTier, DispatchDecision,
    DispatchDenied, DispatchDestination, DispatchGuard, DispatchRequest, PolicyVersion,
    CLASSIFICATION_POLICY_VERSION,
};
pub use worker_eligibility::{
    evaluate_worker_eligibility, placement_reason_is_local_only, WorkerEligibilityInput,
};

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tetonic_egress::{EgressError, EgressGuard};
use thiserror::Error;

use std::sync::atomic::{AtomicI64, Ordering};

/// A chat message. Serializes to Ollama's `/api/chat` message shape.
#[derive(Debug, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// Links a tool-result message to the assistant `tool_calls` id (H3-1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Memoized token count of this (immutable) message. Not serialized; filled
    /// lazily the first time a tokenizer counts it, so a long session doesn't
    /// re-tokenize its whole history every turn. Atomic so `Message` is `Send + Sync`.
    #[serde(skip)]
    token_cache: AtomicI64,
}

impl Clone for Message {
    fn clone(&self) -> Self {
        Self {
            role: self.role.clone(),
            content: self.content.clone(),
            tool_calls: self.tool_calls.clone(),
            tool_name: self.tool_name.clone(),
            tool_call_id: self.tool_call_id.clone(),
            token_cache: AtomicI64::new(self.token_cache.load(Ordering::Relaxed)),
        }
    }
}

impl Message {
    /// Invalidate model-specific token accounting when rebinding inference.
    pub fn invalidate_token_count(&self) {
        self.token_cache.store(-1, Ordering::Relaxed);
    }
    pub fn system(c: impl Into<String>) -> Self {
        Self::bare("system", c)
    }
    pub fn user(c: impl Into<String>) -> Self {
        Self::bare("user", c)
    }
    pub fn assistant(c: impl Into<String>) -> Self {
        Self::bare("assistant", c)
    }
    pub fn tool(name: impl Into<String>, c: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: c.into(),
            tool_calls: None,
            tool_name: Some(name.into()),
            tool_call_id: None,
            token_cache: AtomicI64::new(-1),
        }
    }
    pub fn with_tool_call_id(mut self, id: impl Into<String>) -> Self {
        let id = id.into();
        if !id.is_empty() {
            self.tool_call_id = Some(id);
        }
        self
    }
    fn bare(role: &str, c: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: c.into(),
            tool_calls: None,
            tool_name: None,
            tool_call_id: None,
            token_cache: AtomicI64::new(-1),
        }
    }

    /// Attach tool calls to this message (builder style). Useful for scripted
    /// providers in tests and for assembling assistant turns programmatically.
    pub fn with_tool_calls(mut self, tool_calls: Vec<ToolCall>) -> Self {
        self.tool_calls = Some(tool_calls);
        self
    }

    /// Return this message's memoized token count, computing it with `f` on a miss.
    /// The message is immutable once built, so the first count is reused for the
    /// life of the value (and its clones, since the cache is copied on clone).
    pub fn cached_tokens(&self, f: impl FnOnce() -> usize) -> usize {
        let cached = self.token_cache.load(Ordering::Relaxed);
        if cached >= 0 {
            cached as usize
        } else {
            let n = f();
            self.token_cache.store(n as i64, Ordering::Relaxed);
            n
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    /// Ollama returns an object; some models emit a JSON string. Callers coerce.
    #[serde(default)]
    pub arguments: Value,
}

/// A tool advertised to the model (Ollama "tools" format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    #[serde(rename = "type")]
    pub kind: String,
    pub function: FunctionSchema,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionSchema {
    pub name: String,
    pub description: String,
    /// JSON Schema for the arguments (we derive these via `schemars`).
    pub parameters: Value,
}

impl ToolSchema {
    pub fn function(name: &str, description: &str, parameters: Value) -> Self {
        Self {
            kind: "function".into(),
            function: FunctionSchema {
                name: name.into(),
                description: description.into(),
                parameters,
            },
        }
    }
}

/// JSON schema hint for Ollama `format` when tool calling (D7).
pub fn build_tool_call_format(schemas: &[ToolSchema]) -> Value {
    let names: Vec<&str> = schemas.iter().map(|s| s.function.name.as_str()).collect();
    json!({
        "type": "object",
        "properties": {
            "message": {
                "type": "object",
                "properties": {
                    "tool_calls": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "function": {
                                    "type": "object",
                                    "properties": {
                                        "name": { "type": "string", "enum": names },
                                        "arguments": { "type": "object" }
                                    },
                                    "required": ["name", "arguments"]
                                }
                            },
                            "required": ["function"]
                        }
                    }
                }
            }
        }
    })
}

/// Per-call fabric metadata (coordinator → worker). Omitted for local-only paths.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FabricCallMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub step_index: u32,
    /// One user turn (`chat/send`); when this changes, pooled placement resets affinity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<String>,
    /// Infer hop identity. Not the agent Attempt. Expires OBS-02 CONVERGE.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hop_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hop_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hop_attempt_id: Option<String>,
    #[serde(default)]
    pub data_class: DataClass,
    /// Disclosure tier for remote fabric jobs (SEC2-E2-029).
    #[serde(default)]
    pub disclosure_tier: DisclosureTier,
    /// Aggregated outbound payload classification (M2-2 provenance).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_classification: Option<tetonic_domain::ClassificationSummary>,
    /// Max sensitivity of attached context pack / artifacts (M5-3 full-payload classification).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_data_class: Option<DataClass>,
    /// Digest-bound artifacts included in the outbound payload (M5-3).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_artifacts: Vec<tetonic_domain::ArtifactRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_version: Option<tetonic_domain::WorkspaceVersion>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_capabilities: Vec<String>,
    #[serde(default)]
    pub trace_context: tetonic_domain::TraceContext,
    /// Model capability tier for fabric placement (`fast` | `hard`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_tier: Option<String>,
    /// Verification policy requested for remote execution (e.g. "redundant" or "independent_redundant:2").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_policy: Option<String>,
    /// Broker-selected preferred placement target (`local` or worker id). M6-2 Option A.

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_target: Option<String>,
    /// Ordered placement targets from the scheduler (`local` or worker ids). When
    /// non-empty, pooled walks this list only (no independent ranking).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback_order: Vec<String>,
    /// Scheduler decision id stamped by ComputeBroker (M6-2 / M6-3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduler_decision_id: Option<String>,
}

/// Result of the outbound secret scan (H1-1). Never sent to Ollama or fabric.
///
/// Fields are private so callers cannot forge `scanned: true` with a struct
/// literal. Production Infer constructs this only via [`OutboundScan::from_scan`]
/// inside `redact_outbound`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutboundScan {
    scanned: bool,
    high_confidence: bool,
}

impl OutboundScan {
    /// Mark a request as scanned. Production caller is `redact_outbound`.
    /// Worker job ingress may stamp after coordinator scan (does not rescan).
    pub fn from_scan(high_confidence: bool) -> Self {
        Self {
            scanned: true,
            high_confidence,
        }
    }

    pub fn is_scanned(&self) -> bool {
        self.scanned
    }

    pub fn high_confidence(&self) -> bool {
        self.high_confidence
    }

    pub fn blocks_remote(&self) -> bool {
        self.scanned && self.high_confidence
    }
}

/// Hops refuse `chat` unless [`OutboundScan::from_scan`] ran (M4 / DEL-024).
pub fn require_outbound_scan(req: &ChatRequest) -> Result<(), InferenceError> {
    if req.outbound_scan.is_scanned() {
        Ok(())
    } else {
        Err(InferenceError::SecretScanFailed {
            reason: "outbound scan stamp missing".into(),
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChatRequest {
    pub model: String,
    /// Immutable model digest for exact placement when available (M5-2).
    pub model_digest: Option<String>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSchema>,
    pub temperature: f32,
    /// Context window to request from the runtime (Ollama `num_ctx`).
    pub num_ctx: Option<u32>,
    /// Speculative draft model for accelerated token decoding (OPT-501).
    pub draft_model: Option<String>,
    /// Number of tokens to speculate ahead per forward pass.
    pub draft_count: Option<u32>,
    /// How long the runtime should keep the model resident (Ollama `keep_alive`,
    /// e.g. "10m"). Keeping it warm preserves the KV-cache prefix across turns.
    pub keep_alive: Option<String>,
    /// Fabric routing context for remote inference (N1.2 turn affinity).
    pub fabric: Option<FabricCallMeta>,
    /// Ollama structured output / JSON schema (D7). Ignored when unsupported.
    pub response_format: Option<Value>,
    /// Set by `BrokerInferenceProvider` before dispatch. Default is unscanned.
    pub outbound_scan: OutboundScan,
}

#[derive(Debug, Clone, Default)]
pub struct InferenceProvenance {
    pub provider_kind: String,
    pub worker_id: Option<String>,
    pub model: String,
    pub job_id: Option<String>,
    pub attempt_id: Option<String>,
    /// True when this completion used a privacy-redacted prompt after fabric failover.
    pub prompt_redacted: bool,
    /// M2-2: placement decision from dispatch guard (`local_only`, `remote_allowed`, …).
    pub placement_decision: Option<String>,
    pub placement_reason_code: Option<String>,
    pub placement_class: Option<DataClass>,
    pub classification_sources: Vec<String>,
    /// Worker-local monotonic queue wait (M6-3). Never a wall-clock stamp.
    pub worker_queue_ms: Option<u64>,
    /// Worker-local monotonic execute duration (M6-3).
    pub worker_execute_ms: Option<u64>,
    /// Coordinator-side result verification duration (M6-3).
    pub verification_ms: Option<u64>,
    /// Coordinator-observed fabric round-trip (send+recv) in ms.
    pub transfer_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub message: Message,
    /// Generation timing/token counts reported by the runtime at the `done`
    /// chunk (Ollama: `prompt_eval_count`/`eval_count` + durations). Empty when
    /// the runtime doesn't report them. Lets callers surface prefill/decode
    /// throughput — the lever that governs end-to-end latency.
    pub usage: GenUsage,
    pub provenance: InferenceProvenance,
}

/// Per-generation token counts and durations, plus derived throughput. Prefill
/// = prompt evaluation (re-paid every agent step); decode = output generation.
#[derive(Debug, Clone, Default)]
pub struct GenUsage {
    pub prompt_tokens: Option<u64>,
    pub eval_tokens: Option<u64>,
    pub prompt_eval_ms: Option<f64>,
    pub eval_ms: Option<f64>,
}

impl GenUsage {
    /// Prompt-evaluation (prefill) throughput in tokens/sec, if measurable.
    pub fn prefill_tps(&self) -> Option<f64> {
        match (self.prompt_tokens, self.prompt_eval_ms) {
            (Some(t), Some(ms)) if ms > 0.0 => Some(t as f64 / (ms / 1000.0)),
            _ => None,
        }
    }

    /// Output-generation (decode) throughput in tokens/sec, if measurable.
    pub fn decode_tps(&self) -> Option<f64> {
        match (self.eval_tokens, self.eval_ms) {
            (Some(t), Some(ms)) if ms > 0.0 => Some(t as f64 / (ms / 1000.0)),
            _ => None,
        }
    }

    /// Whether the runtime reported any usage at all.
    pub fn reported(&self) -> bool {
        self.prompt_tokens.is_some() || self.eval_tokens.is_some()
    }
}

#[derive(Debug, Error)]
pub enum InferenceError {
    #[error("egress: {0}")]
    Egress(#[from] EgressError),
    #[error("decode: {0}")]
    Decode(String),
    #[error("provider: {0}")]
    Provider(String),
    #[error("preempted on {node_id}")]
    Preempted { node_id: String },
    #[error("worker busy: {node_id}")]
    WorkerBusy { node_id: String },
    #[error(
        "gpu spill detected: GPU residency {gpu_pct:.0}% below {fail_threshold:.0}% threshold"
    )]
    GpuSpillDetected { gpu_pct: f32, fail_threshold: f32 },
    #[error("secret scan failed: {reason}")]
    SecretScanFailed { reason: String },
    #[error("remote dispatch refused: high-confidence secret finding ({reason})")]
    RemoteSecretDenied { reason: String },
    #[error(
        "incomplete stream: server EOF without done marker (tokens_received={tokens_received})"
    )]
    IncompleteStream { tokens_received: bool },
    #[error("stream timeout ({phase}): no data for {elapsed_secs}s")]
    StreamTimeout {
        phase: &'static str,
        elapsed_secs: u64,
    },
}

/// Capabilities + size for one model (from `/api/show`), used for tiering.
#[derive(Debug, Clone, Default)]
pub struct ModelInfo {
    pub capabilities: Vec<String>,
    /// Parameter count in billions (e.g. 9.7), if the runtime reports it.
    pub param_b: Option<f64>,
    pub digest: Option<String>,
    pub quantization: Option<String>,
}

/// Installed model tag from `/api/tags`.
#[derive(Debug, Clone)]
pub struct OllamaModelTag {
    pub name: String,
    pub digest: Option<String>,
}

/// Non-streaming chat timing from [`OllamaProvider::chat_once`].
#[derive(Debug, Clone)]
pub struct OllamaChatOnce {
    pub wall_s: f64,
    pub prompt_tokens: u32,
    pub eval_tokens: u32,
    pub prefill_tps: f64,
    pub decode_tps: f64,
}

fn bench_rate(count: u32, duration_ns: f64) -> f64 {
    if count == 0 || duration_ns <= 0.0 {
        0.0
    } else {
        count as f64 / (duration_ns / 1e9)
    }
}

/// Parse Ollama's `details.parameter_size` ("9.7B", "36.0B", "7B", "350M") into
/// a billions-of-parameters float for size comparison. Best-effort.
fn parse_param_size(s: &str) -> Option<f64> {
    let s = s.trim();
    let (num, mult) = if let Some(n) = s.strip_suffix(['B', 'b']) {
        (n, 1.0)
    } else if let Some(n) = s.strip_suffix(['M', 'm']) {
        (n, 0.001)
    } else {
        (s, 1.0)
    };
    num.trim().parse::<f64>().ok().map(|x| x * mult)
}

fn normalize_ollama_digest(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with("sha256:") {
        trimmed.to_string()
    } else {
        format!("sha256:{trimmed}")
    }
}

/// Compare exact Ollama tags, allowing only the runtime's implicit `:latest`.
pub fn ollama_model_matches(left: &str, right: &str) -> bool {
    fn tag(name: &str) -> String {
        if name.rsplit('/').next().unwrap_or(name).contains(':') {
            name.to_string()
        } else {
            format!("{name}:latest")
        }
    }
    tag(left) == tag(right)
}

/// GPU layer residency [0, 100] from Ollama `/api/ps` `size_vram` / `size`.
fn gpu_residency_pct(size: u64, size_vram: u64) -> Option<f32> {
    if size == 0 {
        None
    } else {
        Some((100.0 * size_vram as f64 / size as f64).min(100.0) as f32)
    }
}

/// Fail threshold aligned with `tetonic_capacity::GatePolicy::default().gpu_pct_fail`.
const GPU_SPILL_FAIL_PCT: f32 = 100.0;

/// Returns GPU residency when it falls below the spill threshold.
fn vram_spill_below_threshold(size: u64, size_vram: u64) -> Option<f32> {
    gpu_residency_pct(size, size_vram).filter(|_| size_vram < size)
}

/// A sink for streamed content tokens. Called as deltas arrive.
pub type TokenSink<'a> = dyn FnMut(&str) + Send + 'a;

#[async_trait]
pub trait InferenceProvider: Send + Sync {
    /// Stream a chat completion. Content deltas are pushed to `on_token` as they
    /// arrive; the assembled final message (including any tool calls) is returned.
    async fn chat(
        &self,
        req: ChatRequest,
        on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError>;

    /// Current fabric capacity (topology + concurrency). Distinct from
    /// [`OllamaProvider::model_capabilities`] — that queries one model's metadata.
    async fn fabric_snapshot(&self) -> FabricSnapshot {
        FabricSnapshot::empty()
    }

    /// Speculatively warm model weights into VRAM (OPT-101).
    async fn prewarm(&self, _model: &str, _keep_alive: Option<&str>) -> Result<(), InferenceError> {
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaChatOptions {
    pub temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_gpu: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_batch: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_ctx: Option<u32>,
    /// Speculative draft model for accelerated token decoding (OPT-501).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft_model: Option<String>,
    /// Number of tokens to speculate ahead per forward pass.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft_count: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct OllamaChatRequestBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub think: Option<bool>,
    pub model: &'a str,
    pub messages: &'a [Message],
    pub stream: bool,
    pub options: OllamaChatOptions,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_alive: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<&'a [ToolSchema]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<&'a Value>,
}

/// Single-node Ollama provider. Talks to `/api/chat` via the egress guard.
pub struct OllamaProvider {
    thinking: Option<bool>,
    base_url: String,
    guard: Arc<EgressGuard>,
    ps_cache: Arc<tokio::sync::RwLock<Option<(Value, std::time::Instant)>>>,
    last_prewarm: Arc<tokio::sync::Mutex<Option<WarmupSuccess>>>,
    admission: Arc<tokio::sync::Mutex<residency::RuntimeAdmission>>,
}

struct WarmupSuccess {
    model: String,
    num_ctx: Option<u32>,
    keep_alive: Option<String>,
    completed: std::time::Instant,
}

impl OllamaProvider {
    const PS_CACHE_TTL: std::time::Duration = std::time::Duration::from_millis(500);

    pub fn new(base_url: impl Into<String>, guard: Arc<EgressGuard>) -> Self {
        let base_url = base_url.into();
        Self {
            thinking: None,
            admission: residency::runtime_admission(&base_url),
            base_url,
            guard,
            ps_cache: Arc::new(tokio::sync::RwLock::new(None)),
            last_prewarm: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    pub fn egress_guard(&self) -> &Arc<EgressGuard> {
        &self.guard
    }

    /// Explicitly select thinking mode for models that support Ollama's `think`
    /// option. None preserves the provider/model default for existing callers.
    pub fn with_thinking(mut self, thinking: Option<bool>) -> Self {
        self.thinking = thinking;
        self
    }

    /// Speculatively pre-warm a model in VRAM (OPT-101).
    pub async fn prewarm(
        &self,
        model: &str,
        keep_alive: Option<&str>,
    ) -> Result<(), InferenceError> {
        self.prewarm_with_context(model, keep_alive, None).await
    }

    /// Match the intended request's context allocation when preparing the local runtime.
    /// Serialize warm-ups and cache only successful completion, so failure/cancellation
    /// can be retried and concurrent callers never mistake an in-flight load for success.
    pub async fn prewarm_with_context(
        &self,
        model: &str,
        keep_alive: Option<&str>,
        num_ctx: Option<u32>,
    ) -> Result<(), InferenceError> {
        let _admission = self.admission.lock().await;
        let ka = keep_alive;
        let mut guard = self.last_prewarm.lock().await;
        self.invalidate_ps_cache().await;
        let ps = self.get_ps_cached().await?;
        let models = ps["models"].as_array().unwrap();
        // Speculative startup work must not load another model beside an
        // unrelated resident runner. Actual demand can still use the runtime.
        if models.iter().any(|m| {
            !m.get("name")
                .or_else(|| m.get("model"))
                .and_then(Value::as_str)
                .is_some_and(|name| ollama_model_matches(name, model))
        }) {
            tracing::debug!(
                model,
                "skipping speculative warm-up: another model is resident"
            );
            return Ok(());
        }
        let allocation_present = models.iter().any(|m| {
            m.get("name")
                .or_else(|| m.get("model"))
                .and_then(Value::as_str)
                .is_some_and(|name| ollama_model_matches(name, model))
                && num_ctx.is_none_or(|ctx| {
                    m.get("context_length").and_then(Value::as_u64) == Some(u64::from(ctx))
                })
        });
        if allocation_present && ka.is_none() {
            drop(guard);
            return self.verify_or_release_allocation(model).await;
        }
        if let Some(previous) = guard.as_ref() {
            if previous.model == model
                && previous.num_ctx == num_ctx
                && previous.keep_alive.as_deref() == ka
                && allocation_present
                && previous.completed.elapsed() < std::time::Duration::from_secs(60)
            {
                drop(guard);
                return self.verify_or_release_allocation(model).await;
            }
        }

        let url = format!("{}/api/generate", self.base_url);
        // A different allocation may replace the previous runner, even if loading fails.
        *guard = None;
        let started = std::time::Instant::now();
        let mut body = json!({
            "model": model,
            "prompt": "",
            "stream": false
        });
        if let Some(ka) = ka {
            body["keep_alive"] = json!(ka);
        }
        if let Some(num_ctx) = num_ctx {
            body["options"] = json!({"num_ctx": num_ctx});
        }

        let response = self
            .guard
            .post_json(&url, &body, "inference:ollama:prewarm")
            .await?;
        if response.get("error").is_some()
            || response.get("done").and_then(Value::as_bool) != Some(true)
        {
            return Err(InferenceError::Provider(
                "model warm-up did not complete successfully".into(),
            ));
        }
        *guard = Some(WarmupSuccess {
            model: model.to_string(),
            num_ctx,
            keep_alive: ka.map(String::from),
            completed: std::time::Instant::now(),
        });
        self.invalidate_ps_cache().await;
        drop(guard);
        self.verify_or_release_allocation(model).await?;
        tracing::debug!(
            model,
            num_ctx,
            elapsed_ms = started.elapsed().as_millis(),
            "model warm-up complete"
        );
        Ok(())
    }

    async fn get_ps_cached(&self) -> Result<Value, InferenceError> {
        {
            let cache = self.ps_cache.read().await;
            if let Some((ref val, ts)) = *cache {
                if ts.elapsed() < Self::PS_CACHE_TTL {
                    return Ok(val.clone());
                }
            }
        }

        // Coalesce concurrent misses, including the initial inventory burst.
        // This async lock also orders invalidation after an in-flight refresh.
        // Failed or canceled requests never install a cache entry.
        let mut cache = self.ps_cache.write().await;
        if let Some((ref val, ts)) = *cache {
            if ts.elapsed() < Self::PS_CACHE_TTL {
                return Ok(val.clone());
            }
        }
        let ps_url = format!("{}/api/ps", self.base_url);
        let fresh = self.guard.get_json(&ps_url, "inference:ollama:ps").await?;
        if fresh.get("error").is_some() || fresh.get("models").and_then(Value::as_array).is_none() {
            return Err(InferenceError::Provider(
                "runtime residency status unavailable".into(),
            ));
        }
        *cache = Some((fresh.clone(), std::time::Instant::now()));
        Ok(fresh)
    }

    async fn invalidate_ps_cache(&self) {
        let mut cache = self.ps_cache.write().await;
        *cache = None;
    }

    /// Check if the specified model is spilling into system RAM.
    async fn check_vram_spill(&self, req_model: &str) -> Result<(), InferenceError> {
        let ps = self.get_ps_cached().await?;
        let model = ps["models"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| {
                m.get("name")
                    .or_else(|| m.get("model"))
                    .and_then(Value::as_str)
                    .is_some_and(|name| ollama_model_matches(name, req_model))
            })
            .ok_or_else(|| {
                InferenceError::Provider("requested model residency unavailable".into())
            })?;
        let size = model
            .get("size")
            .and_then(Value::as_u64)
            .filter(|size| *size > 0);
        let vram = model.get("size_vram").and_then(Value::as_u64);
        let (Some(size), Some(vram)) = (size, vram) else {
            return Err(InferenceError::Provider(
                "requested model GPU placement unavailable".into(),
            ));
        };
        if let Some(gpu_pct) = vram_spill_below_threshold(size, vram) {
            return Err(InferenceError::GpuSpillDetected {
                gpu_pct,
                fail_threshold: GPU_SPILL_FAIL_PCT,
            });
        }
        Ok(())
    }

    async fn verify_or_release_allocation(&self, model: &str) -> Result<(), InferenceError> {
        if let Err(error) = self.check_vram_spill(model).await {
            self.unload_model_inner(model).await?;
            return Err(error);
        }
        Ok(())
    }

    /// Preflight: is the local runtime reachable (and allowed)?
    pub async fn reachable(&self) -> bool {
        let url = format!("{}/api/tags", self.base_url);
        self.guard
            .get_json(&url, "inference:ollama:tags")
            .await
            .is_ok()
    }

    /// Embed one or more texts via the local runtime's `/api/embed`, through the
    /// egress guard (loopback only). Returns one vector per input, in order. Used
    /// by the local code index — embeddings never leave the machine.
    pub async fn embed(
        &self,
        model: &str,
        inputs: &[String],
    ) -> Result<Vec<Vec<f32>>, InferenceError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        let _admission = self.admission.lock().await;
        let url = format!("{}/api/embed", self.base_url);
        let body = json!({ "model": model, "input": inputs });
        let v = self
            .guard
            .post_json(&url, &body, "inference:ollama:embed")
            .await?;
        if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
            return Err(InferenceError::Provider(err.to_string()));
        }
        let rows = v
            .get("embeddings")
            .and_then(|e| e.as_array())
            .ok_or_else(|| InferenceError::Decode("no `embeddings` in response".into()))?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let vec = row
                .as_array()
                .ok_or_else(|| InferenceError::Decode("embedding row not an array".into()))?
                .iter()
                .map(|x| x.as_f64().unwrap_or(0.0) as f32)
                .collect();
            out.push(vec);
        }
        Ok(out)
    }

    /// Query a model's declared capabilities via `/api/show` (e.g. `tools`,
    /// `completion`, `embedding`, `vision`). Used for a fast preflight so we can
    /// fail *before* a session starts instead of erroring mid-run. Returns an
    /// empty vec on older runtimes that don't report capabilities (caller treats
    /// empty as "unknown" and proceeds).
    pub async fn model_capabilities(&self, model: &str) -> Result<Vec<String>, InferenceError> {
        let url = format!("{}/api/show", self.base_url);
        let body = json!({ "model": model });
        let v = self
            .guard
            .post_json(&url, &body, "inference:ollama:show")
            .await?;
        let caps = v
            .get("capabilities")
            .and_then(|c| c.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        Ok(caps)
    }

    /// Capabilities + parameter size for a model in one `/api/show` call. Used to
    /// build model tiers (pick the largest tool-capable model for hard work)
    /// without a second round-trip. `param_b` is the parameter count in billions,
    /// parsed from Ollama's `details.parameter_size` (e.g. "9.7B"); `None` when
    /// the runtime doesn't report it.
    pub async fn model_info(&self, model: &str) -> Result<ModelInfo, InferenceError> {
        let url = format!("{}/api/show", self.base_url);
        let body = json!({ "model": model });
        let v = self
            .guard
            .post_json(&url, &body, "inference:ollama:show")
            .await?;
        let capabilities = v
            .get("capabilities")
            .and_then(|c| c.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let param_b = v
            .get("details")
            .and_then(|d| d.get("parameter_size"))
            .and_then(|p| p.as_str())
            .and_then(parse_param_size);
        let quantization = v
            .get("details")
            .and_then(|d| d.get("quantization_level"))
            .and_then(|p| p.as_str())
            .map(String::from);
        let digest = v
            .get("digest")
            .and_then(|d| d.as_str())
            .map(normalize_ollama_digest);
        Ok(ModelInfo {
            capabilities,
            param_b,
            digest,
            quantization,
        })
    }

    /// Ollama version string from `/api/version`, if reachable.
    pub async fn version(&self) -> Option<String> {
        let url = format!("{}/api/version", self.base_url);
        self.guard
            .get_json(&url, "inference:ollama:version")
            .await
            .ok()
            .and_then(|v| v.get("version").and_then(|x| x.as_str()).map(String::from))
    }

    /// Raw JSON from `/api/ps` (running models). Empty object on failure.
    pub async fn ps_json(&self) -> Value {
        self.get_ps_cached()
            .await
            .unwrap_or_else(|_| json!({ "models": [] }))
    }

    /// Create or replace a model from an inline Modelfile (`/api/create`).
    ///
    /// Ollama 0.6+ expects `from` + `parameters`; older builds accept inline `modelfile`.
    pub async fn create_model(&self, name: &str, modelfile: &str) -> Result<(), InferenceError> {
        if let Some((from, parameters)) = parse_modelfile_for_create(modelfile) {
            match self
                .post_create(json!({
                    "model": name,
                    "from": from,
                    "parameters": parameters,
                    "stream": false,
                }))
                .await
            {
                Ok(()) => return Ok(()),
                Err(e) => {
                    // Older Ollama builds may reject structured create — fall back below.
                    if let InferenceError::Provider(msg) = &e {
                        if !msg.contains("unknown field") && !msg.contains("invalid field") {
                            return Err(e);
                        }
                    } else {
                        return Err(e);
                    }
                }
            }
        }
        self.post_create(json!({
            "model": name,
            "modelfile": modelfile,
            "stream": false,
        }))
        .await
    }

    async fn post_create(&self, body: Value) -> Result<(), InferenceError> {
        let url = format!("{}/api/create", self.base_url);
        let v = self
            .guard
            .post_json(&url, &body, "inference:ollama:create")
            .await?;
        if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
            return Err(InferenceError::Provider(err.to_string()));
        }
        Ok(())
    }

    /// Delete a local model tag (`/api/delete`).
    pub async fn delete_model(&self, name: &str) -> Result<(), InferenceError> {
        let url = format!("{}/api/delete", self.base_url);
        let body = json!({ "model": name });
        let v = self
            .guard
            .delete_json(&url, &body, "inference:ollama:delete")
            .await?;
        if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
            return Err(InferenceError::Provider(err.to_string()));
        }
        Ok(())
    }

    /// Release a named runner without deleting its installed weights. Wait for
    /// observed removal so a following benchmark cannot reuse its allocation.
    pub async fn unload_model(&self, model: &str) -> Result<(), InferenceError> {
        let _admission = self.admission.lock().await;
        self.unload_model_inner(model).await
    }

    async fn unload_model_inner(&self, model: &str) -> Result<(), InferenceError> {
        let operation = async {
            *self.last_prewarm.lock().await = None;
            self.invalidate_ps_cache().await;
            let ps = self.get_ps_cached().await?;
            let is_present = |ps: &Value| {
                ps["models"].as_array().unwrap().iter().any(|m| {
                    m.get("name")
                        .or_else(|| m.get("model"))
                        .and_then(Value::as_str)
                        .is_some_and(|name| ollama_model_matches(name, model))
                })
            };
            if !is_present(&ps) {
                return Ok(());
            }
            let url = format!("{}/api/generate", self.base_url);
            let response = self
                .guard
                .post_json(
                    &url,
                    &json!({"model": model, "keep_alive": 0, "stream": false}),
                    "inference:ollama:unload",
                )
                .await?;
            if response.get("error").is_some()
                || response.get("done").and_then(Value::as_bool) != Some(true)
            {
                return Err(InferenceError::Provider(
                    "model unload was not acknowledged".into(),
                ));
            }
            loop {
                self.invalidate_ps_cache().await;
                let ps = self.get_ps_cached().await?;
                if !is_present(&ps) {
                    return Ok(());
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        };
        let result = tokio::time::timeout(std::time::Duration::from_secs(15), operation).await;
        result.map_err(|_| {
            InferenceError::Provider(
                "model unload did not release residency within 15 seconds".into(),
            )
        })?
    }

    /// Non-streaming chat for capacity microbench (`/api/chat`, stream=false).
    pub async fn chat_once(
        &self,
        model: &str,
        prompt: &str,
        num_predict: u32,
        num_ctx: Option<u32>,
    ) -> Result<OllamaChatOnce, InferenceError> {
        use std::time::Instant;

        let _admission = self.admission.lock().await;
        let t0 = Instant::now();
        let url = format!("{}/api/chat", self.base_url);
        let mut options = json!({ "temperature": 0.0, "num_predict": num_predict });
        if let Some(n) = num_ctx {
            options["num_ctx"] = json!(n);
        }
        // Capacity probes retain their exact recipe. Reject offload before
        // timing a prompt rather than treating CPU execution as a viable fit.
        let allocation = self
            .guard
            .post_json(
                &format!("{}/api/generate", self.base_url),
                &json!({"model":model,"prompt":"","stream":false,"options":options}),
                "inference:ollama:bench-allocation",
            )
            .await?;
        if allocation.get("error").is_some() || allocation["done"] != true {
            return Err(InferenceError::Provider(
                "benchmark allocation failed".into(),
            ));
        }
        self.invalidate_ps_cache().await;
        self.verify_or_release_allocation(model).await?;
        let body = json!({
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "stream": false,
            "options": options,
        });

        let v = self
            .guard
            .post_json(&url, &body, "inference:ollama:bench")
            .await?;
        let wall_s = t0.elapsed().as_secs_f64();
        if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
            return Err(InferenceError::Provider(err.to_string()));
        }

        self.invalidate_ps_cache().await;
        self.check_vram_spill(model).await?;
        let prompt_tokens = v
            .get("prompt_eval_count")
            .and_then(|x| x.as_u64())
            .unwrap_or(0) as u32;
        let eval_tokens = v.get("eval_count").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
        let pf_ns = v
            .get("prompt_eval_duration")
            .and_then(|x| x.as_f64())
            .unwrap_or(0.0);
        let ev_ns = v
            .get("eval_duration")
            .and_then(|x| x.as_f64())
            .unwrap_or(0.0);
        Ok(OllamaChatOnce {
            wall_s,
            prompt_tokens,
            eval_tokens,
            prefill_tps: bench_rate(prompt_tokens, pf_ns),
            decode_tps: bench_rate(eval_tokens, ev_ns),
        })
    }

    /// List locally installed models.
    pub async fn list_models(&self) -> Result<Vec<String>, InferenceError> {
        Ok(self
            .list_model_tags()
            .await?
            .into_iter()
            .map(|t| t.name)
            .collect())
    }

    /// Installed models with optional digests from `/api/tags`.
    pub async fn list_model_tags(&self) -> Result<Vec<OllamaModelTag>, InferenceError> {
        let url = format!("{}/api/tags", self.base_url);
        let v = self.guard.get_json(&url, "inference:ollama:tags").await?;
        let tags = v
            .get("models")
            .and_then(|m| m.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| {
                        let name = m.get("name")?.as_str()?.to_string();
                        let digest = m
                            .get("digest")
                            .and_then(|d| d.as_str())
                            .map(normalize_ollama_digest);
                        Some(OllamaModelTag { name, digest })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(tags)
    }

    /// Build fabric model inventory entries from Ollama tags + show metadata.
    pub async fn model_inventory_capabilities(
        &self,
        resident: &[String],
    ) -> Vec<tetonic_fabric_protocol::ModelCapability> {
        use std::collections::HashSet;
        use tetonic_fabric_protocol::{ModelCapability, ModelLoadState};

        let resident: HashSet<_> = resident.iter().collect();
        let tags = self.list_model_tags().await.unwrap_or_default();
        use futures_util::{stream, FutureExt, StreamExt};
        // Metadata reads are independent. Bound concurrency and retain tag order;
        // this never loads models or changes inference sampling/placement policy.
        stream::iter(tags.into_iter().map(|tag| {
            let resident = &resident;
            async move {
                let info = self.model_info(&tag.name).await.unwrap_or_default();
                let digest = tag.digest.clone().or(info.digest);
                let warm = resident.contains(&tag.name);
                ModelCapability {
                    local_name: tag.name,
                    model_digest: digest,
                    quantization: info.quantization,
                    parameter_size_b: info.param_b,
                    max_context_length: 0,
                    tool_call_support: info.capabilities.iter().any(|c| c == "tools"),
                    structured_output_support: false,
                    estimated_vram_bytes: None,
                    load_state: if warm {
                        ModelLoadState::Warm
                    } else {
                        ModelLoadState::Cold
                    },
                }
            }
            .boxed()
        }))
        .buffered(4)
        .collect()
        .await
    }

    /// Models currently loaded in the runtime (`/api/ps`), if reachable.
    pub async fn running_models(&self) -> Result<Vec<String>, InferenceError> {
        let v = match self.get_ps_cached().await {
            Ok(v) => v,
            Err(_) => return Ok(Vec::new()),
        };
        let names = v
            .get("models")
            .and_then(|m| m.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        Ok(names)
    }

    /// Optional VRAM hint from `LOKAI_VRAM_MB` (total MiB). Ollama does not expose
    /// GPU memory on all platforms; 0 means unknown.
    fn vram_total_mb_hint() -> u32 {
        std::env::var("LOKAI_VRAM_MB")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    }

    /// Build a single-node fabric snapshot for this loopback provider.
    pub async fn local_fabric_snapshot(&self) -> FabricSnapshot {
        // The inventory request itself is the reachability probe. Avoid fetching
        // the same /api/tags payload twice on every snapshot refresh.
        let inventory = self.list_models().await;
        let reachable = inventory.is_ok();
        let installed = inventory.unwrap_or_default();
        let resident = if reachable {
            self.running_models().await.unwrap_or_default()
        } else {
            Vec::new()
        };
        let vram_total = Self::vram_total_mb_hint();
        let node = NodeInfo {
            id: LOCAL_NODE_ID.into(),
            label: "Local Ollama".into(),
            vram_total_mb: vram_total,
            vram_free_mb: if reachable && vram_total > 0 {
                vram_total
            } else {
                0
            },
            resident_models: if resident.is_empty() {
                installed.clone()
            } else {
                resident
            },
            queue_depth: 0,
            healthy: reachable,
            models_verified: reachable,
            capacity: None,
            legacy_v1_chat_only: false,
            negotiated_protocol_version: None,
        };
        FabricSnapshot {
            effective_concurrency: u32::from(reachable),
            nodes: vec![node],
            generated_at: Utc::now(),
        }
    }
}

#[async_trait]
impl InferenceProvider for OllamaProvider {
    async fn fabric_snapshot(&self) -> FabricSnapshot {
        self.local_fabric_snapshot().await
    }

    async fn prewarm(&self, model: &str, keep_alive: Option<&str>) -> Result<(), InferenceError> {
        self.prewarm(model, keep_alive).await
    }

    async fn chat(
        &self,
        req: ChatRequest,
        on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        require_outbound_scan(&req)?;
        let url = format!("{}/api/chat", self.base_url);
        let tools_slice = if req.tools.is_empty() {
            None
        } else {
            Some(req.tools.as_slice())
        };
        let mut body = OllamaChatRequestBody {
            think: self.thinking,
            model: &req.model,
            messages: &req.messages,
            stream: true,
            options: OllamaChatOptions {
                num_gpu: None,
                num_batch: None,
                temperature: req.temperature,
                num_ctx: req.num_ctx,
                draft_model: req.draft_model.clone(),
                draft_count: req.draft_count,
            },
            keep_alive: req.keep_alive.as_deref(),
            tools: tools_slice,
            format: req.response_format.as_ref(),
        };

        use tetonic_telemetry::{PerfStage, StageTimer};
        // From request dispatch to visible answer, including load/prefill and
        // hidden thinking. A first JSON/thinking chunk is not a visible answer.
        let mut first_content_timer =
            Some(StageTimer::start_visible(PerfStage::InferenceFirstContent));
        let mut admission = self.admission.lock().await;
        self.admit_allocation(&mut admission, &req.model, &mut body.options)
            .await?;

        let header_timer = StageTimer::start_visible(PerfStage::InferenceHeaders);
        let stream_result = self
            .guard
            .post_ndjson_stream(&url, &body, "inference:ollama:chat")
            .await;
        header_timer.finish(stream_result.is_ok());
        let stream = stream_result?;
        futures_util::pin_mut!(stream);
        let stream_timer = StageTimer::start_visible(PerfStage::InferenceStream);
        let mut first_chunk_timer = Some(StageTimer::start_visible(PerfStage::InferenceFirstChunk));

        let mut content = String::new();
        let mut role = "assistant".to_string();
        let mut tool_calls: Option<Vec<ToolCall>> = None;
        let mut usage = GenUsage::default();
        let mut checked_vram = false;
        let mut received_done = false;

        // CMP-01: idle deadline prevents indefinite blocking on stalled servers.
        // 5 minutes is generous enough for local model loading (Ollama can take
        // 30-60s to load a large model before the first token).
        const STREAM_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

        use futures_util::StreamExt;
        while let Some(item) = tokio::time::timeout(STREAM_IDLE_TIMEOUT, stream.next())
            .await
            .map_err(|_| InferenceError::StreamTimeout {
                phase: "idle",
                elapsed_secs: STREAM_IDLE_TIMEOUT.as_secs(),
            })?
        {
            let chunk = item?;
            if let Some(timer) = first_chunk_timer.take() {
                timer.finish(true);
            }
            if !checked_vram {
                self.invalidate_ps_cache().await;
                self.check_vram_spill(&req.model).await?;
                checked_vram = true;
            }

            if let Some(err) = chunk.get("error").and_then(|e| e.as_str()) {
                return Err(InferenceError::Provider(err.to_string()));
            }
            if let Some(msg) = chunk.get("message") {
                if let Some(r) = msg.get("role").and_then(|r| r.as_str()) {
                    role = r.to_string();
                }
                if let Some(c) = msg.get("content").and_then(|c| c.as_str()) {
                    if !c.is_empty() {
                        if let Some(timer) = first_content_timer.take() {
                            timer.finish(true);
                        }
                        content.push_str(c);
                        on_token(c);
                    }
                }
                if let Some(tc) = msg.get("tool_calls") {
                    if !tc.is_null() {
                        let parsed: Vec<ToolCall> = serde_json::from_value(tc.clone())
                            .map_err(|e| InferenceError::Decode(e.to_string()))?;
                        tool_calls.get_or_insert_with(Vec::new).extend(parsed);
                    }
                }
            }
            if chunk.get("done").and_then(|d| d.as_bool()).unwrap_or(false) {
                received_done = true;
                // Final chunk carries the generation accounting. Durations are
                // nanoseconds; convert to ms here so callers don't have to.
                usage.prompt_tokens = chunk.get("prompt_eval_count").and_then(|v| v.as_u64());
                usage.eval_tokens = chunk.get("eval_count").and_then(|v| v.as_u64());
                usage.prompt_eval_ms = chunk
                    .get("prompt_eval_duration")
                    .and_then(|v| v.as_f64())
                    .map(|ns| ns / 1e6);
                usage.eval_ms = chunk
                    .get("eval_duration")
                    .and_then(|v| v.as_f64())
                    .map(|ns| ns / 1e6);
                // Numeric, payload-free stages consumed by the general benchmark
                // collector. Backend total overlaps its load/prefill/decode children.
                for (field, stage) in [
                    ("load_duration", "inference_load"),
                    ("prompt_eval_duration", "inference_prefill"),
                    ("eval_duration", "inference_decode"),
                    ("total_duration", "inference_backend_total"),
                ] {
                    if let Some(ns) = chunk.get(field).and_then(Value::as_u64) {
                        tracing::debug!(target: "lokai_performance", stage,
                            outcome = "succeeded", duration_ms = ns as f64 / 1e6,
                            "backend performance stage");
                    }
                }
                tracing::debug!(
                    model = %req.model,
                    load_ms = ?chunk.get("load_duration").and_then(|v| v.as_f64()).map(|ns| ns / 1e6),
                    prompt_eval_ms = ?usage.prompt_eval_ms,
                    eval_ms = ?usage.eval_ms,
                    "local inference timing"
                );
                break;
            }
        }

        // CMP-02: reject EOF without done marker — a truncated/interrupted
        // response must not be silently accepted as a successful completion.
        if !received_done {
            return Err(InferenceError::IncompleteStream {
                tokens_received: !content.is_empty(),
            });
        }
        stream_timer.finish(true);

        Ok(ChatResponse {
            message: {
                let mut message = Message {
                    role,
                    content,
                    tool_calls,
                    tool_name: None,
                    tool_call_id: None,
                    token_cache: AtomicI64::new(-1),
                };
                recover_message_tool_calls(&mut message);
                message
            },
            usage,
            provenance: InferenceProvenance {
                provider_kind: "local".into(),
                worker_id: None,
                model: req.model.clone(),
                job_id: None,
                attempt_id: None,
                prompt_redacted: false,
                ..Default::default()
            },
        })
    }
}

/// When the model emits tool calls inside JSON `content` instead of native
/// `tool_calls` (common with qwen3.5 + `format`), extract and attach them.
pub fn recover_message_tool_calls(msg: &mut Message) {
    if msg.tool_calls.as_ref().is_some_and(|t| !t.is_empty()) {
        return;
    }
    let Some(calls) = parse_tool_calls_from_content(&msg.content) else {
        return;
    };
    msg.tool_calls = Some(calls);
    if looks_like_tool_call_json(&msg.content) {
        msg.content.clear();
    }
}

fn looks_like_tool_call_json(content: &str) -> bool {
    let t = content.trim();
    t.starts_with('{') && t.contains("tool_calls")
}

fn parse_tool_calls_from_content(content: &str) -> Option<Vec<ToolCall>> {
    let trimmed = content.trim();
    if !trimmed.starts_with('{') {
        return None;
    }
    let v: Value = serde_json::from_str(trimmed).ok()?;
    let arr = v
        .get("message")
        .and_then(|m| m.get("tool_calls"))
        .or_else(|| v.get("tool_calls"))?;
    let mut calls: Vec<ToolCall> = serde_json::from_value(arr.clone()).ok()?;
    for tc in &mut calls {
        normalize_function_call(&mut tc.function);
    }
    if calls.is_empty() {
        None
    } else {
        Some(calls)
    }
}

fn normalize_function_call(f: &mut FunctionCall) {
    if f.name.is_empty() {
        if let Some(n) = f.arguments.get("name").and_then(|v| v.as_str()) {
            f.name = n.to_string();
        }
    }
    if let Some(path) = f.arguments.get("path").and_then(|v| v.as_str()) {
        if f.name == "read_file" || f.name.is_empty() {
            f.name = "read_file".into();
            f.arguments = json!({ "path": path });
        }
    }
}

/// Parse a minimal Modelfile into Ollama `/api/create` `from` + `parameters`.
fn parse_modelfile_for_create(modelfile: &str) -> Option<(String, Value)> {
    let mut from = None;
    let mut params = serde_json::Map::new();
    for line in modelfile.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("FROM ") {
            from = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("PARAMETER ") {
            let mut parts = rest.split_whitespace();
            let key = parts.next()?;
            let val = parts.next()?;
            let value = val
                .parse::<i64>()
                .map(|n| json!(n))
                .unwrap_or_else(|_| json!(val));
            params.insert(key.to_string(), value);
        }
    }
    from.map(|f| (f, json!(params)))
}

#[cfg(test)]
mod create_model_tests {
    use super::*;

    #[test]
    fn parse_modelfile_extracts_from_and_parameters() {
        let mf = "FROM qwen3.6:latest\nPARAMETER num_ctx 4096\nPARAMETER num_gpu 999";
        let (from, params) = parse_modelfile_for_create(mf).unwrap();
        assert_eq!(from, "qwen3.6:latest");
        assert_eq!(params["num_ctx"], 4096);
        assert_eq!(params["num_gpu"], 999);
    }

    #[test]
    fn parse_modelfile_requires_from() {
        assert!(parse_modelfile_for_create("PARAMETER num_ctx 4096").is_none());
    }
}

#[cfg(test)]
mod tool_recovery_tests {
    use super::*;

    #[test]
    fn recovers_tool_calls_from_wrapped_json_content() {
        let mut msg = Message::assistant(
            r#"{"message":{"tool_calls":[{"function":{"name":"list_dir","arguments":{"path":"."}}}]}}"#,
        );
        recover_message_tool_calls(&mut msg);
        let calls = msg.tool_calls.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "list_dir");
    }

    #[test]
    fn recovers_malformed_read_file_with_name_in_arguments() {
        let mut msg = Message::assistant(
            r#"{"message":{"tool_calls":[{"function":{"arguments":{"name":"read_file","path":"src/lib.rs"},"name":"read_file"}}]}}"#,
        );
        recover_message_tool_calls(&mut msg);
        let calls = msg.tool_calls.unwrap();
        assert_eq!(calls[0].function.name, "read_file");
        assert_eq!(calls[0].function.arguments["path"], "src/lib.rs");
    }
}

#[cfg(test)]
mod vram_spill_tests {
    use super::*;

    #[test]
    fn rejects_even_a_small_cpu_offload_tail() {
        // Observed on qwen3.6:latest: ~96% VRAM residency with a small RAM tail.
        let size = 23_693_099_002u64;
        let size_vram = 22_838_394_221u64;
        assert!(gpu_residency_pct(size, size_vram).unwrap() > 90.0);
        assert!(vram_spill_below_threshold(size, size_vram).is_some());
    }

    #[test]
    fn blocks_severe_spill_below_gate_threshold() {
        let size = 10_000u64;
        let size_vram = 2_000u64;
        assert_eq!(vram_spill_below_threshold(size, size_vram), Some(20.0));
    }
}

#[cfg(test)]
mod request_serialization_tests {
    use super::*;

    #[test]
    fn parity_between_typed_body_and_legacy_json_ast() {
        let messages = vec![
            Message::system("You are a helpful assistant."),
            Message::user("Please list files in src/"),
        ];
        let tools = vec![ToolSchema::function(
            "list_dir",
            "List directory contents",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                }
            }),
        )];

        let req = ChatRequest {
            model: "qwen3.6:latest".into(),
            model_digest: None,
            messages: messages.clone(),
            tools: tools.clone(),
            temperature: 0.2,
            num_ctx: Some(8192),
            keep_alive: Some("30m".into()),
            fabric: None,
            response_format: None,
            outbound_scan: OutboundScan::default(),
            ..Default::default()
        };

        // Legacy dynamic JSON assembly
        let mut legacy_body = json!({
            "model": req.model,
            "messages": req.messages,
            "stream": true,
            "options": { "temperature": req.temperature }
        });
        if let Some(n) = req.num_ctx {
            legacy_body["options"]["num_ctx"] = json!(n);
        }
        if let Some(k) = &req.keep_alive {
            legacy_body["keep_alive"] = json!(k);
        }
        if !req.tools.is_empty() {
            legacy_body["tools"] = serde_json::to_value(&req.tools).unwrap();
        }

        // New typed direct serialization (OPT-102)
        let tools_slice = if req.tools.is_empty() {
            None
        } else {
            Some(req.tools.as_slice())
        };
        let mut typed_body = OllamaChatRequestBody {
            think: None,
            model: &req.model,
            messages: &req.messages,
            stream: true,
            options: OllamaChatOptions {
                num_gpu: None,
                num_batch: None,
                temperature: req.temperature,
                num_ctx: req.num_ctx,
                draft_model: req.draft_model.clone(),
                draft_count: req.draft_count,
            },
            keep_alive: req.keep_alive.as_deref().or(Some("30m")),
            tools: tools_slice,
            format: req.response_format.as_ref(),
        };

        let typed_json_bytes = serde_json::to_vec(&typed_body).unwrap();
        let parsed_from_typed: Value = serde_json::from_slice(&typed_json_bytes).unwrap();

        assert_eq!(parsed_from_typed["model"], legacy_body["model"]);
        assert_eq!(parsed_from_typed["messages"], legacy_body["messages"]);
        assert_eq!(parsed_from_typed["stream"], legacy_body["stream"]);
        assert_eq!(parsed_from_typed["keep_alive"], legacy_body["keep_alive"]);
        assert_eq!(parsed_from_typed["tools"], legacy_body["tools"]);
        assert_eq!(
            parsed_from_typed["options"]["num_ctx"],
            legacy_body["options"]["num_ctx"]
        );
        let typed_temp = parsed_from_typed["options"]["temperature"]
            .as_f64()
            .unwrap();
        assert!((typed_temp - 0.2).abs() < 1e-5);
        assert!(parsed_from_typed.get("think").is_none());
        typed_body.think = Some(false);
        assert_eq!(serde_json::to_value(&typed_body).unwrap()["think"], false);
    }

    #[test]
    fn test_speculative_decoding_options_serialization() {
        let opts = OllamaChatOptions {
            num_gpu: None,
            num_batch: None,
            temperature: 0.2,
            num_ctx: Some(8192),
            draft_model: Some("qwen2.5-coder:1.5b".to_string()),
            draft_count: Some(5),
        };
        let json = serde_json::to_value(&opts).unwrap();
        assert_eq!(json["draft_model"], "qwen2.5-coder:1.5b");
        assert_eq!(json["draft_count"], 5);

        let opts_empty = OllamaChatOptions {
            num_gpu: None,
            num_batch: None,
            temperature: 0.2,
            num_ctx: None,
            draft_model: None,
            draft_count: None,
        };
        let json_empty = serde_json::to_value(&opts_empty).unwrap();
        assert!(json_empty.get("draft_model").is_none());
        assert!(json_empty.get("draft_count").is_none());
    }
}

#[cfg(test)]
mod outbound_scan_stamp_tests {
    use super::*;
    use std::sync::Arc;

    fn unscanned_req() -> ChatRequest {
        ChatRequest {
            model: "qwen:7b".into(),
            messages: vec![Message::user("hi")],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn ollama_chat_refuses_unstamped_request() {
        let guard = Arc::new(EgressGuard::new());
        let provider = OllamaProvider::new("http://127.0.0.1:9", guard);
        let mut sink = |_t: &str| {};
        let err = provider.chat(unscanned_req(), &mut sink).await.unwrap_err();
        assert!(
            matches!(err, InferenceError::SecretScanFailed { ref reason } if reason.contains("stamp")),
            "got {err}"
        );
    }

    #[tokio::test]
    async fn pooled_chat_refuses_unstamped_request() {
        let local = Arc::new(OllamaProvider::new(
            "http://127.0.0.1:9",
            Arc::new(EgressGuard::new()),
        ));
        let pooled = crate::PooledProvider::new(local, vec![]);
        let mut sink = |_t: &str| {};
        let err = pooled.chat(unscanned_req(), &mut sink).await.unwrap_err();
        assert!(
            matches!(err, InferenceError::SecretScanFailed { ref reason } if reason.contains("stamp")),
            "got {err}"
        );
    }
}

#[cfg(test)]
mod warmup_tests;

#[cfg(test)]
mod performance_tests;

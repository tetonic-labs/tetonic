//! Brain abstraction: the cognitive processing unit for an agent.
//!
//! A `Brain` is the complete reasoning system an agent delegates to.
//! The agent has no knowledge of what is inside the brain — one model,
//! many models, a hierarchy, an ensemble — all are valid implementations.
//! This is the stable contract; implementations live above in the assembly layer.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// The complete cognitive processing unit for an agent.
///
/// Implementations may compose any number of inference providers internally.
/// The `Agent` only ever sees this interface.
#[async_trait]
pub trait Brain: Send + Sync {
    /// Process a turn request and produce a response stream or completion.
    ///
    /// The brain decides internally how many models to invoke, in what order,
    /// and with what routing logic. It returns a single unified response.
    async fn complete(
        &self,
        req: BrainRequest,
        on_token: &mut BrainTokenSink<'_>,
    ) -> Result<BrainResponse, BrainError>;

    /// A short human-readable description of this brain's architecture.
    /// Used in logs, the chronicle, and experiment tracking.
    /// Examples: `"single:claude-sonnet-4-5"`, `"dual:jev+claude"`, `"ensemble:3x-local"`
    fn describe(&self) -> &str;

    /// Estimated token cost for the last `complete` call.
    /// Used by the Mantle broker for scheduling and cost accounting.
    fn last_cost(&self) -> BrainCost;
}

/// A streaming token callback — identical contract to `InferenceProvider`'s
/// `TokenSink` so the brain layer can forward streams without buffering.
pub type BrainTokenSink<'a> = dyn FnMut(&str) + Send + 'a;

/// Everything the brain needs to produce a response for one agent turn.
///
/// Mirrors `ChatRequest` at the inference layer but carries no provider-specific
/// fields. The brain translates this into however many provider calls it needs.
#[derive(Debug, Clone)]
pub struct BrainRequest {
    /// The conversation history and system prompt.
    pub messages: Vec<BrainMessage>,
    /// Tool schemas available for this turn.
    pub tools: Vec<serde_json::Value>,
    /// Maximum tokens to generate.
    pub max_tokens: Option<u32>,
    /// Caller-assigned label for tracing (session_id, turn_id, etc.)
    pub trace_label: String,
}

/// A single message in the brain's conversation view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrainMessage {
    pub role: BrainRole,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BrainRole {
    System,
    User,
    Assistant,
    Tool,
}

/// The brain's unified response after completing a turn.
#[derive(Debug, Clone)]
pub struct BrainResponse {
    /// The final text content of the response.
    pub content: String,
    /// Any tool calls the brain decided to make.
    pub tool_calls: Option<serde_json::Value>,
    /// Which internal pathway(s) produced this response.
    pub pathway: BrainPathway,
    /// Stop reason from the underlying model(s).
    pub finish_reason: BrainFinishReason,
    /// Token usage across all internal model calls.
    pub cost: BrainCost,
}

/// Which cognitive pathway(s) the brain engaged to produce this response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrainPathway {
    /// A single model handled the full turn.
    Single { model: String },
    /// A fast reflexive model handled the turn.
    Reflexive { model: String },
    /// A slow deliberative model was engaged (possibly after a reflexive pass).
    Deliberative { model: String },
    /// Multiple models contributed; the orchestrator synthesized the final output.
    Hierarchical { orchestrator: String, specialists: Vec<String> },
    /// Multiple models voted; the result was chosen by the configured strategy.
    Ensemble { members: Vec<String>, strategy: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrainFinishReason {
    Stop,
    ToolUse,
    Length,
    Error,
}

/// Aggregate token cost across all internal model calls in one brain turn.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BrainCost {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Number of distinct model calls made inside this brain turn.
    pub model_calls: u32,
}

impl BrainCost {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }

    pub fn add(&mut self, other: &BrainCost) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.model_calls += other.model_calls;
    }
}

/// Errors the brain may surface to the agent loop.
#[derive(Debug, thiserror::Error)]
pub enum BrainError {
    #[error("all inference pathways failed: {0}")]
    AllPathwaysFailed(String),
    #[error("brain configuration invalid: {0}")]
    Configuration(String),
    #[error("inference error in pathway '{pathway}': {detail}")]
    Inference { pathway: String, detail: String },
    #[error("brain turn cancelled")]
    Cancelled,
}

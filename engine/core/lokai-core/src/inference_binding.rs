use crate::Tokenizer;
use lokai_inference::InferenceProvider;
use std::sync::Arc;

/// A complete inference dependency replacement supplied by the owning runtime.
/// It carries no credentials, endpoint selection, or routing policy.
pub struct AgentInferenceBinding {
    pub provider: Arc<dyn InferenceProvider>,
    pub model: String,
    pub num_ctx: usize,
    pub tokenizer: Box<dyn Tokenizer>,
}

#[derive(Debug, thiserror::Error)]
#[error("inference binding requires a nonempty model and context larger than the response reserve")]
pub struct InvalidInferenceBinding;

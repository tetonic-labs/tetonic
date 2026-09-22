//! Opaque inference boundary for the capacity plane — no egress dependency.

use std::sync::Arc;

use async_trait::async_trait;
use tetonic_inference::{InferenceError, ModelInfo, OllamaChatOnce, OllamaProvider};

use crate::microbench::ChatOnceResult;
use crate::probe::parse_ps_response_for_model;
use crate::profile::ObservedPlacement;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("inference: {0}")]
    Inference(#[from] InferenceError),
    #[error("ollama: {0}")]
    Ollama(String),
}

/// Capacity-plane view of the local inference runtime. Construct in lokaid/CLI
/// with [`OllamaInferenceClient::new`] (wraps [`OllamaProvider`] + egress guard).
#[async_trait]
pub trait InferenceClient: Send + Sync {
    fn base_url(&self) -> &str;
    async fn reachable(&self) -> bool;
    async fn version(&self) -> Option<String>;
    async fn list_models(&self) -> Result<Vec<String>, ClientError>;
    async fn model_capabilities(&self, model: &str) -> Result<Vec<String>, ClientError>;
    async fn model_info(&self, model: &str) -> Result<ModelInfo, ClientError>;
    async fn create_model(&self, name: &str, modelfile: &str) -> Result<(), ClientError>;
    async fn delete_model(&self, name: &str) -> Result<(), ClientError>;
    async fn unload_model(&self, name: &str) -> Result<(), ClientError>;
    async fn chat_once(
        &self,
        model: &str,
        prompt: &str,
        num_predict: u32,
        num_ctx: Option<u32>,
    ) -> Result<ChatOnceResult, ClientError>;
    async fn fetch_observed_placement(&self, model: &str) -> Option<ObservedPlacement>;
}

/// Production adapter: [`OllamaProvider`] behind [`InferenceClient`].
pub struct OllamaInferenceClient {
    provider: Arc<OllamaProvider>,
    base_url: String,
}

impl OllamaInferenceClient {
    pub fn new(base_url: impl Into<String>, provider: Arc<OllamaProvider>) -> Self {
        Self {
            provider,
            base_url: base_url.into(),
        }
    }
}

#[async_trait]
impl InferenceClient for OllamaInferenceClient {
    fn base_url(&self) -> &str {
        &self.base_url
    }

    async fn reachable(&self) -> bool {
        self.provider.reachable().await
    }

    async fn version(&self) -> Option<String> {
        self.provider.version().await
    }

    async fn list_models(&self) -> Result<Vec<String>, ClientError> {
        Ok(self.provider.list_models().await?)
    }

    async fn model_capabilities(&self, model: &str) -> Result<Vec<String>, ClientError> {
        Ok(self.provider.model_capabilities(model).await?)
    }

    async fn model_info(&self, model: &str) -> Result<ModelInfo, ClientError> {
        Ok(self.provider.model_info(model).await?)
    }

    async fn create_model(&self, name: &str, modelfile: &str) -> Result<(), ClientError> {
        Ok(self.provider.create_model(name, modelfile).await?)
    }

    async fn delete_model(&self, name: &str) -> Result<(), ClientError> {
        Ok(self.provider.delete_model(name).await?)
    }

    async fn unload_model(&self, name: &str) -> Result<(), ClientError> {
        Ok(self.provider.unload_model(name).await?)
    }

    async fn chat_once(
        &self,
        model: &str,
        prompt: &str,
        num_predict: u32,
        num_ctx: Option<u32>,
    ) -> Result<ChatOnceResult, ClientError> {
        let r: OllamaChatOnce = self
            .provider
            .chat_once(model, prompt, num_predict, num_ctx)
            .await?;
        Ok(ChatOnceResult {
            wall_s: r.wall_s,
            prompt_tokens: r.prompt_tokens,
            eval_tokens: r.eval_tokens,
            prefill_tps: r.prefill_tps,
            decode_tps: r.decode_tps,
        })
    }

    async fn fetch_observed_placement(&self, model: &str) -> Option<ObservedPlacement> {
        parse_ps_response_for_model(&self.provider.ps_json().await, model)
    }
}

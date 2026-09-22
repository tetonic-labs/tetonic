//! Opt-in, buffered Chat Completions and Messages adapter. No automatic routing or fallback.
//! Backend options and authentication remain outside the portable agent loop.
pub mod anthropic;
pub mod openai;
pub mod registry;
#[cfg(test)]
mod tests;
pub mod wire;

use std::sync::Arc;

use async_trait::async_trait;
use lokai_domain::secrets::{ScanContext, SecretScanner};
use lokai_egress::{EgressGuard, HostedCredential};
use lokai_policy::HostedInferencePolicy;
use serde_json::Value;

use crate::{ChatRequest, ChatResponse, DataClass, InferenceError, InferenceProvider, TokenSink};

/// Resolve a credential from the host's secret store at request time. Implementors
/// must not include credential values in errors. No environment/global lookup here.
#[async_trait]
pub trait HostedCredentialSource: Send + Sync {
    async fn credential(&self) -> Result<HostedCredential, InferenceError>;
}

pub struct AnthropicEnvCredentialSource {
    custom_key: Option<String>,
}

impl AnthropicEnvCredentialSource {
    pub fn new(custom_key: Option<String>) -> Self {
        Self { custom_key }
    }
}

#[async_trait]
impl HostedCredentialSource for AnthropicEnvCredentialSource {
    async fn credential(&self) -> Result<HostedCredential, InferenceError> {
        let key = self
            .custom_key
            .clone()
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
            .or_else(|| std::env::var("CLAUDE_API_KEY").ok())
            .ok_or_else(|| error("missing ANTHROPIC_API_KEY or CLAUDE_API_KEY"))?;
        HostedCredential::anthropic(&key).map_err(|e| error(&e.to_string()))
    }
}

pub struct BearerEnvCredentialSource {
    env_var: String,
    custom_key: Option<String>,
}

impl BearerEnvCredentialSource {
    pub fn new(env_var: impl Into<String>, custom_key: Option<String>) -> Self {
        Self {
            env_var: env_var.into(),
            custom_key,
        }
    }
}

#[async_trait]
impl HostedCredentialSource for BearerEnvCredentialSource {
    async fn credential(&self) -> Result<HostedCredential, InferenceError> {
        let key = self
            .custom_key
            .clone()
            .or_else(|| std::env::var(&self.env_var).ok())
            .ok_or_else(|| error(&format!("missing {} environment variable", self.env_var)))?;
        HostedCredential::bearer(&key).map_err(|e| error(&e.to_string()))
    }
}

pub struct StaticCredentialSource(pub HostedCredential);

#[async_trait]
impl HostedCredentialSource for StaticCredentialSource {
    async fn credential(&self) -> Result<HostedCredential, InferenceError> {
        Ok(self.0.clone())
    }
}

/// Transport seam; production uses EgressHostedTransport. Allows protocol tests
/// without opening sockets or requiring an account.
#[async_trait]
pub trait HostedTransport: Send + Sync {
    async fn complete(&self, body: Value) -> Result<Value, InferenceError>;
}

pub struct EgressHostedTransport {
    guard: Arc<EgressGuard>,
    endpoint: String,
    credentials: Arc<dyn HostedCredentialSource>,
}

impl EgressHostedTransport {
    /// Does not enroll the endpoint. The operator must separately grant the exact
    /// URL via EgressGuard::allow_hosted_endpoint.
    pub fn new(
        guard: Arc<EgressGuard>,
        endpoint: String,
        credentials: Arc<dyn HostedCredentialSource>,
    ) -> Self {
        Self {
            guard,
            endpoint,
            credentials,
        }
    }
}

#[async_trait]
impl HostedTransport for EgressHostedTransport {
    async fn complete(&self, body: Value) -> Result<Value, InferenceError> {
        let credential = self
            .credentials
            .credential()
            .await
            .map_err(|_| error("hosted credential unavailable"))?;
        self.guard
            .post_hosted_json(&self.endpoint, &body, &credential)
            .await
            .map_err(|e| error(&format!("hosted transport error: {e}")))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HostedWireProtocol {
    #[default]
    OpenAiChatCompletions,
    AnthropicMessages,
}

#[derive(Debug, Clone, Copy)]
pub enum OutputLimitField {
    MaxTokens,
    MaxCompletionTokens,
}

/// Explicit model/endpoint profile; contains no secrets. Capabilities are operator
/// declarations, not inferred from the model's name.
#[derive(Debug, Clone)]
pub struct HostedModelConfig {
    pub protocol: HostedWireProtocol,
    pub model: String,
    pub allowed_models: Vec<String>,
    pub max_output_tokens: u32,
    pub output_limit_field: OutputLimitField,
    pub supports_tools: bool,
    pub supports_json_schema: bool,
    pub send_temperature: bool,
}

impl HostedModelConfig {
    pub fn is_model_allowed(&self, model: &str) -> bool {
        if self.model == model {
            return true;
        }
        self.allowed_models.iter().any(|m| m == model)
    }

    pub fn openai(model: impl Into<String>, max_output_tokens: u32) -> Self {
        let m = model.into();
        Self {
            protocol: HostedWireProtocol::OpenAiChatCompletions,
            model: m.clone(),
            allowed_models: vec![m],
            max_output_tokens,
            output_limit_field: OutputLimitField::MaxTokens,
            supports_tools: true,
            supports_json_schema: true,
            send_temperature: true,
        }
    }

    pub fn anthropic(model: impl Into<String>, max_output_tokens: u32) -> Self {
        let m = model.into();
        Self {
            protocol: HostedWireProtocol::AnthropicMessages,
            model: m.clone(),
            allowed_models: vec![m],
            max_output_tokens,
            output_limit_field: OutputLimitField::MaxTokens,
            supports_tools: true,
            supports_json_schema: false,
            send_temperature: true,
        }
    }
}

pub struct HostedChatProvider {
    config: HostedModelConfig,
    policy: HostedInferencePolicy,
    scanner: Arc<dyn SecretScanner>,
    transport: Arc<dyn HostedTransport>,
}

impl HostedChatProvider {
    pub fn new(
        config: HostedModelConfig,
        policy: HostedInferencePolicy,
        scanner: Arc<dyn SecretScanner>,
        transport: Arc<dyn HostedTransport>,
    ) -> Result<Self, InferenceError> {
        if config.model.trim().is_empty() || config.max_output_tokens == 0 {
            return Err(error("hosted model and positive output limit required"));
        }
        Ok(Self {
            config,
            policy,
            scanner,
            transport,
        })
    }
}

#[async_trait]
impl InferenceProvider for HostedChatProvider {
    async fn chat(
        &self,
        req: ChatRequest,
        on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        if req.outbound_scan.blocks_remote() {
            return Err(InferenceError::RemoteSecretDenied {
                reason: "upstream secret finding".into(),
            });
        }
        let body = wire::request(&req, &self.config)?;
        // Scan exactly what will be sent, including tool schemas, function arguments,
        // and structured-output schemas. Never trust a stamp from a different payload.
        let raw =
            serde_json::to_string(&body).map_err(|_| error("hosted request encoding failed"))?;
        let class = crate::aggregate_chat_classification(&req)
            .map(|c| c.class)
            .unwrap_or(DataClass::RepositorySource);
        let class = lokai_policy::classify_text_content(&raw)
            .map(|c| class.max(c.class))
            .unwrap_or(class);
        if !self.policy.allows(class) {
            return Err(error("hosted disclosure policy denied request"));
        }
        match self
            .scanner
            .scan_and_redact_in_context(
                &raw,
                None,
                ScanContext {
                    session_id: req.fabric.as_ref().and_then(|f| f.session_id.as_deref()),
                    project_id: None,
                },
            )
            .await
        {
            Ok(None) => {}
            Ok(Some(_)) => {
                return Err(InferenceError::RemoteSecretDenied {
                    reason: "hosted payload secret finding".into(),
                })
            }
            Err(_) => {
                return Err(InferenceError::SecretScanFailed {
                    reason: "hosted payload scan failed".into(),
                })
            }
        }
        let value = self.transport.complete(body).await?;
        let mut response = wire::response(value, &self.config, &req.model)?;
        response.provenance.placement_class = Some(class);
        response.provenance.placement_decision = Some("hosted_explicit".into());
        response.provenance.attempt_id = req.fabric.as_ref().and_then(|f| f.attempt_id.clone());
        // Buffered compatibility: publish only a complete, validated response.
        if !response.message.content.is_empty() {
            on_token(&response.message.content);
        }
        Ok(response)
    }
}

fn error(message: &str) -> InferenceError {
    InferenceError::Provider(message.into())
}

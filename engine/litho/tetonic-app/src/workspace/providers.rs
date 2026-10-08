//! Provider selection and OS-backed credentials for the authenticated local owner.
use super::*;
use serde::Deserialize;
use std::sync::Arc;
use tetonic_domain::key_storage::{KeyStorage, SecretKeyRef};
use tetonic_inference::hosted::{
    EgressHostedTransport, HostedChatProvider, HostedCredentialSource, HostedModelConfig,
};
use tetonic_inference::InferenceError;

mod discovery;
pub use discovery::LocalModelCatalog;

#[derive(Serialize)]
pub struct LocalProvider {
    pub id: String,
    pub name: String,
    pub key_saved: bool,
}

// No Debug/Serialize: this payload must never reach logs or a response.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveProviderKey {
    pub provider: String,
    pub api_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoveProviderKey {
    pub provider: String,
}

fn provider_info(id: &str) -> Result<(&'static str, &'static str), AppError> {
    match id {
        "openai" => Ok(("OpenAI", "https://api.openai.com/v1/responses")),
        "anthropic" => Ok(("Anthropic", "https://api.anthropic.com/v1/messages")),
        "google" => Ok((
            "Google",
            "https://generativelanguage.googleapis.com/v1beta/models",
        )),
        _ => Err(AppError::InvalidRequest(
            "Choose OpenAI, Anthropic or Google.".into(),
        )),
    }
}

pub(crate) fn inference_endpoint(provider: &str, model: &str) -> Result<String, AppError> {
    let base = provider_info(provider)?.1;
    if provider == "google" {
        if model.is_empty()
            || model.len() > 256
            || !model
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            || model.starts_with('.')
        {
            return Err(AppError::InvalidRequest(
                "Enter a Gemini model ID without a URL or models/ prefix.".into(),
            ));
        }
        Ok(format!("{base}/{model}:generateContent"))
    } else {
        Ok(base.into())
    }
}

pub(crate) struct ProviderKeys {
    pub store: tetonic_memory::SharedStore,
    pub vault: Arc<dyn KeyStorage>,
}

impl ProviderKeys {
    async fn reference(&self, provider: &str) -> Result<Option<String>, AppError> {
        let provider = provider.to_owned();
        self.store
            .read(move |db| db.local_provider_key(&provider))
            .await
            .map_err(|_| key_error())?
            .map_err(|_| key_error())
    }

    pub(crate) async fn ready(&self, provider: &str) -> bool {
        let Ok(Some(reference)) = self.reference(provider).await else {
            return false;
        };
        let vault = self.vault.clone();
        tokio::task::spawn_blocking(move || vault.read(&SecretKeyRef(reference)).is_ok())
            .await
            .unwrap_or(false)
    }

    async fn save(&self, provider: &str, key: Vec<u8>) -> Result<(), AppError> {
        let old = self.reference(provider).await?;
        let vault = self.vault.clone();
        let secret = tetonic_domain::key_storage::SecretBytes::new(key);
        let reference = tokio::task::spawn_blocking(move || vault.create(secret.as_ref()))
            .await
            .map_err(|_| key_error())?
            .map_err(|_| key_error())?;
        let provider = provider.to_owned();
        let new_ref = reference.0.clone();
        let write = self
            .store
            .write(move |db| db.set_local_provider_key(&provider, Some(&new_ref)))
            .await;
        if !matches!(write, Ok(Ok(()))) {
            let vault = self.vault.clone();
            let _ = tokio::task::spawn_blocking(move || vault.delete(&reference)).await;
            return Err(key_error());
        }
        if let Some(old) = old {
            let vault = self.vault.clone();
            let _ = tokio::task::spawn_blocking(move || vault.delete(&SecretKeyRef(old))).await;
        }
        Ok(())
    }
}

fn key_error() -> AppError {
    AppError::InvalidRequest("Could not access the OS credential store. Unlock it and retry; keys are never saved as plaintext.".into())
}

struct StoredCredential {
    keys: Arc<ProviderKeys>,
    provider: String,
}
#[async_trait::async_trait]
impl HostedCredentialSource for StoredCredential {
    async fn credential(&self) -> Result<tetonic_egress::HostedCredential, InferenceError> {
        let unavailable = || {
            InferenceError::Provider(
                "Provider key unavailable. Save it again in agent setup.".into(),
            )
        };
        let reference = self
            .keys
            .reference(&self.provider)
            .await
            .map_err(|_| unavailable())?
            .ok_or_else(unavailable)?;
        let vault = self.keys.vault.clone();
        let secret = tokio::task::spawn_blocking(move || vault.read(&SecretKeyRef(reference)))
            .await
            .map_err(|_| unavailable())?
            .map_err(|_| unavailable())?;
        let key = std::str::from_utf8(secret.as_ref()).map_err(|_| unavailable())?;
        if self.provider == "anthropic" {
            tetonic_egress::HostedCredential::anthropic(key)
        } else if self.provider == "google" {
            tetonic_egress::HostedCredential::api_key("x-goog-api-key", key)
        } else {
            tetonic_egress::HostedCredential::bearer(key)
        }
        .map_err(|_| unavailable())
    }
}

impl WorkspaceServices {
    pub async fn remove_provider_key(
        &self,
        input: RemoveProviderKey,
    ) -> Result<LocalProvider, AppError> {
        let (name, _) = provider_info(&input.provider)?;
        let _serial = self.admission.lock().await;
        if let Some(reference) = self.keys.reference(&input.provider).await? {
            let vault = self.keys.vault.clone();
            let removal =
                tokio::task::spawn_blocking(move || vault.delete(&SecretKeyRef(reference)))
                    .await
                    .map_err(|_| key_error())?;
            if !matches!(
                removal,
                Ok(()) | Err(tetonic_domain::key_storage::KeyStorageError::Missing)
            ) {
                return Err(key_error());
            }
        }
        let provider = input.provider.clone();
        self.keys
            .store
            .write(move |db| db.set_local_provider_key(&provider, None))
            .await
            .map_err(|_| key_error())?
            .map_err(|_| key_error())?;
        Ok(LocalProvider {
            id: input.provider,
            name: name.into(),
            key_saved: false,
        })
    }

    pub(crate) async fn providers(&self) -> Vec<LocalProvider> {
        let mut providers = Vec::new();
        for (id, name) in [
            ("openai", "OpenAI"),
            ("anthropic", "Anthropic"),
            ("google", "Google"),
        ] {
            providers.push(LocalProvider {
                id: id.into(),
                name: name.into(),
                key_saved: self.keys.ready(id).await,
            });
        }
        providers
    }

    pub async fn save_provider_key(
        &self,
        input: SaveProviderKey,
    ) -> Result<LocalProvider, AppError> {
        let (name, _) = provider_info(&input.provider)?;
        // Header-safe and bounded. Do not echo rejected input or invoke the API.
        if input.api_key.len() < 8
            || input.api_key.len() > 4096
            || !input.api_key.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(AppError::InvalidRequest(
                "Enter a valid API key without whitespace.".into(),
            ));
        }
        let _serial = self.admission.lock().await;
        self.keys
            .save(&input.provider, input.api_key.into_bytes())
            .await?;
        Ok(LocalProvider {
            id: input.provider,
            name: name.into(),
            key_saved: true,
        })
    }

    pub(crate) async fn hosted_binding(
        &self,
        agent: &LocalAgent,
    ) -> Result<crate::resources::RegisteredHostedInference, AppError> {
        let endpoint = inference_endpoint(&agent.provider, &agent.model)?;
        let workspace_disclosure = if let Some(disclosure) = &agent.hosted_workspace {
            let root = self
                .host
                .settings
                .workspace_root
                .as_ref()
                .ok_or(AppError::WorkspaceUnavailable)?;
            let workspace =
                tetonic_tools::Workspace::new(root).map_err(|_| AppError::WorkspaceUnavailable)?;
            if workspace.root().to_str() != Some(disclosure) {
                return Err(AppError::PolicyDenied("The agent's approved hosted workspace no longer matches this host or its selected tools.".into()));
            }
            Some(disclosure.clone())
        } else {
            None
        };
        // Old definitions authorize only the exact OpenAI file-read profile they
        // originally approved. Never migrate legacy consent into new capabilities.
        let disclosure = agent.tool_disclosure.clone().or_else(|| {
            (agent.provider == "openai"
                && workspace_disclosure.is_some()
                && agent
                    .tools
                    .iter()
                    .all(|t| crate::resources::HOSTED_READ_TOOLS.contains(&t.as_str())))
            .then(|| {
                let mut tools = agent.tools.clone();
                tools.sort();
                tools.dedup();
                crate::resources::ToolDisclosure {
                    version: 1,
                    provider: agent.provider.clone(),
                    endpoint: endpoint.clone(),
                    tools,
                    workspace: workspace_disclosure.clone(),
                }
            })
        });
        if disclosure.as_ref().is_some_and(|d| {
            !d.matches(
                &agent.provider,
                &endpoint,
                &agent.tools,
                workspace_disclosure.as_deref(),
            )
        }) || (!agent.tools.is_empty() && disclosure.is_none())
            || (crate::resources::uses_workspace(&agent.tools) && workspace_disclosure.is_none())
        {
            return Err(AppError::PolicyDenied("Selected tools need approval for this model destination and data scope. Review the agent's access.".into()));
        }
        if !agent.hosted_consent || !self.keys.ready(&agent.provider).await {
            return Err(AppError::InvalidRequest(
                "Save a provider key and allow prompts to be sent to this provider in agent setup."
                    .into(),
            ));
        }
        let guard = self.host.app.host.guard();
        guard
            .allow_hosted_endpoint(&endpoint)
            .map_err(|_| AppError::InvalidRequest("Provider endpoint unavailable.".into()))?;
        let mut config = if agent.provider == "anthropic" {
            HostedModelConfig::anthropic(&agent.model, agent.max_tokens as u32)
        } else if agent.provider == "google" {
            HostedModelConfig::google(&agent.model, agent.max_tokens as u32)
        } else {
            HostedModelConfig::responses(&agent.model, agent.max_tokens as u32)
        };
        // Optional temperature is not accepted by several reasoning models.
        config.send_temperature = false;
        let transport: Arc<dyn tetonic_inference::hosted::HostedTransport> =
            Arc::new(EgressHostedTransport::new(
                guard,
                endpoint.clone(),
                Arc::new(StoredCredential {
                    keys: self.keys.clone(),
                    provider: agent.provider.clone(),
                }),
            ));
        #[cfg(test)]
        let transport = self.hosted_transport.clone().unwrap_or(transport);
        let provider = HostedChatProvider::new(
            config,
            tetonic_policy::HostedInferencePolicy::allow_up_to(DataClass::SensitiveSource),
            crate::secret_scanner_factory::scanner_from_shared_store(&self.host.app.host.store),
            transport,
        )
        .map_err(|_| AppError::InvalidRequest("Could not configure hosted model.".into()))?;
        Ok(crate::resources::RegisteredHostedInference {
            provider: Arc::new(provider),
            binding: format!(
                "hosted-v3:{}:{}:{}",
                agent.provider,
                endpoint,
                serde_json::to_string(&disclosure).unwrap_or_default()
            ),
            tool_disclosure: disclosure,
        })
    }
}

#[cfg(test)]
mod tests;

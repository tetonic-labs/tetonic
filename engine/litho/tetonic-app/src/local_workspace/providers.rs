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
        _ => Err(AppError::InvalidRequest(
            "Choose OpenAI or Anthropic.".into(),
        )),
    }
}

pub(super) struct ProviderKeys {
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

    pub(super) async fn ready(&self, provider: &str) -> bool {
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
        } else {
            tetonic_egress::HostedCredential::bearer(key)
        }
        .map_err(|_| unavailable())
    }
}

impl LocalWorkspace {
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

    pub(super) async fn providers(&self) -> Vec<LocalProvider> {
        let mut providers = Vec::new();
        for (id, name) in [("openai", "OpenAI"), ("anthropic", "Anthropic")] {
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

    pub(super) async fn hosted_binding(
        &self,
        agent: &LocalAgent,
    ) -> Result<crate::resources::RegisteredHostedInference, AppError> {
        let (_, endpoint) = provider_info(&agent.provider)?;
        if !agent.hosted_consent || !self.keys.ready(&agent.provider).await {
            return Err(AppError::InvalidRequest(
                "Save a provider key and allow prompts to be sent to this provider in agent setup."
                    .into(),
            ));
        }
        let guard = self.host.app.turn.guard();
        guard
            .allow_hosted_endpoint(endpoint)
            .map_err(|_| AppError::InvalidRequest("Provider endpoint unavailable.".into()))?;
        let mut config = if agent.provider == "anthropic" {
            HostedModelConfig::anthropic(&agent.model, agent.max_tokens as u32)
        } else {
            HostedModelConfig::responses(&agent.model, agent.max_tokens as u32)
        };
        // Optional temperature is not accepted by several reasoning models.
        config.send_temperature = false;
        let transport: Arc<dyn tetonic_inference::hosted::HostedTransport> =
            Arc::new(EgressHostedTransport::new(
                guard,
                endpoint.into(),
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
            crate::secret_scanner_factory::scanner_from_shared_store(&self.host.app.turn.store),
            transport,
        )
        .map_err(|_| AppError::InvalidRequest("Could not configure hosted model.".into()))?;
        Ok(crate::resources::RegisteredHostedInference {
            provider: Arc::new(provider),
            binding: format!("prompt-only-v1:{}:{}", agent.provider, endpoint),
        })
    }
}

#[cfg(test)]
mod tests;

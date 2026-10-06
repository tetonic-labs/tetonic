//! Account-backed discovery. Availability does not certify model tool capabilities.
use super::*;
use tetonic_inference::hosted::HostedTransport;

#[derive(Serialize)]
pub struct LocalModelCatalog {
    pub provider: String,
    pub models: Vec<String>,
    pub capabilities_verified: bool,
}

impl LocalWorkspace {
    pub async fn provider_models(&self, provider: &str) -> Result<LocalModelCatalog, AppError> {
        let (_, inference_endpoint) = provider_info(provider)?;
        if !self.keys.ready(provider).await {
            return Err(AppError::InvalidRequest(
                "Save this provider's API key to discover its models.".into(),
            ));
        }
        let endpoint = match provider {
            "openai" => "https://api.openai.com/v1/models",
            "anthropic" => "https://api.anthropic.com/v1/models",
            _ => unreachable!(),
        };
        let guard = self.host.app.turn.guard();
        guard.allow_hosted_endpoint(endpoint).map_err(|_| {
            AppError::InvalidRequest("Model discovery endpoint unavailable.".into())
        })?;
        let transport: Arc<dyn HostedTransport> = Arc::new(EgressHostedTransport::new(
            guard,
            inference_endpoint.into(),
            Arc::new(StoredCredential {
                keys: self.keys.clone(),
                provider: provider.into(),
            }),
        ));
        #[cfg(test)]
        let transport = self.hosted_transport.clone().unwrap_or(transport);
        let models = tokio::time::timeout(std::time::Duration::from_secs(20),
            discover(transport.as_ref(), endpoint)).await
            .map_err(|_| AppError::InvalidRequest("Model discovery timed out. Retry or enter a model ID.".into()))?
            .map_err(|_| AppError::InvalidRequest("Could not discover models. Check the provider key, account access and network, then retry or enter a model ID.".into()))?;
        Ok(LocalModelCatalog {
            provider: provider.into(),
            models,
            capabilities_verified: false,
        })
    }
}

async fn discover(
    transport: &dyn HostedTransport,
    endpoint: &str,
) -> Result<Vec<String>, InferenceError> {
    let mut models = std::collections::BTreeSet::new();
    let mut cursors = std::collections::HashSet::new();
    let mut cursor: Option<String> = None;
    // Bounded pagination, with no provider-returned URL or endpoint following.
    for _ in 0..25 {
        let page = transport.list_models(endpoint, cursor.as_deref()).await?;
        let entries = page["data"]
            .as_array()
            .ok_or_else(|| InferenceError::Decode("invalid model catalog".into()))?;
        for model in entries {
            let id = model["id"]
                .as_str()
                .filter(|id| {
                    !id.is_empty()
                        && id.len() <= 256
                        && !id.chars().any(|c| c.is_control() || c.is_whitespace())
                })
                .ok_or_else(|| InferenceError::Decode("invalid model ID".into()))?;
            models.insert(id.to_owned());
            if models.len() > 5000 {
                return Err(InferenceError::Decode("model catalog too large".into()));
            }
        }
        if page.get("has_more").is_none() || page["has_more"] == false {
            return Ok(models.into_iter().collect());
        }
        if page["has_more"] != true {
            return Err(InferenceError::Decode("invalid catalog pagination".into()));
        }
        let next = page["last_id"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 256)
            .ok_or_else(|| InferenceError::Decode("missing catalog cursor".into()))?;
        if entries.is_empty() || !cursors.insert(next.to_owned()) {
            return Err(InferenceError::Decode(
                "model catalog pagination did not advance".into(),
            ));
        }
        cursor = Some(next.into());
    }
    Err(InferenceError::Decode(
        "model catalog pagination limit reached".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::sync::Mutex;

    struct Catalog {
        pages: Mutex<Vec<Value>>,
        cursors: Mutex<Vec<Option<String>>>,
    }
    #[async_trait::async_trait]
    impl HostedTransport for Catalog {
        async fn complete(&self, _: Value) -> Result<Value, InferenceError> {
            panic!("discovery must not invoke inference")
        }
        async fn list_models(
            &self,
            endpoint: &str,
            cursor: Option<&str>,
        ) -> Result<Value, InferenceError> {
            assert_eq!(endpoint, "https://api.anthropic.com/v1/models");
            self.cursors.lock().unwrap().push(cursor.map(str::to_owned));
            Ok(self.pages.lock().unwrap().remove(0))
        }
    }
    #[tokio::test]
    async fn follows_bounded_cursors_and_deduplicates_models() {
        let catalog = Catalog {
            pages: Mutex::new(vec![
                json!({"data":[{"id":"z"},{"id":"a"}],"has_more":true,"last_id":"a"}),
                json!({"data":[{"id":"a"},{"id":"b"}],"has_more":false}),
            ]),
            cursors: Mutex::default(),
        };
        assert_eq!(
            discover(&catalog, "https://api.anthropic.com/v1/models")
                .await
                .unwrap(),
            vec!["a", "b", "z"]
        );
        assert_eq!(
            *catalog.cursors.lock().unwrap(),
            vec![None, Some("a".into())]
        );
    }
    #[tokio::test]
    async fn rejects_malformed_and_non_advancing_catalogs() {
        for page in [
            json!({}),
            json!({"data":[{"id":"bad\nmodel"}]}),
            json!({"data":[],"has_more":true,"last_id":"x"}),
            json!({"data":[{"id":"x"}],"has_more":true,"last_id":"x"}),
        ] {
            let catalog = Catalog {
                pages: Mutex::new(vec![page.clone(), page]),
                cursors: Mutex::default(),
            };
            assert!(discover(&catalog, "https://api.anthropic.com/v1/models")
                .await
                .is_err());
        }
    }
}

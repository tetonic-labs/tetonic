use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use lokai_domain::DataClass;
use lokai_inference::hosted::registry::{build_hosted_config, find_provider};
use lokai_inference::hosted::{
    AnthropicEnvCredentialSource, BearerEnvCredentialSource, EgressHostedTransport,
    HostedChatProvider, HostedCredentialSource, HostedWireProtocol,
};
use lokai_memory::RecoverMutex;
use lokai_policy::HostedInferencePolicy;

use crate::{errors::AppError, Application, ComputePlane};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceSelection {
    /// `default` is the application's currently installed admitted compute plane.
    pub profile: String,
    pub model_fast: String,
    pub model_hard: String,
    pub revision: u64,
}

#[derive(Debug, Clone)]
pub struct ChangeInferenceCommand {
    pub session_id: String,
    pub profile: String,
    pub model_fast: String,
    pub model_hard: String,
    /// Optimistic concurrency: stale editors cannot overwrite a newer selection.
    pub expected_revision: u64,
}

#[derive(Clone)]
pub(crate) struct RegisteredProfile {
    pub provider: Arc<dyn lokai_inference::InferenceProvider>,
    pub broker: Option<Arc<lokai_broker::DefaultComputeBroker>>,
    pub models: Vec<String>,
    pub num_ctx: u32,
}

#[derive(Default)]
pub(crate) struct InferenceProfiles(
    Mutex<HashMap<String, RegisteredProfile>>,
    Mutex<Vec<String>>,
);

/// Product-facing choice. IDs are opaque; portals must not decode provider routing.
#[derive(Debug, Clone)]
pub struct ModelChoice {
    pub id: String,
    pub name: String,
    pub provider_label: String,
    pub availability: String,
    pub current: bool,
    pub requires_auth: bool,
    pub provider_id: Option<String>,
    pub login_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ModelCatalog {
    pub revision: u64,
    pub models: Vec<ModelChoice>,
}

impl InferenceProfiles {
    pub fn get(&self, name: &str) -> Option<RegisteredProfile> {
        self.0.lock_recover().get(name).cloned()
    }
}

impl Application {
    /// Host-owned discovery snapshot, not a provider enrollment or permission grant.
    pub fn set_default_model_inventory(&self, mut models: Vec<String>) {
        models.retain(|model| valid_model(model));
        models.sort();
        models.dedup();
        *self.turn.inference_profiles.1.lock_recover() = models;
    }

    pub fn model_catalog(&self, session_id: &str) -> Result<ModelCatalog, AppError> {
        let selected = self.session_inference(session_id)?;
        let mut choices: Vec<ModelChoice> = Vec::new();

        // 1. Local discovered models (from Ollama or default inventory)
        let local_models = self.turn.inference_profiles.1.lock_recover().clone();
        for m in local_models {
            choices.push(ModelChoice {
                id: choice_id("default", &m),
                current: selected.profile == "default"
                    && (m == selected.model_fast || m == selected.model_hard),
                name: m,
                provider_label: "Local (Ollama)".into(),
                availability: "Local model".into(),
                requires_auth: false,
                provider_id: None,
                login_url: None,
            });
        }

        // 2. Explicitly registered profiles
        let profiles = self.turn.inference_profiles.0.lock_recover().clone();
        for (name, profile) in profiles.iter() {
            for m in &profile.models {
                choices.push(ModelChoice {
                    id: choice_id(name, m),
                    current: selected.profile == *name
                        && (m == &selected.model_fast || m == &selected.model_hard),
                    name: m.clone(),
                    provider_label: name.clone(),
                    availability: "Configured profile".into(),
                    requires_auth: false,
                    provider_id: Some(name.clone()),
                    login_url: None,
                });
            }
        }

        // 3. Cloud / third-party providers from lokai-inference registry
        for p in lokai_inference::hosted::registry::PROVIDERS {
            let pid = p.provider_id;
            let is_authed = p.is_credential_configured() || profiles.contains_key(pid);
            let availability = if is_authed {
                "Ready (Key configured)".to_string()
            } else {
                "Requires API key or account login".to_string()
            };

            for m in p.models {
                if !choices
                    .iter()
                    .any(|c| c.provider_id.as_deref() == Some(pid) && c.name == *m)
                {
                    choices.push(ModelChoice {
                        id: choice_id(pid, m),
                        current: selected.profile == pid
                            && (*m == selected.model_fast || *m == selected.model_hard),
                        name: m.to_string(),
                        provider_label: p.name.into(),
                        availability: availability.clone(),
                        requires_auth: !is_authed,
                        provider_id: Some(pid.to_string()),
                        login_url: Some(p.login_url.to_string()),
                    });
                }
            }
        }

        // Keep configured session models discoverable even when discovery is unavailable
        for model in [&selected.model_fast, &selected.model_hard] {
            if !choices.iter().any(|c| &c.name == model && c.current) {
                choices.push(ModelChoice {
                    id: choice_id(&selected.profile, model),
                    current: true,
                    name: model.clone(),
                    provider_label: if selected.profile == "default" {
                        "Default provider".into()
                    } else {
                        selected.profile.clone()
                    },
                    availability: "Configured; availability not verified".into(),
                    requires_auth: false,
                    provider_id: if selected.profile == "default" {
                        None
                    } else {
                        Some(selected.profile.clone())
                    },
                    login_url: None,
                });
            }
        }

        choices.sort_by(|a, b| {
            b.current
                .cmp(&a.current)
                .then_with(|| a.provider_label.cmp(&b.provider_label))
                .then_with(|| a.name.cmp(&b.name))
        });

        Ok(ModelCatalog {
            revision: selected.revision,
            models: choices,
        })
    }

    pub fn select_session_model(
        &self,
        session_id: &str,
        choice: &str,
        expected_revision: u64,
    ) -> Result<InferenceSelection, AppError> {
        let catalog = self.model_catalog(session_id)?;
        let target = catalog
            .models
            .iter()
            .find(|m| m.id == choice)
            .ok_or_else(|| {
                AppError::InvalidRequest(
                    "model choice is no longer available; reopen the model picker".into(),
                )
            })?;

        if target.requires_auth {
            return Err(AppError::InvalidRequest(
                "model requires authentication; enter API key or log in".into(),
            ));
        }

        let profile = target.provider_id.as_deref().unwrap_or("default");
        if profile != "default" && self.turn.inference_profiles.get(profile).is_none() {
            if let Some(desc) = find_provider(profile) {
                if let Some(key) = desc.get_configured_credential() {
                    self.register_cloud_provider_model(profile, &key, None, Some(&target.name))?;
                } else {
                    return Err(AppError::InvalidRequest(format!(
                        "provider '{}' requires authentication credentials",
                        desc.name
                    )));
                }
            }
        }

        self.change_session_inference(ChangeInferenceCommand {
            session_id: session_id.into(),
            profile: profile.to_string(),
            model_fast: target.name.clone(),
            model_hard: target.name.clone(),
            expected_revision,
        })
    }

    pub fn register_cloud_provider(
        &self,
        provider_id: &str,
        api_key: &str,
        custom_endpoint: Option<&str>,
    ) -> Result<(), AppError> {
        self.register_cloud_provider_model(provider_id, api_key, custom_endpoint, None)
    }

    pub fn register_cloud_provider_model(
        &self,
        provider_id: &str,
        api_key: &str,
        custom_endpoint: Option<&str>,
        target_model: Option<&str>,
    ) -> Result<(), AppError> {
        let trimmed_key = api_key.trim();
        if trimmed_key.is_empty() {
            return Err(AppError::InvalidRequest("API key cannot be empty".into()));
        }

        let p = find_provider(provider_id).ok_or_else(|| {
            AppError::InvalidRequest(format!("unknown provider: '{provider_id}'"))
        })?;

        if let Some(first_key) = p.env_keys.first() {
            std::env::set_var(first_key, trimmed_key);
        }

        let guard = self.turn.guard();
        let endpoint = custom_endpoint
            .filter(|e| !e.trim().is_empty())
            .unwrap_or(p.default_endpoint);

        guard.allow_hosted_endpoint(endpoint).map_err(|e| {
            AppError::InvalidRequest(format!("failed to allow hosted endpoint '{endpoint}': {e}"))
        })?;

        let cred_source: Arc<dyn HostedCredentialSource> =
            if p.protocol == HostedWireProtocol::AnthropicMessages {
                Arc::new(AnthropicEnvCredentialSource::new(Some(
                    trimmed_key.to_string(),
                )))
            } else {
                Arc::new(BearerEnvCredentialSource::new(
                    p.env_keys[0],
                    Some(trimmed_key.to_string()),
                ))
            };

        let transport = Arc::new(EgressHostedTransport::new(
            guard,
            endpoint.to_string(),
            cred_source,
        ));

        let active_model = target_model.unwrap_or(p.default_model);
        let num_ctx = self.turn.num_ctx.load(std::sync::atomic::Ordering::Relaxed);
        let mut config = build_hosted_config(active_model, Some(num_ctx));
        for m in p.models {
            if !config.allowed_models.iter().any(|existing| existing == m) {
                config.allowed_models.push(m.to_string());
            }
        }
        if let Some(target) = target_model {
            if !config
                .allowed_models
                .iter()
                .any(|existing| existing == target)
            {
                config.allowed_models.push(target.to_string());
            }
        }

        let policy = HostedInferencePolicy::allow_up_to(DataClass::SensitiveSource);
        let scanner = crate::secret_scanner_factory::scanner_from_shared_store(&self.turn.store);

        let provider = Arc::new(
            HostedChatProvider::new(config, policy, scanner, transport).map_err(|e| {
                AppError::InvalidRequest(format!("initializing hosted provider: {e}"))
            })?,
        );

        let mut models: Vec<String> = p.models.iter().map(|m| m.to_string()).collect();
        if let Some(target) = target_model {
            if !models.iter().any(|existing| existing == target) {
                models.push(target.to_string());
            }
        }
        let broker = self.turn.compute_broker();

        let mut profiles = self.turn.inference_profiles.0.lock_recover();
        profiles.insert(
            provider_id.to_string(),
            RegisteredProfile {
                provider: provider as Arc<dyn lokai_inference::InferenceProvider>,
                broker,
                models,
                num_ctx,
            },
        );

        Ok(())
    }

    pub fn configure_and_select_cloud_model(
        &self,
        session_id: &str,
        provider_id: &str,
        api_key: &str,
        model: &str,
    ) -> Result<InferenceSelection, AppError> {
        self.register_cloud_provider_model(provider_id, api_key, None, Some(model))?;
        let current = self.session_inference(session_id)?;
        self.change_session_inference(ChangeInferenceCommand {
            session_id: session_id.to_string(),
            profile: provider_id.to_string(),
            model_fast: model.to_string(),
            model_hard: model.to_string(),
            expected_revision: current.revision,
        })
    }

    /// Host composition API, deliberately not exposed through RPC. Only complete
    /// admitted compute planes can be registered; a raw HTTP adapter cannot bypass
    /// broker scanning, admission, scheduling or attempt ownership here.
    pub fn register_inference_profile(
        &self,
        name: &str,
        plane: &ComputePlane,
        models: Vec<String>,
        num_ctx: u32,
    ) -> Result<(), AppError> {
        if name.is_empty()
            || name == "default"
            || name.chars().any(char::is_whitespace)
            || models.is_empty()
            || models.iter().any(|m| !valid_model(m))
            || num_ctx <= 1024
            || !plane.provider.has_secret_scanner()
            || !Arc::ptr_eq(plane.provider.broker(), &plane.compute_broker)
        {
            return Err(AppError::InvalidRequest(
                "invalid inference profile name, models, context budget, or admitted provider/broker pair".into(),
            ));
        }
        let mut profiles = self.turn.inference_profiles.0.lock_recover();
        if profiles.contains_key(name) {
            return Err(AppError::InvalidRequest(
                "inference profile already registered; use a new name".into(),
            ));
        }
        self.attach_compute_lifecycle(Some(&plane.compute_broker), &plane.fabric_remotes);
        profiles.insert(
            name.into(),
            RegisteredProfile {
                provider: plane.provider.clone() as Arc<dyn lokai_inference::InferenceProvider>,
                broker: Some(plane.compute_broker.clone()),
                models,
                num_ctx,
            },
        );
        Ok(())
    }

    pub fn inference_profiles(&self) -> Vec<String> {
        let mut names: Vec<_> = self
            .turn
            .inference_profiles
            .0
            .lock_recover()
            .keys()
            .cloned()
            .collect();
        names.push("default".into());
        names.sort();
        names
    }

    pub fn session_inference(&self, session_id: &str) -> Result<InferenceSelection, AppError> {
        Ok(self.sessions.live(session_id)?.inference_selection())
    }

    pub fn change_session_inference(
        &self,
        command: ChangeInferenceCommand,
    ) -> Result<InferenceSelection, AppError> {
        if !valid_model(&command.model_fast) || !valid_model(&command.model_hard) {
            return Err(AppError::InvalidRequest(
                "model IDs must be nonempty and contain no whitespace".into(),
            ));
        }
        if command.profile != "default" {
            let profile = self
                .turn
                .inference_profiles
                .get(&command.profile)
                .ok_or_else(|| AppError::InvalidRequest("unknown inference profile".into()))?;
            if !profile.models.contains(&command.model_fast)
                || !profile.models.contains(&command.model_hard)
            {
                return Err(AppError::InvalidRequest(
                    "model is not allowed by the inference profile".into(),
                ));
            }
        } else if self.turn.provider().is_none() {
            return Err(AppError::InvalidRequest(
                "default inference provider is not bound".into(),
            ));
        }
        self.sessions
            .live(&command.session_id)?
            .change_inference(command)
    }
}

fn valid_model(model: &str) -> bool {
    !model.is_empty()
        && model.len() <= 256
        && !model.chars().any(|c| c.is_whitespace() || c.is_control())
}

fn choice_id(profile: &str, model: &str) -> String {
    format!("{}:{profile}{model}", profile.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::StartSessionCommand;

    #[tokio::test]
    async fn catalog_selection_is_stable_validated_and_revision_checked() {
        let dir = tempfile::tempdir().unwrap();
        let (sink, _) = crate::events::RecordingEventSink::new();
        let app = Application::bootstrap_mock(dir.path(), sink, vec![]);
        let session = app
            .sessions
            .start_session(StartSessionCommand {
                workspace_root: dir.path().display().to_string(),
                briefing: Some(false),
                model_fast: Some("original".into()),
                model_hard: Some("original".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        let sid = &session.session_id;
        app.set_default_model_inventory(vec!["second".into(), "second".into(), "bad id".into()]);
        let catalog = app.model_catalog(sid).unwrap();
        assert!(catalog.models.len() >= 2);
        assert!(catalog
            .models
            .iter()
            .any(|m| m.name == "original" && m.current));
        let id = catalog
            .models
            .iter()
            .find(|m| m.name == "second")
            .unwrap()
            .id
            .clone();
        let live = app.sessions.live(sid).unwrap();
        live.try_begin_turn().unwrap();
        assert!(app
            .select_session_model(sid, &id, catalog.revision)
            .is_err());
        live.end_turn();
        assert!(app.select_session_model(sid, "invented", 0).is_err());
        let changed = app
            .select_session_model(sid, &id, catalog.revision)
            .unwrap();
        assert_eq!(changed.model_fast, "second");
        assert_eq!(changed.model_hard, "second");
        assert!(app
            .select_session_model(sid, &id, catalog.revision)
            .is_err());
        assert_ne!(choice_id("a", "bc"), choice_id("ab", "c"));
    }

    #[tokio::test]
    async fn selection_reaches_turn_host_and_unknown_profiles_do_not_mutate() {
        let dir = tempfile::tempdir().unwrap();
        let (sink, _) = crate::events::RecordingEventSink::new();
        let app = Application::bootstrap_mock(dir.path(), sink, vec![]);
        let session = app
            .sessions
            .start_session(StartSessionCommand {
                workspace_root: dir.path().display().to_string(),
                briefing: Some(false),
                model_fast: Some("initial".into()),
                model_hard: Some("initial-hard".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        let command = ChangeInferenceCommand {
            session_id: session.session_id.clone(),
            profile: "missing".into(),
            model_fast: "new".into(),
            model_hard: "new".into(),
            expected_revision: 0,
        };
        assert!(app.change_session_inference(command.clone()).is_err());
        assert_eq!(
            app.session_inference(&session.session_id).unwrap().revision,
            0
        );
        app.change_session_inference(ChangeInferenceCommand {
            profile: "default".into(),
            ..command
        })
        .unwrap();
        let host = app.build_session_host(&session.session_id).unwrap();
        assert_eq!(host.model_fast, "new");
        assert_eq!(host.model_hard, "new");
        assert!(host.tokenizer.estimated());
    }

    #[tokio::test]
    async fn registered_profile_replaces_provider_broker_and_budget_together() {
        let dir = tempfile::tempdir().unwrap();
        let (sink, _) = crate::events::RecordingEventSink::new();
        let app = Application::bootstrap_mock(dir.path(), sink, vec![]);
        let pooled = Arc::new(lokai_inference::PooledProvider::new(
            Arc::new(lokai_inference::OllamaProvider::new(
                "http://127.0.0.1:11434",
                Arc::new(lokai_egress::EgressGuard::new()),
            )),
            vec![],
        ));
        let (provider, broker) =
            crate::compute_plane::wrap_pooled_with_broker(pooled.clone(), None, None);
        let plane = ComputePlane {
            provider: provider.clone(),
            compute_broker: broker.clone(),
            pooled: Some(pooled),
            compute_registry: None,
            fabric_remotes: vec![],
            coordinator: None,
            worker_activity_targets: vec![],
        };
        app.register_inference_profile("secondary", &plane, vec!["allowed".into()], 4096)
            .unwrap();
        assert!(app
            .register_inference_profile("secondary", &plane, vec!["other".into()], 8192)
            .is_err());
        let session = app
            .sessions
            .start_session(StartSessionCommand {
                workspace_root: dir.path().display().to_string(),
                briefing: Some(false),
                ..Default::default()
            })
            .await
            .unwrap();
        let command = ChangeInferenceCommand {
            session_id: session.session_id.clone(),
            profile: "secondary".into(),
            model_fast: "forbidden".into(),
            model_hard: "allowed".into(),
            expected_revision: 0,
        };
        assert!(app.change_session_inference(command.clone()).is_err());
        let catalog = app.model_catalog(&session.session_id).unwrap();
        let choice = catalog
            .models
            .iter()
            .find(|m| m.provider_label == "secondary")
            .unwrap();
        app.select_session_model(&session.session_id, &choice.id, catalog.revision)
            .unwrap();
        let host = app.build_session_host(&session.session_id).unwrap();
        let expected_provider: Arc<dyn lokai_inference::InferenceProvider> = provider;
        assert!(Arc::ptr_eq(&host.provider, &expected_provider));
        assert!(Arc::ptr_eq(host.compute_broker.as_ref().unwrap(), &broker));
        assert_eq!(host.num_ctx, 4096);
    }

    #[tokio::test]
    async fn catalog_includes_cloud_models_and_configures_with_key() {
        let dir = tempfile::tempdir().unwrap();
        let (sink, _) = crate::events::RecordingEventSink::new();
        let app = Application::bootstrap_mock(dir.path(), sink, vec![]);
        let session = app
            .sessions
            .start_session(StartSessionCommand {
                workspace_root: dir.path().display().to_string(),
                briefing: Some(false),
                model_fast: Some("local-model".into()),
                model_hard: Some("local-model".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        let catalog = app.model_catalog(&session.session_id).unwrap();
        let cloud_model = catalog
            .models
            .iter()
            .find(|m| m.provider_id.as_deref() == Some("anthropic"))
            .expect("anthropic cloud model in catalog");

        let selection = app
            .configure_and_select_cloud_model(
                &session.session_id,
                "anthropic",
                "test-api-key",
                &cloud_model.name,
            )
            .unwrap();
        assert_eq!(selection.profile, "anthropic");
        assert_eq!(selection.model_fast, cloud_model.name);

        let updated_catalog = app.model_catalog(&session.session_id).unwrap();
        let selected_entry = updated_catalog
            .models
            .iter()
            .find(|m| m.name == cloud_model.name)
            .unwrap();
        assert!(selected_entry.current);
        assert!(!selected_entry.requires_auth);
    }
}

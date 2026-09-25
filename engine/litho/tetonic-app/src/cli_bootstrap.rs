//! CLI application bootstrap — single entry point for tetonic-cli.
//!
//! Encapsulates all egress guard setup, compute plane assembly, database opening,
//! and session startup behind the product facade.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tetonic_domain::DataClass;
use tetonic_inference::hosted::registry::{
    build_hosted_config, find_provider, resolve_provider_for_model,
};
use tetonic_inference::hosted::{
    AnthropicEnvCredentialSource, BearerEnvCredentialSource, EgressHostedTransport,
    HostedChatProvider, HostedCredentialSource,
};
use tetonic_policy::HostedInferencePolicy;

use crate::commands::{InitializeCommand, StartSessionCommand, StartSessionResultPayload};
use crate::errors::AppError;
use crate::events::ApplicationEventSink;
use crate::Application;

pub struct CliBootstrapParams {
    pub workspace: String,
    pub ollama: String,
    pub model: Option<String>,
    pub model_hard: Option<String>,
    pub model_tier: String,
    pub allow_shell: bool,
    pub explain: bool,
    pub no_verify: bool,
    pub verify: Option<String>,
    pub orchestrate: String,
    pub no_critic: bool,
    pub llm_router: bool,
    pub max_steps: usize,
    pub num_ctx: u32,
    pub event_sink: Arc<dyn ApplicationEventSink>,
    pub anthropic_key: Option<String>,
    pub openai_key: Option<String>,
    pub endpoint: Option<String>,
}

pub struct CliBootstrapOutput {
    pub app: Arc<Application>,
    pub workspace_root: String,
    pub session_id: String,
    pub session_result: StartSessionResultPayload,
    pub model_fast: String,
    pub model_hard: String,
    pub ollama_base: String,
    pub verify_resolved: Option<String>,
}

impl Application {
    pub async fn bootstrap_cli(params: CliBootstrapParams) -> Result<CliBootstrapOutput, AppError> {
        let startup_timer =
            tetonic_telemetry::StageTimer::start(tetonic_telemetry::PerfStage::Startup);
        let guard = Arc::new(tetonic_egress::EgressGuard::new());

        // 1. Audit store
        let store = open_default_audit_store();
        if let Some(ref s) = store {
            let _ = s.read_sync(|db| {
                let _ = crate::estate_enrollment::reload_enrollment_egress(db, guard.as_ref());
            });
        }
        let defaults = store
            .as_ref()
            .and_then(|s| {
                s.read_sync(|db| {
                    tetonic_capacity::load_inference_defaults(db, tetonic_capacity::LOCAL_NODE_ID)
                })
                .ok()
            })
            .unwrap_or_else(tetonic_capacity::InferenceDefaults::fallback);
        let (mut model_fast, mut model_hard) = resolve_models(
            params.model.as_deref(),
            params.model_hard.as_deref(),
            &defaults,
        );
        let selected_model = if params.model_tier.eq_ignore_ascii_case("hard") {
            &model_hard
        } else {
            &model_fast
        };
        crate::install_shared_scanner_from_store(&store);

        let hosted_setup = resolve_hosted_setup(&params, selected_model, guard.clone(), &store)?;

        let (installed, active_endpoint) = if let Some(ref hosted) = hosted_setup {
            model_fast = hosted.model.clone();
            model_hard = hosted.model.clone();
            (vec![hosted.model.clone()], hosted.endpoint.clone())
        } else {
            // 2. Local Ollama provider checks
            let provider = Arc::new(tetonic_inference::OllamaProvider::new(
                &params.ollama,
                guard.clone(),
            ));
            let installed = provider.list_models().await.map_err(|_| {
                AppError::InvalidRequest(format!(
                    "Ollama is not reachable at {} (is `ollama serve` running?)",
                    params.ollama
                ))
            })?;
            if !installed.iter().any(|m| m == selected_model) {
                tracing::warn!(
                    "model '{}' not found locally. Installed: {}",
                    selected_model,
                    if installed.is_empty() {
                        "(none)".to_string()
                    } else {
                        installed.join(", ")
                    }
                );
            } else {
                let prewarm_provider = provider.clone();
                let prewarm_model = selected_model.clone();
                let num_ctx = params.num_ctx;
                tokio::spawn(async move {
                    if let Err(error) = prewarm_provider
                        .prewarm_with_context(&prewarm_model, None, Some(num_ctx))
                        .await
                    {
                        tracing::warn!("startup model warm-up failed: {error}");
                    }
                });
            }

            if let Ok(caps) = provider.model_capabilities(selected_model).await {
                if !caps.is_empty() && !caps.iter().any(|c| c == "tools") {
                    return Err(AppError::InvalidRequest(format!(
                        "model '{}' does not support tool calling (capabilities: {}).\nPick a tool-capable model, e.g. qwen3.5:latest or qwen3-coder.",
                        selected_model,
                        caps.join(", ")
                    )));
                }
            }
            (installed, params.ollama.clone())
        };

        // 3. Workspace resolution
        let ws = tetonic_tools::Workspace::new(Path::new(&params.workspace)).map_err(|e| {
            AppError::InvalidRequest(format!("opening workspace '{}': {e}", params.workspace))
        })?;
        let workspace_root = ws.root().display().to_string();

        let verify_explicit = if params.no_verify || params.explain {
            Some("none")
        } else {
            params.verify.as_deref()
        };
        let verify_resolved =
            tetonic_tools::resolve_verify_cmd(verify_explicit, Path::new(&workspace_root));

        // 4. Disposable code index
        let mut index_db: Option<PathBuf> = None;
        if let Some(dirs) = directories::ProjectDirs::from("", "", "lokai") {
            let db_path = dirs.data_dir().join("index.db");
            if let Ok(idx) = tetonic_index::Index::open(&db_path) {
                index_db = Some(db_path);
                let _ = idx.prune_missing_workspaces();
                let _ = idx.index_workspace(Path::new(&workspace_root));
            }
        }

        // 5. Bootstrap Application kernel
        let init_cmd = InitializeCommand {
            workspace_root: workspace_root.clone(),
            rpc_token_provided: false,
            rpc_auth_disabled: true,
        };
        let (app, _, bootstrap) =
            Application::bootstrap(init_cmd, store.clone(), params.event_sink, index_db, None)
                .await?;
        let app = Arc::new(app);
        app.bind_num_ctx(params.num_ctx);
        app.set_default_model_inventory(installed);

        // 6. Compute plane assembly & installation
        let runtime = bootstrap.runtime.clone();
        let coordinator = store.as_ref().and_then(|s| {
            s.read_sync(|db| {
                let dir = directories::ProjectDirs::from("", "", "lokai")?
                    .data_dir()
                    .to_path_buf();
                crate::estate_enrollment::load_or_create_coordinator(db, &dir).ok()
            })
            .unwrap_or(None)
            .map(Arc::new)
        });
        let plane = crate::build_compute_plane(crate::ComputePlaneRequest {
            guard: guard.clone(),
            ollama_base: params.ollama.clone(),
            policy: bootstrap.policy.clone(),
            workspace_root: PathBuf::from(&workspace_root),
            artifact_store: runtime.artifact_store().clone(),
            store: store.clone(),
            coordinator,
            placement_sink: None,
            previous_pooled: None,
        })
        .await;
        app.install_compute_services(&plane);
        if let Some(ref hosted) = hosted_setup {
            app.bind_inference(hosted.provider.clone(), Some(plane.compute_broker.clone()));
            app.attach_egress(guard.clone(), hosted.endpoint.clone());
        } else {
            app.attach_egress(guard.clone(), params.ollama.clone());
        }

        // 7. Session configuration and startup
        let orchestration = tetonic_orchestrator::OrchestrationMode::parse(&params.orchestrate);
        let critic_enabled = orchestration == tetonic_orchestrator::OrchestrationMode::Auto
            && !params.no_critic
            && !params.explain;
        let llm_router = params.llm_router
            || std::env::var("LOKAI_LLM_ROUTER")
                .ok()
                .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));

        let resume_active = store.as_ref().and_then(|s| {
            s.read_sync(|db| {
                let Ok(Some(sid)) = db.find_latest_session_for_workspace(&workspace_root) else {
                    return false;
                };
                matches!(
                    db.session_status(&sid).ok().flatten().as_deref(),
                    Some("running")
                )
            })
            .ok()
        });

        let mut start_cmd = StartSessionCommand {
            workspace_root: workspace_root.clone(),
            resume: resume_active,
            model_tier: if params.model_tier.eq_ignore_ascii_case("hard") {
                Some("hard".into())
            } else {
                None
            },
            session_id: None,
            goal: None,
            data_class: None,
            verify_cmd: verify_resolved.clone(),
            briefing: Some(true),
            orchestration: Some(params.orchestrate),
            critic: Some(critic_enabled),
            llm_router: Some(llm_router),
            model_fast: Some(model_fast.clone()),
            model_hard: Some(model_hard.clone()),
            session_max_steps: Some(params.max_steps),
            allow_shell: Some(params.allow_shell),
            force_explain: Some(params.explain),
            auto_grant_approvals: None,
        };

        let session_result = match app.sessions.start_session(start_cmd.clone()).await {
            Ok(r) => r,
            Err(e)
                if start_cmd.resume == Some(true)
                    && e.to_string().contains("no prior session to resume") =>
            {
                start_cmd.resume = None;
                app.sessions.start_session(start_cmd).await?
            }
            Err(e) => return Err(e),
        };

        let _ = tetonic_tools::ensure_session_worktree(
            Path::new(&workspace_root),
            &session_result.session_id,
        );

        startup_timer.finish(true);
        Ok(CliBootstrapOutput {
            app,
            workspace_root,
            session_id: session_result.session_id.clone(),
            session_result,
            model_fast,
            model_hard,
            ollama_base: active_endpoint,
            verify_resolved,
        })
    }

    pub async fn bootstrap_offline(workspace: Option<&str>) -> Result<Arc<Application>, AppError> {
        let store = open_default_audit_store();
        let guard = Arc::new(tetonic_egress::EgressGuard::new());
        if let Some(ref s) = store {
            let _ = s.read_sync(|db| {
                let _ = crate::estate_enrollment::reload_enrollment_egress(db, guard.as_ref());
            });
        }
        let ws_root = match workspace {
            Some(w) => tetonic_tools::Workspace::new(Path::new(w))
                .map_err(|e| AppError::InvalidRequest(format!("opening workspace '{w}': {e}")))?
                .root()
                .display()
                .to_string(),
            None => store
                .as_ref()
                .and_then(|s| {
                    s.read_sync(|db| {
                        db.list_recent_sessions(1)
                            .ok()?
                            .into_iter()
                            .next()
                            .map(|x| x.workspace_root)
                    })
                    .ok()
                    .flatten()
                })
                .unwrap_or_else(|| {
                    std::env::current_dir()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|_| ".".to_string())
                }),
        };

        let index_db = crate::cli_index::default_index_db_path().ok();
        let init_cmd = InitializeCommand {
            workspace_root: ws_root,
            rpc_token_provided: false,
            rpc_auth_disabled: true,
        };
        let (app, _, _) = Application::bootstrap(
            init_cmd,
            store,
            Arc::new(crate::events::NoopEventSink),
            index_db,
            None,
        )
        .await?;
        let app = Arc::new(app);
        app.attach_egress(guard, "http://127.0.0.1:11434".to_string());
        Ok(app)
    }

    pub async fn bootstrap_cli_app(
        workspace: Option<&str>,
        sink: Arc<dyn ApplicationEventSink>,
    ) -> Result<Arc<Application>, AppError> {
        let store = open_default_audit_store();
        let guard = Arc::new(tetonic_egress::EgressGuard::new());
        if let Some(ref s) = store {
            let _ = s.read_sync(|db| {
                let _ = crate::estate_enrollment::reload_enrollment_egress(db, guard.as_ref());
            });
        }
        let ws_root = match workspace {
            Some(w) => tetonic_tools::Workspace::new(Path::new(w))
                .map_err(|e| AppError::InvalidRequest(format!("opening workspace '{w}': {e}")))?
                .root()
                .display()
                .to_string(),
            None => std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| ".".to_string()),
        };

        let index_db = crate::cli_index::default_index_db_path().ok();
        let init_cmd = InitializeCommand {
            workspace_root: ws_root,
            rpc_token_provided: false,
            rpc_auth_disabled: true,
        };
        let (app, _, _) = Application::bootstrap(init_cmd, store, sink, index_db, None).await?;
        let app = Arc::new(app);
        app.attach_egress(guard, "http://127.0.0.1:11434".to_string());
        Ok(app)
    }
}

struct HostedSetup {
    model: String,
    endpoint: String,
    provider: Arc<dyn tetonic_inference::InferenceProvider>,
}

fn resolve_hosted_setup(
    params: &CliBootstrapParams,
    selected_model: &str,
    guard: Arc<tetonic_egress::EgressGuard>,
    store: &Option<tetonic_memory::SharedStore>,
) -> Result<Option<HostedSetup>, AppError> {
    let anthropic_key = params
        .anthropic_key
        .clone()
        .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
        .or_else(|| std::env::var("CLAUDE_API_KEY").ok())
        .filter(|k| !k.trim().is_empty());

    let openai_key = params
        .openai_key
        .clone()
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .filter(|k| !k.trim().is_empty());

    let deepseek_key = std::env::var("DEEPSEEK_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty());

    // 1. Check if the model explicitly maps to a known cloud provider
    let explicit_provider = resolve_provider_for_model(selected_model);

    let (provider_id, active_model, default_endpoint, cred_source): (
        &str,
        String,
        &str,
        Arc<dyn HostedCredentialSource>,
    ) = match explicit_provider {
        Some(p) if p.provider_id == "anthropic" => {
            let key = anthropic_key.ok_or_else(|| {
                AppError::InvalidRequest(format!(
                    "model '{selected_model}' requires an Anthropic API key via --anthropic-key, ANTHROPIC_API_KEY, or CLAUDE_API_KEY"
                ))
            })?;
            (
                "anthropic",
                selected_model.to_string(),
                p.default_endpoint,
                Arc::new(AnthropicEnvCredentialSource::new(Some(key))),
            )
        }
        Some(p) if p.provider_id == "openai" => {
            let key = openai_key.ok_or_else(|| {
                AppError::InvalidRequest(format!(
                    "model '{selected_model}' requires an OpenAI API key via --openai-key or OPENAI_API_KEY"
                ))
            })?;
            (
                "openai",
                selected_model.to_string(),
                p.default_endpoint,
                Arc::new(BearerEnvCredentialSource::new("OPENAI_API_KEY", Some(key))),
            )
        }
        Some(p) if p.provider_id == "deepseek" => {
            let key = deepseek_key.ok_or_else(|| {
                AppError::InvalidRequest(format!(
                    "model '{selected_model}' requires a DeepSeek API key via DEEPSEEK_API_KEY"
                ))
            })?;
            (
                "deepseek",
                selected_model.to_string(),
                p.default_endpoint,
                Arc::new(BearerEnvCredentialSource::new(
                    "DEEPSEEK_API_KEY",
                    Some(key),
                )),
            )
        }
        _ => {
            // 2. If model was not explicitly specified on CLI, or explicit hosted flags were given
            if params.model.is_none()
                || params.anthropic_key.is_some()
                || params.openai_key.is_some()
            {
                if let Some(key) = anthropic_key {
                    let p = find_provider("anthropic").expect("anthropic provider");
                    let model = params
                        .model
                        .clone()
                        .unwrap_or_else(|| p.default_model.to_string());
                    (
                        "anthropic",
                        model,
                        p.default_endpoint,
                        Arc::new(AnthropicEnvCredentialSource::new(Some(key))),
                    )
                } else if let Some(key) = openai_key {
                    let p = find_provider("openai").expect("openai provider");
                    let model = params
                        .model
                        .clone()
                        .unwrap_or_else(|| p.default_model.to_string());
                    (
                        "openai",
                        model,
                        p.default_endpoint,
                        Arc::new(BearerEnvCredentialSource::new("OPENAI_API_KEY", Some(key))),
                    )
                } else if let Some(key) = deepseek_key {
                    let p = find_provider("deepseek").expect("deepseek provider");
                    let model = params
                        .model
                        .clone()
                        .unwrap_or_else(|| p.default_model.to_string());
                    (
                        "deepseek",
                        model,
                        p.default_endpoint,
                        Arc::new(BearerEnvCredentialSource::new(
                            "DEEPSEEK_API_KEY",
                            Some(key),
                        )),
                    )
                } else {
                    return Ok(None);
                }
            } else {
                return Ok(None);
            }
        }
    };

    let endpoint = params
        .endpoint
        .clone()
        .or_else(|| std::env::var("LOKAI_INFERENCE_ENDPOINT").ok())
        .filter(|e| !e.trim().is_empty())
        .unwrap_or_else(|| default_endpoint.to_string());

    guard.allow_hosted_endpoint(&endpoint).map_err(|e| {
        AppError::InvalidRequest(format!("failed to allow hosted endpoint '{endpoint}': {e}"))
    })?;

    if std::env::var("LOKAI_ALLOW_PRIVATE_HOSTED").is_ok() {
        guard.allow_private_hosted_endpoints(true);
    }

    let transport = Arc::new(EgressHostedTransport::new(
        guard.clone(),
        endpoint.clone(),
        cred_source,
    ));

    let config = build_hosted_config(&active_model, Some(params.num_ctx));
    let policy = HostedInferencePolicy::allow_up_to(DataClass::SensitiveSource);
    let scanner = crate::secret_scanner_factory::scanner_from_shared_store(store);

    let provider = Arc::new(
        HostedChatProvider::new(config, policy, scanner, transport)
            .map_err(|e| AppError::InvalidRequest(format!("initializing hosted provider: {e}")))?,
    );

    tracing::info!(
        provider = provider_id,
        model = %active_model,
        endpoint = %endpoint,
        "initialized hosted cloud inference provider"
    );

    Ok(Some(HostedSetup {
        model: active_model,
        endpoint,
        provider,
    }))
}

pub fn open_default_audit_store() -> Option<tetonic_memory::SharedStore> {
    let dirs = directories::ProjectDirs::from("", "", "lokai")?;
    let path = dirs.data_dir().join("lokai.db");
    tetonic_memory::SharedStore::open(&path, 5).ok()
}

fn resolve_models(
    explicit: Option<&str>,
    hard: Option<&str>,
    defaults: &tetonic_capacity::InferenceDefaults,
) -> (String, String) {
    let fast = explicit.unwrap_or(&defaults.model_fast).to_string();
    let hard = hard
        .or(explicit)
        .unwrap_or(&defaults.model_hard)
        .to_string();
    (fast, hard)
}

#[cfg(test)]
mod model_tests {
    use super::*;
    #[test]
    fn explicit_model_including_default_name_beats_saved_profile() {
        let mut defaults = tetonic_capacity::InferenceDefaults::fallback();
        defaults.model_fast = "saved-fast".into();
        defaults.model_hard = "saved-hard".into();
        assert_eq!(
            resolve_models(Some(tetonic_inference::DEFAULT_MODEL), None, &defaults),
            (
                tetonic_inference::DEFAULT_MODEL.into(),
                tetonic_inference::DEFAULT_MODEL.into()
            )
        );
        assert_eq!(
            resolve_models(None, None, &defaults),
            ("saved-fast".into(), "saved-hard".into())
        );
        assert_eq!(
            resolve_models(Some("chosen"), Some("chosen-hard"), &defaults),
            ("chosen".into(), "chosen-hard".into())
        );
    }

    fn test_params() -> CliBootstrapParams {
        CliBootstrapParams {
            workspace: ".".into(),
            ollama: "http://localhost:11434".into(),
            model: None,
            model_hard: None,
            model_tier: "fast".into(),
            allow_shell: false,
            explain: false,
            no_verify: false,
            verify: None,
            orchestrate: "single".into(),
            no_critic: false,
            llm_router: false,
            max_steps: 16,
            num_ctx: 8192,
            event_sink: Arc::new(crate::events::NoopEventSink),
            anthropic_key: None,
            openai_key: None,
            endpoint: None,
        }
    }

    #[test]
    fn detects_anthropic_model_with_key() {
        let mut params = test_params();
        params.anthropic_key = Some("test-anthropic-key".into());
        let guard = Arc::new(tetonic_egress::EgressGuard::new());
        let setup = resolve_hosted_setup(&params, "claude-3-5-sonnet-20241022", guard, &None)
            .unwrap()
            .expect("should resolve hosted setup");
        assert_eq!(setup.model, "claude-3-5-sonnet-20241022");
        assert_eq!(setup.endpoint, "https://api.anthropic.com/v1/messages");
    }

    #[test]
    fn detects_openai_model_with_key() {
        let mut params = test_params();
        params.openai_key = Some("test-openai-key".into());
        let guard = Arc::new(tetonic_egress::EgressGuard::new());
        let setup = resolve_hosted_setup(&params, "gpt-4o", guard, &None)
            .unwrap()
            .expect("should resolve hosted setup");
        assert_eq!(setup.model, "gpt-4o");
        assert_eq!(setup.endpoint, "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn detects_custom_endpoint() {
        let mut params = test_params();
        params.anthropic_key = Some("test-key".into());
        params.endpoint = Some("https://internal.corp/v1/messages".into());
        let guard = Arc::new(tetonic_egress::EgressGuard::new());
        let setup = resolve_hosted_setup(&params, "claude-3-5-sonnet-20241022", guard, &None)
            .unwrap()
            .expect("should resolve hosted setup");
        assert_eq!(setup.endpoint, "https://internal.corp/v1/messages");
    }

    #[test]
    fn returns_none_for_local_model_without_cloud_credentials() {
        let mut params = test_params();
        params.model = Some("qwen3.5:latest".into());
        let guard = Arc::new(tetonic_egress::EgressGuard::new());
        let setup = resolve_hosted_setup(&params, "qwen3.5:latest", guard, &None).unwrap();
        assert!(setup.is_none());
    }

    #[test]
    fn fails_when_claude_model_requested_without_key() {
        let params = test_params();
        let guard = Arc::new(tetonic_egress::EgressGuard::new());
        // Remove env vars if set
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("CLAUDE_API_KEY");
        let result = resolve_hosted_setup(&params, "claude-3-5-sonnet-20241022", guard, &None);
        assert!(result.is_err());
    }
}

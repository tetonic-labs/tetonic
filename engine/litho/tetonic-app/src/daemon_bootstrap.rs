//! Daemon application bootstrap — entry point for lokaid.
//!
//! Encapsulates workspace initialization, egress guard configuration,
//! compute plane setup, model discovery, and application kernel wiring.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use tetonic_capacity::{CapacityStatus, LOCAL_NODE_ID};
use tetonic_egress::EgressGuard;
use tetonic_enroll::KeyPair;
use tetonic_inference::{DispatchPlacementSink, InferenceProvider, OllamaProvider};
use tetonic_tools::{Tools, Workspace};

use crate::commands::InitializeCommand;
use crate::compute_plane::{build_compute_plane, ComputePlaneRequest};
use crate::errors::AppError;
use crate::events::ApplicationEventSink;
use crate::services::{DefaultInitializationService, InitializationService};
use crate::Application;

pub struct DaemonBootstrapParams {
    pub workspace_root: String,
    pub event_sink: Arc<dyn ApplicationEventSink>,
    pub placement_sink: Option<Arc<dyn DispatchPlacementSink>>,
}

pub struct DaemonBootstrapOutput {
    pub app: Arc<Application>,
    pub workspace_root: String,
    pub tools: Vec<String>,
    pub capacity_status: Option<CapacityStatus>,
    pub models: Vec<String>,
    pub model_fast: String,
    pub model_hard: String,
    pub tool_capable: bool,
    pub reachable: bool,
    pub shell_tool_enabled: bool,
    pub num_ctx: u32,
    pub ollama_base: String,
    pub fabric_pooled: bool,
    pub fabric_hint: Option<String>,
}

pub fn inference_port_from_base(base: &str) -> u16 {
    let trimmed = base.trim().trim_end_matches('/');
    trimmed
        .rsplit_once(':')
        .and_then(|(_, port)| port.parse().ok())
        .filter(|p| *p > 0)
        .unwrap_or(11434)
}

pub fn load_coordinator_key() -> Result<KeyPair> {
    let dirs = directories::ProjectDirs::from("", "", "lokai")
        .ok_or_else(|| anyhow::anyhow!("could not resolve lokai data directory"))?;
    let path = dirs.data_dir().join("coordinator.key");
    let bytes = std::fs::read(&path).map_err(|e| anyhow::anyhow!("read coordinator.key: {e}"))?;
    KeyPair::from_signing_bytes(&bytes).map_err(|e| anyhow::anyhow!(e))
}

fn fabric_hint_from_snapshot(
    node_count: usize,
    healthy_count: usize,
    effective_concurrency: usize,
    fabric_pooled: bool,
) -> Option<String> {
    if !fabric_pooled || node_count == 0 {
        None
    } else {
        Some(format!(
            "fabric: {healthy_count}/{node_count} nodes healthy, conc={effective_concurrency}"
        ))
    }
}

impl Application {
    pub async fn bootstrap_daemon(
        params: DaemonBootstrapParams,
    ) -> Result<DaemonBootstrapOutput, AppError> {
        let startup_timer =
            tetonic_telemetry::StageTimer::start(tetonic_telemetry::PerfStage::Startup);
        let guard = Arc::new(EgressGuard::new());
        let store = crate::cli_bootstrap::open_default_audit_store();

        if let Some(ref s) = store {
            let egress_guard = guard.clone();
            let _ = s.write_sync(move |db| {
                let _ =
                    crate::estate_enrollment::reload_enrollment_egress(db, egress_guard.as_ref());
                if let Ok(rows) = db.list_egress_allow_rules() {
                    for row in rows {
                        if let Ok(ip) = row.ip.parse() {
                            egress_guard.allow_node(row.label, ip, row.port);
                        }
                    }
                }
                if let Err(e) = db.mark_running_capacity_jobs_interrupted() {
                    tracing::warn!("capacity job cleanup: {e}");
                }
            });
        }

        let init: Arc<dyn InitializationService> = Arc::new(DefaultInitializationService::new());
        let init_cmd = InitializeCommand {
            workspace_root: params.workspace_root.clone(),
            rpc_token_provided: false,
            rpc_auth_disabled: true,
        };
        let init_result = init.initialize_workspace(init_cmd.clone()).await?;
        let bootstrap = init.bootstrap_runtime(&init_cmd, &store)?;
        let workspace_root = init_result.workspace_root;
        let policy = bootstrap.policy.clone();
        let runtime = bootstrap.runtime.clone();

        let ollama_base =
            std::env::var("LOKAI_OLLAMA").unwrap_or_else(|_| "http://localhost:11434".to_string());
        guard.configure_loopback_inference(inference_port_from_base(&ollama_base));

        let inference_defaults = bootstrap.inference_defaults.clone();
        let model = inference_defaults.model_fast.clone();
        let num_ctx = inference_defaults.num_ctx;
        let profile_model_hard = inference_defaults.model_hard.clone();
        let has_profile = inference_defaults.profile_id.is_some();

        let coordinator = load_coordinator_key().ok().map(Arc::new);
        let plane = build_compute_plane(ComputePlaneRequest {
            guard: guard.clone(),
            ollama_base: ollama_base.clone(),
            policy: policy.clone(),
            workspace_root: PathBuf::from(&workspace_root),
            artifact_store: runtime.artifact_store().clone(),
            store: store.clone(),
            coordinator: coordinator.clone(),
            placement_sink: params.placement_sink,
            previous_pooled: None,
        })
        .await;
        let fabric_pooled = plane.compute_registry.is_some();

        let local = Arc::new(OllamaProvider::new(&ollama_base, guard.clone()));
        let (reachable, models) = match local.list_models().await {
            Ok(models) => (true, models),
            Err(_) => {
                tracing::warn!("inference runtime not reachable");
                (false, Vec::new())
            }
        };

        if models.contains(&model) {
            let provider = local.clone();
            let warm_model = model.clone();
            tokio::spawn(async move {
                if let Err(error) = provider
                    .prewarm_with_context(&warm_model, None, Some(num_ctx))
                    .await
                {
                    tracing::warn!("startup model warm-up failed: {error}");
                }
            });
        }

        let mut default_tool_capable = !reachable;
        let mut tool_models: Vec<(String, f64)> = Vec::new();
        if reachable {
            for m in &models {
                if let Ok(info) = local.model_info(m).await {
                    let toolc = info.capabilities.is_empty()
                        || info.capabilities.iter().any(|c| c == "tools");
                    if m == &model {
                        default_tool_capable = toolc;
                    }
                    if toolc {
                        tool_models.push((m.clone(), info.param_b.unwrap_or(0.0)));
                    }
                }
            }
        }

        let hard_model = if has_profile {
            profile_model_hard
        } else {
            std::env::var("LOKAI_MODEL_HARD")
                .ok()
                .filter(|h| models.iter().any(|m| m == h))
                .or_else(|| {
                    tool_models
                        .iter()
                        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                        .map(|(name, _)| name.clone())
                })
                .unwrap_or_else(|| model.clone())
        };

        let snap = plane.provider.fabric_snapshot().await;
        let healthy = snap.nodes.iter().filter(|n| n.healthy).count();
        let fabric_hint = fabric_hint_from_snapshot(
            snap.nodes.len(),
            healthy,
            snap.effective_concurrency as usize,
            fabric_pooled,
        );

        let mut index_db: Option<PathBuf> = None;
        let mut open_index: Option<tetonic_index::Index> = None;
        if let Some(dirs) = directories::ProjectDirs::from("", "", "lokai") {
            let db_path = dirs.data_dir().join("index.db");
            if let Ok(idx) = tetonic_index::Index::open(&db_path) {
                let _ = idx.prune_missing_workspaces();
                index_db = Some(db_path);
                open_index = Some(idx);
            }
        }

        let app = Arc::new(Application::from_bootstrap(
            init,
            &bootstrap,
            store.clone(),
            params.event_sink,
            index_db.clone(),
            fabric_hint.clone(),
        ));
        app.set_default_model_inventory(models.clone());
        app.install_compute_plane(
            plane.provider.clone(),
            Some(plane.compute_broker.clone()),
            &plane.fabric_remotes,
        );
        app.attach_egress(guard.clone(), ollama_base.clone());
        app.set_fabric_plane(plane.pooled.clone(), plane.compute_registry.clone());

        if let Some(idx) = open_index {
            let broker = plane.compute_broker.clone();
            let now = chrono::Utc::now();
            let attempt_id =
                tetonic_domain::ids::AttemptId::new(format!("att_index_{}", uuid::Uuid::new_v4()));
            let index_req = tetonic_broker::ComputeRequest {
                run_id: tetonic_domain::RunId::new(format!("run_index_{}", uuid::Uuid::new_v4())),
                task_id: tetonic_domain::TaskId::new("task_index_workspace"),
                task_version: 1,
                attempt_id,
                job_kind: tetonic_fabric_protocol::JobKind::IndexShard,
                input_artifacts: vec![],
                input_digest: tetonic_domain::ContentDigest::new(format!(
                    "index:{}",
                    workspace_root
                )),
                workspace_version: None,
                data_class: tetonic_domain::DataClass::RepositorySource,
                placement_decision: tetonic_broker::PlacementDecisionReference {
                    decision_id: format!("plc_index_{}", uuid::Uuid::new_v4()),
                    issued_at: now,
                    expires_at: now + chrono::Duration::minutes(30),
                    policy_epoch: 0,
                    decision: tetonic_domain::TrustPlacementDecision::LocalOnly {
                        reason: tetonic_domain::PlacementReason::CapabilityUnavailable,
                    },
                },
                resource_request: tetonic_broker::profile_for(
                    &tetonic_fabric_protocol::JobKind::IndexShard,
                )
                .default_resources,
                deadline: tetonic_broker::DeadlinePolicy::default(),
                retry_policy: tetonic_broker::RetryPolicyReference::default(),
                verification_policy: tetonic_domain::VerificationPolicyReference {
                    policy_id: "structural".into(),
                },
                priority: tetonic_broker::ComputePriority::Background,
                trace_context: tetonic_domain::TraceContext::default(),
                speculative: false,
                project_id: None,
                target_worker_id: None,
                fallback_order: vec![],
                scheduler_decision_id: None,
            };
            let ws_root = workspace_root.clone();
            if let Err(e) = broker
                .with_index_shard_reservation(index_req, || {
                    idx.index_workspace(Path::new(&ws_root))
                        .map_err(|e| e.to_string())
                })
                .await
            {
                tracing::warn!("index build failed: {e}");
            }
        }

        let tools: Vec<String> = {
            let ws = Workspace::new(Path::new(&workspace_root))
                .map_err(|e| AppError::InvalidRequest(format!("workspace: {e}")))?;
            let mut t = Tools::new(ws, true);
            if let Some(db) = &index_db {
                t = t
                    .with_index(db.clone())
                    .with_code_index_open(Arc::new(tetonic_index::FilesystemCodeIndex));
            }
            if let Some(ref s) = store {
                let mem_path = s
                    .read_sync(|db| db.path().to_path_buf())
                    .unwrap_or_else(|_| PathBuf::from("lokai.db"));
                t = t.with_memory(&mem_path, None);
            }
            t.defs().into_iter().map(|d| d.name.to_string()).collect()
        };

        let shell_tool_enabled = tools.iter().any(|t| t == "run_shell");

        let capacity_status = if let Some(ref s) = store {
            let client: Arc<dyn tetonic_capacity::InferenceClient> =
                Arc::new(tetonic_capacity::OllamaInferenceClient::new(
                    &ollama_base,
                    Arc::new(OllamaProvider::new(&ollama_base, guard.clone())),
                ));
            let ver = client.version().await;
            let (status, _) =
                tetonic_capacity::capacity_status_for_store(s, LOCAL_NODE_ID, &client, ver).await;
            Some(status)
        } else {
            None
        };

        startup_timer.finish(true);
        Ok(DaemonBootstrapOutput {
            app,
            workspace_root,
            tools,
            capacity_status,
            models,
            model_fast: model,
            model_hard: hard_model,
            tool_capable: default_tool_capable,
            reachable,
            shell_tool_enabled,
            num_ctx,
            ollama_base,
            fabric_pooled,
            fabric_hint,
        })
    }
}

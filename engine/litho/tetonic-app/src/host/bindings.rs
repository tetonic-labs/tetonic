//! Shared runtime, policy and compute bindings owned by the application host.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use tetonic_core::{ExactTokenizer, HeuristicTokenizer, Tokenizer};
use tetonic_egress::EgressGuard;
use tetonic_inference::InferenceProvider;
use tetonic_memory::RecoverMutex;

use crate::errors::AppError;
use crate::events::ApplicationEventSink;
use crate::Application;

/// One installed binding: the provider and its broker cannot be replaced independently.
enum InstalledInference {
    Host {
        provider: Arc<dyn InferenceProvider>,
        broker: Option<Arc<tetonic_broker::DefaultComputeBroker>>,
    },
    Compute(Arc<tetonic_broker::BrokerInferenceProvider>),
}

type InferenceSnapshot = (
    Arc<dyn InferenceProvider>,
    Option<Arc<tetonic_broker::DefaultComputeBroker>>,
);

pub struct HostServices {
    pub runtime: Arc<tetonic_runtime::EngineRuntime>,
    pub store: Option<tetonic_memory::SharedStore>,
    pub index_db: Option<std::path::PathBuf>,
    pub workspace_root: String,
    pub num_ctx: AtomicU32,
    pub event_sink: Arc<dyn ApplicationEventSink>,
    pub policy: Arc<tetonic_policy::PolicyEngine>,
    guard: Mutex<Option<Arc<EgressGuard>>>,
    ollama_base: Mutex<String>,
    inference: Mutex<Option<InstalledInference>>,
    pooled: Mutex<Option<Arc<tetonic_inference::PooledProvider>>>,
    compute_registry: Mutex<Option<Arc<tetonic_inference::ComputeTargetRegistry>>>,
    tokenizer: Mutex<Arc<dyn Tokenizer>>,
}

impl HostServices {
    pub fn new(
        runtime: Arc<tetonic_runtime::EngineRuntime>,
        store: Option<tetonic_memory::SharedStore>,
        index_db: Option<std::path::PathBuf>,
        workspace_root: String,
        num_ctx: u32,
        event_sink: Arc<dyn ApplicationEventSink>,
        policy: Arc<tetonic_policy::PolicyEngine>,
    ) -> Arc<Self> {
        Arc::new(Self {
            runtime,
            store,
            index_db,
            workspace_root,
            num_ctx: AtomicU32::new(num_ctx),
            event_sink,
            policy,
            guard: Mutex::new(None),
            ollama_base: Mutex::new("http://127.0.0.1:11434".into()),
            inference: Mutex::new(None),
            pooled: Mutex::new(None),
            compute_registry: Mutex::new(None),
            tokenizer: Mutex::new(Arc::new(HeuristicTokenizer) as Arc<dyn Tokenizer>),
        })
    }

    pub fn set_fabric_plane(
        &self,
        pooled: Option<Arc<tetonic_inference::PooledProvider>>,
        compute_registry: Option<Arc<tetonic_inference::ComputeTargetRegistry>>,
    ) {
        *self.pooled.lock_recover() = pooled;
        *self.compute_registry.lock_recover() = compute_registry;
    }

    pub fn attach_egress(&self, guard: Arc<EgressGuard>, ollama_base: String) {
        *self.guard.lock_recover() = Some(guard);
        *self.ollama_base.lock_recover() = ollama_base;
    }

    pub fn guard(&self) -> Arc<EgressGuard> {
        self.guard
            .lock_recover()
            .clone()
            .unwrap_or_else(|| Arc::new(EgressGuard::new()))
    }

    pub fn ollama_base(&self) -> String {
        self.ollama_base.lock_recover().clone()
    }

    fn inference_snapshot(&self) -> Option<InferenceSnapshot> {
        match self.inference.lock_recover().as_ref()? {
            InstalledInference::Host { provider, broker } => {
                Some((provider.clone(), broker.clone()))
            }
            InstalledInference::Compute(provider) => {
                Some((provider.clone(), Some(provider.broker().clone())))
            }
        }
    }

    pub(crate) fn registered_provider(
        &self,
    ) -> Option<Arc<tetonic_broker::BrokerInferenceProvider>> {
        match self.inference.lock_recover().as_ref()? {
            InstalledInference::Compute(provider) => Some(provider.clone()),
            InstalledInference::Host { .. } => None,
        }
    }

    pub fn provider(&self) -> Option<Arc<dyn InferenceProvider>> {
        self.inference_snapshot().map(|(provider, _)| provider)
    }

    pub fn compute_broker(&self) -> Option<Arc<tetonic_broker::DefaultComputeBroker>> {
        self.inference_snapshot().and_then(|(_, broker)| broker)
    }
}

impl Application {
    pub fn bind_inference(
        &self,
        provider: Arc<dyn InferenceProvider>,
        compute_broker: Option<Arc<tetonic_broker::DefaultComputeBroker>>,
    ) {
        *self.host.inference.lock_recover() = Some(InstalledInference::Host {
            provider,
            broker: compute_broker,
        });
    }

    /// Install the complete product compute state, including management handles.
    pub fn install_compute_services(&self, plane: &crate::ComputePlane) {
        self.set_fabric_plane(plane.pooled.clone(), plane.compute_registry.clone());
        self.attach_compute_lifecycle(Some(plane.provider.broker()), &plane.fabric_remotes);
        *self.host.inference.lock_recover() =
            Some(InstalledInference::Compute(plane.provider.clone()));
    }

    /// Bind the inference plane and attach the private supervisor (021).
    pub fn install_compute_plane(
        &self,
        provider: Arc<dyn InferenceProvider>,
        compute_broker: Option<Arc<tetonic_broker::DefaultComputeBroker>>,
        fabric_remotes: &[Arc<tetonic_fabric_client::RemoteNodeProvider>],
    ) {
        self.bind_inference(provider, compute_broker.clone());
        self.attach_compute_lifecycle(compute_broker.as_ref(), fabric_remotes);
    }

    pub(crate) fn attach_compute_lifecycle(
        &self,
        compute_broker: Option<&Arc<tetonic_broker::DefaultComputeBroker>>,
        fabric_remotes: &[Arc<tetonic_fabric_client::RemoteNodeProvider>],
    ) {
        let bridge = crate::fabric_run_bridge::SupervisorRunBridge::arc(self.supervisor.clone());
        for remote in fabric_remotes {
            remote.set_run_bridge(bridge.clone());
        }
        if let Some(broker) = compute_broker {
            broker.set_supervisor(self.supervisor.clone());
        }
    }

    pub fn bind_tokenizer(&self, tokenizer: Arc<dyn Tokenizer>) {
        *self.host.tokenizer.lock_recover() = tokenizer;
    }

    pub fn bind_exact_tokenizer(&self, path: &str) -> Result<(), AppError> {
        let tok = ExactTokenizer::from_file(std::path::Path::new(path))
            .map_err(|e| AppError::InvalidRequest(format!("tokenizer load failed: {e}")))?;
        self.bind_tokenizer(Arc::new(tok));
        Ok(())
    }

    pub fn bind_heuristic_tokenizer(&self) {
        self.bind_tokenizer(Arc::new(HeuristicTokenizer));
    }

    pub fn attach_egress(&self, guard: Arc<EgressGuard>, ollama_base: String) {
        self.host.attach_egress(guard, ollama_base);
    }

    pub fn egress_guard(&self) -> Arc<EgressGuard> {
        self.host.guard()
    }

    pub fn inference_provider(&self) -> Option<Arc<dyn InferenceProvider>> {
        self.host.provider()
    }

    pub fn compute_broker(&self) -> Option<Arc<tetonic_broker::DefaultComputeBroker>> {
        self.host.compute_broker()
    }

    pub fn ollama_base(&self) -> String {
        self.host.ollama_base()
    }

    pub fn set_fabric_plane(
        &self,
        pooled: Option<Arc<tetonic_inference::PooledProvider>>,
        compute_registry: Option<Arc<tetonic_inference::ComputeTargetRegistry>>,
    ) {
        self.host.set_fabric_plane(pooled, compute_registry);
    }

    pub fn egress_allow_rules(&self) -> Vec<tetonic_egress::AllowRule> {
        self.host.guard().allow_rules()
    }

    pub fn policy_epoch(&self) -> u64 {
        self.host
            .compute_registry
            .lock_recover()
            .as_ref()
            .map(|r| r.current_epoch())
            .unwrap_or(0)
    }

    pub fn bump_policy_epoch(&self) {
        if let Some(ref reg) = *self.host.compute_registry.lock_recover() {
            reg.policy_epoch().fetch_add(1, Ordering::Relaxed);
        }
        if let Some(ref pooled) = *self.host.pooled.lock_recover() {
            pooled.policy_epoch().fetch_add(1, Ordering::Relaxed);
            pooled.on_policy_invalidation();
        }
    }

    pub fn policy_mode(&self) -> String {
        self.host.policy.mode().as_str().to_string()
    }

    pub async fn fabric_status(&self) -> Result<serde_json::Value, AppError> {
        if let Some(ref pooled) = *self.host.pooled.lock_recover() {
            pooled.invalidate_snapshot_cache();
        }
        let provider = self
            .host
            .provider()
            .ok_or_else(|| AppError::InternalViolation("no provider installed".into()))?;
        let mut snap = provider.fabric_snapshot().await;
        if let Some(ref store) = self.host.store {
            let cap = store
                .read_sync(tetonic_capacity::status_for_node)
                .unwrap_or_default();
            tetonic_capacity::enrich_local_node_capacity(&mut snap, &cap);
        }
        let mut value =
            serde_json::to_value(snap).map_err(|e| AppError::InternalViolation(e.to_string()))?;
        if let Some(broker) = self.host.compute_broker() {
            let metrics = broker.scheduler_metrics();
            if let (Some(obj), Some(mae)) = (
                value.as_object_mut(),
                metrics.mean_abs_prediction_error_ms(),
            ) {
                obj.insert("scheduler_prediction_mae_ms".into(), serde_json::json!(mae));
                obj.insert(
                    "scheduler_prediction_samples".into(),
                    serde_json::json!(metrics.prediction_sample_count()),
                );
            }
        }
        Ok(value)
    }

    pub async fn set_fabric_worker_trust(
        &self,
        worker_id: &str,
        trust_str: &str,
    ) -> Result<crate::commands::SetWorkerTrustResult, AppError> {
        let trust = tetonic_domain::WorkerTrust::parse(trust_str).ok_or_else(|| {
            AppError::InvalidRequest(format!("unknown worker trust tier '{trust_str}'"))
        })?;
        let reg_opt = self.host.compute_registry.lock_recover().clone();
        let Some(reg) = reg_opt else {
            return Err(AppError::InvalidRequest(
                "fabric pooling not active — no compute registry".into(),
            ));
        };
        if !reg.set_worker_trust(worker_id, trust) {
            return Err(AppError::InvalidRequest(format!(
                "worker '{worker_id}' not enrolled"
            )));
        }
        let epoch = reg.current_epoch();
        if let Some(ref pooled) = *self.host.pooled.lock_recover() {
            pooled.on_trust_change(worker_id, trust);
        }
        if let Some(ref store) = self.host.store {
            let wid = worker_id.to_string();
            let trust_s = trust.as_str().to_string();
            store
                .write(move |db| {
                    db.set_worker_trust(&wid, &trust_s, epoch, "rpc")
                        .map_err(|e| anyhow::anyhow!("{e}"))
                })
                .await
                .map_err(|e| AppError::PersistenceFailed(e.to_string()))?
                .map_err(|e| AppError::PersistenceFailed(e.to_string()))?;
        }
        Ok(crate::commands::SetWorkerTrustResult {
            worker_id: worker_id.to_string(),
            trust: trust.as_str().to_string(),
            policy_epoch: epoch,
        })
    }

    pub fn get_fabric_worker_trust(
        &self,
        worker_id: &str,
    ) -> Result<crate::commands::GetWorkerTrustResult, AppError> {
        let reg_opt = self.host.compute_registry.lock_recover().clone();
        let trust = reg_opt
            .as_ref()
            .and_then(|r| r.trust_for_worker(worker_id))
            .or_else(|| {
                self.host.store.as_ref().and_then(|store| {
                    store
                        .read_sync(|db| db.worker_trust(worker_id))
                        .ok()
                        .and_then(|res| res.ok())
                        .flatten()
                        .and_then(|s| tetonic_domain::WorkerTrust::parse(&s))
                })
            })
            .ok_or_else(|| AppError::InvalidRequest(format!("worker '{worker_id}' not found")))?;
        let policy_epoch = reg_opt.as_ref().map(|r| r.current_epoch()).unwrap_or(0);
        let audit = self
            .host
            .store
            .as_ref()
            .and_then(|store| {
                store
                    .read_sync(|db| db.list_worker_trust_audit(worker_id))
                    .ok()
                    .and_then(|res| res.ok())
            })
            .unwrap_or_default()
            .into_iter()
            .map(|row| crate::commands::WorkerTrustAuditEntry {
                trust: row.trust,
                policy_epoch: row.policy_epoch,
                recorded_at: row.recorded_at,
                source: row.source,
            })
            .collect::<Vec<_>>();
        Ok(crate::commands::GetWorkerTrustResult {
            worker_id: worker_id.to_string(),
            trust: trust.as_str().to_string(),
            policy_epoch,
            audit,
        })
    }
}

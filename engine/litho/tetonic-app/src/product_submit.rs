//! Product submit/await: portal-as-transport door (PORTAL-01).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use tetonic_core::{Conversation, ExactTokenizer, HeuristicTokenizer, Tokenizer};
use tetonic_egress::EgressGuard;
use tetonic_inference::InferenceProvider;
use tetonic_memory::RecoverMutex;
use tetonic_orchestrator::SpawnLimits;
use tokio::sync::oneshot;

use crate::commands::{RunTurnCommand, SpawnAgentCommand, TurnFinish};
use crate::errors::AppError;
use crate::events::ApplicationEventSink;
use crate::session_live::{admit_chat_turn, LiveSession, TurnAdmitDecision, TurnAdmitError};
use crate::store_audit::product_audit_factory;
use crate::turn_execution::{outbound_event_scanner, TurnExecutionHost};
use crate::Application;

pub struct TurnBind {
    pub(crate) inference_profiles: crate::inference_selection::InferenceProfiles,
    pub runtime: Arc<tetonic_runtime::EngineRuntime>,
    pub store: Option<tetonic_memory::SharedStore>,
    pub index_db: Option<std::path::PathBuf>,
    pub workspace_root: String,
    pub num_ctx: AtomicU32,
    pub event_sink: Arc<dyn ApplicationEventSink>,
    pub policy: Arc<tetonic_policy::PolicyEngine>,
    guard: Mutex<Option<Arc<EgressGuard>>>,
    ollama_base: Mutex<String>,
    provider: Mutex<Option<Arc<dyn InferenceProvider>>>,
    compute_broker: Mutex<Option<Arc<tetonic_broker::DefaultComputeBroker>>>,
    pooled: Mutex<Option<Arc<tetonic_inference::PooledProvider>>>,
    compute_registry: Mutex<Option<Arc<tetonic_inference::ComputeTargetRegistry>>>,
    tokenizer: Mutex<Arc<dyn Tokenizer>>,
    joins: Mutex<HashMap<String, oneshot::Sender<TurnFinish>>>,
}

impl TurnBind {
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
            inference_profiles: Default::default(),
            runtime,
            store,
            index_db,
            workspace_root,
            num_ctx: AtomicU32::new(num_ctx),
            event_sink,
            policy,
            guard: Mutex::new(None),
            ollama_base: Mutex::new("http://127.0.0.1:11434".into()),
            provider: Mutex::new(None),
            compute_broker: Mutex::new(None),
            pooled: Mutex::new(None),
            compute_registry: Mutex::new(None),
            tokenizer: Mutex::new(Arc::new(HeuristicTokenizer) as Arc<dyn Tokenizer>),
            joins: Mutex::new(HashMap::new()),
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

    pub fn provider(&self) -> Option<Arc<dyn InferenceProvider>> {
        self.provider.lock_recover().clone()
    }

    pub fn compute_broker(&self) -> Option<Arc<tetonic_broker::DefaultComputeBroker>> {
        self.compute_broker.lock_recover().clone()
    }
}

pub(crate) struct OwnedTurn {
    pub(crate) live: Arc<LiveSession>,
    pub(crate) convo: Option<Conversation>,
    pub(crate) join: Option<oneshot::Sender<TurnFinish>>,
    pub(crate) delivery: Arc<crate::turn_delivery::TurnDelivery>,
}

impl Drop for OwnedTurn {
    fn drop(&mut self) {
        let Some(c) = self.convo.take() else {
            return; // Normal completion already released the lease and published its outcome.
        };
        self.live.restore_conversation(c);
        self.live.end_turn();
        self.delivery.finish(
            self.live.cancel.load(Ordering::Relaxed),
            Some("turn execution dropped".into()),
        );
        if let Some(tx) = self.join.take() {
            let canceled = self.live.cancel.load(Ordering::Relaxed);
            let _ = tx.send(TurnFinish {
                ok: false,
                canceled,
                error: Some("dropped".into()),
            });
        }
    }
}

fn admit_err(err: TurnAdmitError) -> AppError {
    match err {
        TurnAdmitError::Draining => AppError::InvalidRequest("daemon is shutting down".into()),
        TurnAdmitError::CapacityBusy => AppError::InvalidRequest(
            "capacity optimize in progress — interactive chat blocked".into(),
        ),
        TurnAdmitError::TurnInFlight => {
            AppError::InvalidRequest("a turn is already running for this session".into())
        }
        TurnAdmitError::CapacityGate { detail, .. } => AppError::InvalidRequest(detail),
    }
}

impl Application {
    pub fn bind_inference(
        &self,
        provider: Arc<dyn InferenceProvider>,
        compute_broker: Option<Arc<tetonic_broker::DefaultComputeBroker>>,
    ) {
        *self.turn.provider.lock_recover() = Some(provider);
        *self.turn.compute_broker.lock_recover() = compute_broker;
    }

    /// Install the complete product compute state, including management handles.
    pub fn install_compute_services(&self, plane: &crate::ComputePlane) {
        self.set_fabric_plane(plane.pooled.clone(), plane.compute_registry.clone());
        self.install_compute_plane(
            plane.provider.clone(),
            Some(plane.compute_broker.clone()),
            &plane.fabric_remotes,
        );
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
        *self.turn.tokenizer.lock_recover() = tokenizer;
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
        self.turn.attach_egress(guard, ollama_base);
    }

    pub fn egress_guard(&self) -> Arc<EgressGuard> {
        self.turn.guard()
    }

    pub fn inference_provider(&self) -> Option<Arc<dyn InferenceProvider>> {
        self.turn.provider()
    }

    pub fn compute_broker(&self) -> Option<Arc<tetonic_broker::DefaultComputeBroker>> {
        self.turn.compute_broker()
    }

    pub fn ollama_base(&self) -> String {
        self.turn.ollama_base()
    }

    pub fn set_fabric_plane(
        &self,
        pooled: Option<Arc<tetonic_inference::PooledProvider>>,
        compute_registry: Option<Arc<tetonic_inference::ComputeTargetRegistry>>,
    ) {
        self.turn.set_fabric_plane(pooled, compute_registry);
    }

    pub fn egress_allow_rules(&self) -> Vec<tetonic_egress::AllowRule> {
        self.turn.guard().allow_rules()
    }

    pub fn policy_epoch(&self) -> u64 {
        self.turn
            .compute_registry
            .lock_recover()
            .as_ref()
            .map(|r| r.current_epoch())
            .unwrap_or(0)
    }

    pub fn bump_policy_epoch(&self) {
        if let Some(ref reg) = *self.turn.compute_registry.lock_recover() {
            reg.policy_epoch().fetch_add(1, Ordering::Relaxed);
        }
        if let Some(ref pooled) = *self.turn.pooled.lock_recover() {
            pooled.policy_epoch().fetch_add(1, Ordering::Relaxed);
            pooled.on_policy_invalidation();
        }
    }

    pub fn policy_mode(&self) -> String {
        self.turn.policy.mode().as_str().to_string()
    }

    pub async fn fabric_status(&self) -> Result<serde_json::Value, AppError> {
        if let Some(ref pooled) = *self.turn.pooled.lock_recover() {
            pooled.invalidate_snapshot_cache();
        }
        let provider = self
            .turn
            .provider()
            .ok_or_else(|| AppError::InternalViolation("no provider installed".into()))?;
        let mut snap = provider.fabric_snapshot().await;
        if let Some(ref store) = self.turn.store {
            let cap = store
                .read_sync(tetonic_capacity::status_for_node)
                .unwrap_or_default();
            tetonic_capacity::enrich_local_node_capacity(&mut snap, &cap);
        }
        let mut value =
            serde_json::to_value(snap).map_err(|e| AppError::InternalViolation(e.to_string()))?;
        if let Some(broker) = self.turn.compute_broker() {
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
        let reg_opt = self.turn.compute_registry.lock_recover().clone();
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
        if let Some(ref pooled) = *self.turn.pooled.lock_recover() {
            pooled.on_trust_change(worker_id, trust);
        }
        if let Some(ref store) = self.turn.store {
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
        let reg_opt = self.turn.compute_registry.lock_recover().clone();
        let trust = reg_opt
            .as_ref()
            .and_then(|r| r.trust_for_worker(worker_id))
            .or_else(|| {
                self.turn.store.as_ref().and_then(|store| {
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
            .turn
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

    pub fn cancel_session_broker_jobs(&self, session_id: &str) {
        let selected = self
            .session_inference(session_id)
            .ok()
            .and_then(|s| self.turn.inference_profiles.get(&s.profile));
        let broker = selected
            .and_then(|p| p.broker)
            .or_else(|| self.turn.compute_broker());
        if let Some(broker) = broker {
            broker.cancel_session_jobs(session_id);
            if let Ok(live) = self.sessions.live(session_id) {
                if let Some(run_id) = live.current_run_id() {
                    broker.cancel_run_jobs(&run_id);
                }
            }
        } else if let Some(ref pooled) = *self.turn.pooled.lock_recover() {
            pooled.cancel_session_jobs(session_id);
        }
    }

    pub fn reclassify_session_live(&self, session_id: &str, data_class: &str) {
        if let Some(class) = tetonic_policy::parse_data_class(data_class) {
            if let Ok(live) = self.sessions.live(session_id) {
                live.set_plan_data_class(class);
            }
        }
        self.bump_policy_epoch();
    }

    pub fn reload_inference_services(&self) -> Option<tetonic_capacity::InferenceDefaults> {
        let store = self.turn.store.as_ref()?;
        store
            .read_sync(|db| {
                tetonic_capacity::load_inference_defaults(db, tetonic_capacity::LOCAL_NODE_ID)
            })
            .ok()
    }

    pub async fn daemon_capacity_status(
        &self,
    ) -> Result<tetonic_capacity::CapacityStatus, AppError> {
        let guard = self.turn.guard();
        let base = self.turn.ollama_base();
        let provider = Arc::new(tetonic_inference::OllamaProvider::new(&base, guard));
        let client: Arc<dyn tetonic_capacity::InferenceClient> = Arc::new(
            tetonic_capacity::OllamaInferenceClient::new(&base, provider),
        );
        let ver = client.version().await;
        let result = self
            .capacity
            .get_capacity_status(crate::commands::CapacityStatusCommand {
                client,
                node_id: tetonic_capacity::LOCAL_NODE_ID.to_string(),
                ollama_version: ver,
            })
            .await?;
        Ok(result.status)
    }

    pub async fn daemon_capacity_doctor(
        &self,
    ) -> Result<
        (
            tetonic_capacity::CapacityStatus,
            tetonic_capacity::CapacityDiagnosis,
        ),
        AppError,
    > {
        let guard = self.turn.guard();
        let base = self.turn.ollama_base();
        let provider = Arc::new(tetonic_inference::OllamaProvider::new(&base, guard));
        let client: Arc<dyn tetonic_capacity::InferenceClient> = Arc::new(
            tetonic_capacity::OllamaInferenceClient::new(&base, provider),
        );
        let ver = client.version().await;
        let result = self
            .capacity
            .get_capacity_doctor(crate::commands::CapacityDoctorCommand {
                client,
                node_id: tetonic_capacity::LOCAL_NODE_ID.to_string(),
                ollama_version: ver,
            })
            .await?;
        Ok((result.status, result.diagnosis))
    }

    pub async fn daemon_run_capacity_optimize(
        &self,
        depth: String,
        auto_apply: bool,
        prebegin: Option<crate::commands::BeginOptimizeResultPayload>,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<tetonic_capacity::OptimizeOutcome, AppError> {
        let guard = self.turn.guard();
        let base = self.turn.ollama_base();
        let provider = Arc::new(tetonic_inference::OllamaProvider::new(&base, guard));
        let client: Arc<dyn tetonic_capacity::InferenceClient> = Arc::new(
            tetonic_capacity::OllamaInferenceClient::new(&base, provider),
        );
        self.capacity
            .run_optimize(
                crate::commands::RunOptimizeCommand {
                    sessions_busy: false,
                    capacity_busy: false,
                    depth,
                    auto_apply,
                    client,
                    prebegin,
                },
                cancel,
            )
            .await
    }

    pub fn bind_num_ctx(&self, num_ctx: u32) {
        self.turn.num_ctx.store(num_ctx, Ordering::Relaxed);
    }

    pub fn arm_turn_join(&self, session_id: &str) -> oneshot::Receiver<TurnFinish> {
        let (tx, rx) = oneshot::channel();
        self.turn
            .joins
            .lock_recover()
            .insert(session_id.to_string(), tx);
        rx
    }

    pub(crate) fn build_session_host(
        &self,
        session_id: &str,
    ) -> Result<TurnExecutionHost, AppError> {
        let live = self.sessions.live(session_id)?;
        let selection = live.inference_selection();
        let selected_profile = if selection.profile == "default" {
            None
        } else {
            Some(
                self.turn
                    .inference_profiles
                    .get(&selection.profile)
                    .ok_or_else(|| {
                        AppError::InvalidRequest("selected inference profile unavailable".into())
                    })?,
            )
        };
        let provider = self
            .turn
            .provider
            .lock_recover()
            .clone()
            .ok_or_else(|| AppError::InvalidRequest("inference provider not bound".into()))?;
        let (scanner, sink) = outbound_event_scanner(&self.turn.store);
        Ok(TurnExecutionHost {
            live_session: Some(Arc::downgrade(&live)),
            runtime: self.turn.runtime.clone(),
            provider: selected_profile
                .as_ref()
                .map(|p| p.provider.clone() as Arc<dyn InferenceProvider>)
                .unwrap_or(provider),
            store: self.turn.store.clone(),
            index_db: self.turn.index_db.clone(),
            workspace_root: self.turn.workspace_root.clone(),
            tool_workspace: live.tool_workspace.clone(),
            model_fast: selection.model_fast,
            model_hard: selection.model_hard,
            num_ctx: selected_profile
                .as_ref()
                .map(|p| p.num_ctx)
                .unwrap_or_else(|| self.turn.num_ctx.load(Ordering::Relaxed)),
            session_max_steps: live.session_max_steps,
            explicit_hard_tier: live.explicit_hard_tier,
            orchestration: live.orchestration,
            critic_enabled: live.critic_enabled,
            llm_router: live.llm_router,
            plan: live.plan(),
            session_id: session_id.to_string(),
            allow_shell: live.allow_shell,
            force_explain: live.force_explain,
            spawn_limits: SpawnLimits::from_env(),
            tokenizer: if selection.revision > 0 {
                Arc::new(HeuristicTokenizer) as Arc<dyn Tokenizer>
            } else {
                self.turn.tokenizer.lock_recover().clone()
            },
            cancel: live.cancel.clone(),
            approvals: self.approvals.clone(),
            audit_factory: product_audit_factory(&self.turn.store),
            spawn_track: Some(live.spawn_track.clone()),
            turn_spawn_count: Arc::new(AtomicU32::new(live.turn_spawn_count())),
            compute_broker: selected_profile
                .as_ref()
                .and_then(|p| p.broker.clone())
                .or_else(|| self.turn.compute_broker.lock_recover().clone()),
            secret_scanner: Some(scanner),
            redaction_sink: Some(sink),
            auto_grant_approvals: live.auto_grant_approvals,
        })
    }

    pub fn submit_chat_turn(&self, mut cmd: RunTurnCommand) -> Result<(), AppError> {
        let live = self.sessions.live(&cmd.session_id)?;
        if cmd.verify_cmd.is_none() {
            cmd.verify_cmd = live.plan().verify_cmd.clone();
        }
        if cmd.llm_router.is_none() {
            cmd.llm_router = Some(live.llm_router);
        }
        let cap = self
            .capacity
            .snapshot_admission_status(tetonic_capacity::LOCAL_NODE_ID);
        match admit_chat_turn(false, false, &live, cap.as_ref()) {
            TurnAdmitDecision::Admit { .. } => {}
            TurnAdmitDecision::Reject(err) => return Err(admit_err(err)),
        }
        let convo = match live.take_conversation() {
            Ok(c) => c,
            Err(e) => {
                live.end_turn();
                return Err(e);
            }
        };
        let host = match self.build_session_host(&cmd.session_id) {
            Ok(h) => h,
            Err(e) => {
                live.restore_conversation(convo);
                live.end_turn();
                return Err(e);
            }
        };
        let join = self.turn.joins.lock_recover().remove(&cmd.session_id);
        let owned = OwnedTurn {
            live: live.clone(),
            convo: Some(convo),
            join,
            delivery: Arc::new(crate::turn_delivery::TurnDelivery::new(
                cmd.session_id.clone(),
                self.turn.event_sink.clone(),
            )),
        };
        self.run_manager.dispatch_chat_turn(cmd, host, owned)?;
        Ok(())
    }

    pub fn submit_spawn(&self, cmd: SpawnAgentCommand) -> Result<(), AppError> {
        crate::turn_execution::parse_spawn_role(&cmd.role)?;
        let live = self.sessions.live(&cmd.session_id)?;
        let cap = self
            .capacity
            .snapshot_admission_status(tetonic_capacity::LOCAL_NODE_ID);
        match admit_chat_turn(false, false, &live, cap.as_ref()) {
            TurnAdmitDecision::Admit { .. } => {}
            TurnAdmitDecision::Reject(err) => return Err(admit_err(err)),
        }
        let convo = match live.take_conversation() {
            Ok(c) => c,
            Err(e) => {
                live.end_turn();
                return Err(e);
            }
        };
        let host = match self.build_session_host(&cmd.session_id) {
            Ok(h) => h,
            Err(e) => {
                live.restore_conversation(convo);
                live.end_turn();
                return Err(e);
            }
        };
        let join = self.turn.joins.lock_recover().remove(&cmd.session_id);
        let owned = OwnedTurn {
            live: live.clone(),
            convo: Some(convo),
            join,
            delivery: Arc::new(crate::turn_delivery::TurnDelivery::new(
                cmd.session_id.clone(),
                self.turn.event_sink.clone(),
            )),
        };
        self.run_manager.dispatch_spawn(cmd, host, owned)?;
        Ok(())
    }
}

pub(crate) fn complete_dispatched_turn(
    mut owned: OwnedTurn,
    host: &TurnExecutionHost,
    result: Result<(), AppError>,
    spawn_serial: u32,
) {
    owned
        .live
        .spawn_serial
        .store(spawn_serial, Ordering::Relaxed);
    owned
        .live
        .store_turn_spawn_count(host.turn_spawn_count.load(Ordering::Relaxed));
    let canceled = owned.live.cancel.load(Ordering::Relaxed);
    let finish = match &result {
        Ok(()) => TurnFinish {
            ok: !canceled,
            canceled,
            error: None,
        },
        Err(e) => TurnFinish {
            ok: false,
            canceled,
            error: Some(e.to_string()),
        },
    };
    if let Some(c) = owned.convo.take() {
        owned.live.restore_conversation(c);
    }
    owned.live.end_turn();
    owned.delivery.finish(canceled, finish.error.clone());
    if let Some(tx) = owned.join.take() {
        let _ = tx.send(finish);
    }
}

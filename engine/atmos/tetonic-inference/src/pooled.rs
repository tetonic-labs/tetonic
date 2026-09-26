//! Homelab `PooledProvider`: local Ollama + enrolled remote workers (N0.4).

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::Utc;
use futures_util::future::join_all;
use tetonic_fabric_protocol::{CapabilityRegistry, ModelSelection};

use crate::attempt::JobFinishGuard;
use crate::dispatch::{
    plan_remote_attempt, redact_messages_for_failover, stamp_request_classification,
};
use crate::placement::{DispatchPlacementReport, DispatchPlacementSink};
use crate::placement_engine::{evaluate_placement, placement_request_from_chat};
use crate::worker_eligibility::placement_reason_is_local_only;
use tetonic_domain::placement::{PlacementDecision, ProjectPlacementPolicy};
use tetonic_policy::trust_permits_data_class;

use crate::{
    new_fabric_job_id, ActiveJobRegistry, ChatRequest, ChatResponse, DispatchDecision,
    DispatchGuard, FabricCallMeta, FabricNodeProvider, FabricSnapshot, InferenceError,
    InferenceProvider, OllamaProvider, TokenSink, LOCAL_NODE_ID,
};

struct SnapshotCache {
    snap: FabricSnapshot,
    fetched_at: Instant,
}

#[cfg(test)]
#[path = "snapshot_tests.rs"]
mod snapshot_tests;

pub type WorkerTrustResolver =
    dyn Fn(&str) -> Option<(tetonic_domain::WorkerTrust, u64)> + Send + Sync;

type WorkerLossHook = Arc<dyn Fn(&str) + Send + Sync>;

/// AJR hop identity. Uses `hop_*` only. Never falls back to agent `attempt_id` / `run_id`.
fn hop_identity_for_ajr(fabric: Option<&FabricCallMeta>) -> (Option<String>, Option<String>) {
    let Some(f) = fabric else {
        return (None, None);
    };
    (
        f.hop_attempt_id.clone().filter(|id| !id.is_empty()),
        f.hop_run_id.clone().filter(|id| !id.is_empty()),
    )
}

/// Local loopback + remote enrolled workers with placement and failover.
pub struct PooledProvider {
    local: Arc<OllamaProvider>,
    remotes: Vec<Arc<dyn FabricNodeProvider>>,
    turn_affinity: RwLock<Option<String>>,
    last_turn_id: RwLock<Option<String>>,
    last_session_id: RwLock<Option<String>>,
    last_run_id: RwLock<Option<String>>,
    last_context_id: RwLock<Option<String>>,
    dispatch_guard: Option<Arc<dyn DispatchGuard>>,
    placement_sink: Option<Arc<dyn DispatchPlacementSink>>,
    snapshot_cache: RwLock<Option<SnapshotCache>>,
    snapshot_refresh: tokio::sync::Mutex<()>,
    snapshot_epoch: AtomicU64,
    /// Incremented on each cache miss refresh (tests / diagnostics, AR1-2).
    snapshot_generation: AtomicU32,
    /// Monotonic policy epoch sent on remote fabric jobs (SEC-008).
    policy_epoch: Arc<AtomicU64>,
    job_registry: Arc<ActiveJobRegistry>,
    /// Shared coordinator capability cache (M5-2); filters placement when wired.
    capability_registry: Option<Arc<RwLock<CapabilityRegistry>>>,
    /// Reads coordinator-owned trust at the last safe point before remote send.
    worker_trust_resolver: Option<Arc<WorkerTrustResolver>>,
    /// Optional ComputeBroker reservation release on worker loss (M6-1).
    worker_loss_hook: RwLock<Option<WorkerLossHook>>,
}

impl PooledProvider {
    pub fn new(local: Arc<OllamaProvider>, remotes: Vec<Arc<dyn FabricNodeProvider>>) -> Self {
        Self::new_with_policy_epoch(local, remotes, Arc::new(AtomicU64::new(0)))
    }

    pub fn new_with_registry(
        local: Arc<OllamaProvider>,
        remotes: Vec<Arc<dyn FabricNodeProvider>>,
        policy_epoch: Arc<AtomicU64>,
        job_registry: Arc<ActiveJobRegistry>,
    ) -> Self {
        Self {
            local,
            remotes,
            turn_affinity: RwLock::new(None),
            last_turn_id: RwLock::new(None),
            last_session_id: RwLock::new(None),
            last_run_id: RwLock::new(None),
            last_context_id: RwLock::new(None),
            dispatch_guard: None,
            placement_sink: None,
            snapshot_cache: RwLock::new(None),
            snapshot_refresh: tokio::sync::Mutex::new(()),
            snapshot_epoch: AtomicU64::new(0),
            snapshot_generation: AtomicU32::new(0),
            policy_epoch,
            job_registry,
            capability_registry: None,
            worker_trust_resolver: None,
            worker_loss_hook: RwLock::new(None),
        }
    }

    pub fn with_capability_registry(mut self, registry: Arc<RwLock<CapabilityRegistry>>) -> Self {
        self.capability_registry = Some(registry);
        self
    }

    pub fn capability_registry(&self) -> Option<Arc<RwLock<CapabilityRegistry>>> {
        self.capability_registry.clone()
    }

    pub fn with_worker_trust_resolver(mut self, resolver: Arc<WorkerTrustResolver>) -> Self {
        self.worker_trust_resolver = Some(resolver);
        self
    }

    pub fn job_registry(&self) -> Arc<ActiveJobRegistry> {
        self.job_registry.clone()
    }

    pub fn new_with_policy_epoch(
        local: Arc<OllamaProvider>,
        remotes: Vec<Arc<dyn FabricNodeProvider>>,
        policy_epoch: Arc<AtomicU64>,
    ) -> Self {
        Self {
            local,
            remotes,
            turn_affinity: RwLock::new(None),
            last_turn_id: RwLock::new(None),
            last_session_id: RwLock::new(None),
            last_run_id: RwLock::new(None),
            last_context_id: RwLock::new(None),
            dispatch_guard: None,
            placement_sink: None,
            snapshot_cache: RwLock::new(None),
            snapshot_refresh: tokio::sync::Mutex::new(()),
            snapshot_epoch: AtomicU64::new(0),
            snapshot_generation: AtomicU32::new(0),
            policy_epoch,
            job_registry: Arc::new(ActiveJobRegistry::new()),
            capability_registry: None,
            worker_trust_resolver: None,
            worker_loss_hook: RwLock::new(None),
        }
    }

    pub fn policy_epoch(&self) -> Arc<AtomicU64> {
        self.policy_epoch.clone()
    }

    pub fn bump_policy_epoch(&self, epoch: u64) {
        self.policy_epoch.fetch_max(epoch, Ordering::Relaxed);
    }

    /// Revocation or worker removal: bump epoch, revoke remotes, drop cached probes (AC2-8).
    pub fn on_revocation(&self, epoch: u64) {
        self.bump_policy_epoch(epoch);
        let ids: Vec<String> = self
            .remotes
            .iter()
            .map(|r| r.node_id().to_string())
            .collect();
        self.mark_all_remotes_revoked();
        self.invalidate_snapshot_cache();
        self.on_policy_invalidation();
        if let Some(reg) = &self.capability_registry {
            if let Ok(mut g) = reg.write() {
                g.invalidate_all();
            }
        }
        for id in ids {
            self.notify_worker_lost(&id);
        }
    }

    /// Trust, classification, or policy changes invalidate queued dispatches (M5-3).
    pub fn on_policy_invalidation(&self) {
        self.job_registry.cancel_all_pending();
    }

    /// Notify optional compute-broker reservation release (M6-1 worker loss).
    pub fn set_worker_loss_hook(&self, hook: WorkerLossHook) {
        if let Ok(mut g) = self.worker_loss_hook.write() {
            *g = Some(hook);
        }
    }

    fn notify_worker_lost(&self, worker_id: &str) {
        if let Ok(g) = self.worker_loss_hook.read() {
            if let Some(hook) = g.as_ref() {
                hook(worker_id);
            }
        }
    }

    /// Trust downgrade or explicit reassignment — revalidate queued work (M5-3).
    pub fn on_trust_change(&self, worker_id: &str, trust: tetonic_domain::WorkerTrust) {
        for (job_id, session_id, data_class, project_policy) in
            self.job_registry.dispatched_for_worker(worker_id)
        {
            if trust_permits_data_class(trust, data_class, &project_policy).is_err() {
                tracing::error!(
                    worker = worker_id,
                    job = job_id,
                    class = ?data_class,
                    "audit incident: trust changed after sensitive remote dispatch started"
                );
                if let Some(sink) = &self.placement_sink {
                    sink.report(DispatchPlacementReport {
                        session_id,
                        agent_id: None,
                        target: worker_id.to_string(),
                        decision: DispatchDecision::Denied,
                        reason_code: Some("trust_changed_after_dispatch"),
                        reason: Some(
                            "trust changed after sensitive remote dispatch started".into(),
                        ),
                        effective_class: Some(data_class),
                        classification: None,
                        redacted: false,
                        worker_trust: Some(trust),
                    });
                }
            }
        }
        if let Some(remote) = self.remotes.iter().find(|r| r.node_id() == worker_id) {
            remote.set_worker_trust(trust);
        }
        if let Some(reg) = &self.capability_registry {
            if let Ok(mut g) = reg.write() {
                g.invalidate_worker(worker_id);
            }
        }
        self.invalidate_snapshot_cache();
        self.on_policy_invalidation();
    }

    /// Refresh coordinator-owned trust immediately before dispatch.
    ///
    /// A changed policy cancels the queued attempt and invalidates capability
    /// state. The caller must choose another target and create a fresh plan.
    fn refresh_worker_trust_before_dispatch(&self, remote: &dyn FabricNodeProvider) -> bool {
        let Some(resolver) = &self.worker_trust_resolver else {
            return false;
        };
        let Some((trust, epoch)) = resolver(remote.node_id()) else {
            let id = remote.node_id().to_string();
            remote.mark_fabric_revoked();
            self.on_policy_invalidation();
            self.notify_worker_lost(&id);
            return true;
        };
        let changed =
            trust != remote.worker_trust() || epoch > self.policy_epoch.load(Ordering::Relaxed);
        if changed {
            self.bump_policy_epoch(epoch);
            self.on_trust_change(remote.node_id(), trust);
        }
        changed
    }

    /// Mark every remote worker revoked so in-flight results are rejected.
    pub fn mark_all_remotes_revoked(&self) {
        for remote in &self.remotes {
            remote.mark_fabric_revoked();
        }
    }

    /// Coordinator trust currently stamped on a remote provider, if present.
    pub fn worker_trust_for(&self, worker_id: &str) -> Option<tetonic_domain::WorkerTrust> {
        self.remotes
            .iter()
            .find(|r| r.node_id() == worker_id)
            .and_then(|r| {
                if r.worker_trust_resolved() {
                    Some(r.worker_trust())
                } else {
                    None
                }
            })
    }

    fn resolved_remote_trust(
        remote: &dyn FabricNodeProvider,
    ) -> Option<tetonic_domain::WorkerTrust> {
        if remote.worker_trust_resolved() {
            Some(remote.worker_trust())
        } else {
            None
        }
    }

    /// Drop cached worker probes (e.g. after `fabric/status` or enrollment change).
    pub fn invalidate_snapshot_cache(&self) {
        if let Ok(mut g) = self.snapshot_cache.write() {
            *g = None;
            self.snapshot_epoch.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn snapshot_ttl() -> Duration {
        std::env::var("LOKAI_FABRIC_SNAPSHOT_TTL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .map(Duration::from_secs)
            .unwrap_or(Duration::from_secs(3))
    }

    #[cfg(test)]
    pub(crate) fn snapshot_generation(&self) -> u32 {
        self.snapshot_generation.load(Ordering::Relaxed)
    }

    async fn refresh_fabric_snapshot(&self, epoch: u64) -> FabricSnapshot {
        // Independent status probes should not wait behind local runtime I/O.
        let (mut snap, remote_nodes) = futures_util::join!(
            self.local.fabric_snapshot(),
            join_all(self.remotes.iter().map(|r| r.probe_node()))
        );
        snap.nodes.extend(remote_nodes.into_iter().flatten());
        snap.effective_concurrency = snap.nodes.iter().filter(|n| n.healthy).count() as u32;
        snap.effective_concurrency = snap.effective_concurrency.max(1);
        snap.generated_at = Utc::now();
        self.snapshot_generation.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut g) = self.snapshot_cache.write() {
            // An invalidation during I/O must not be overwritten by this result.
            // Dispatch still revalidates eligibility even for an uncached result.
            if self.snapshot_epoch.load(Ordering::Relaxed) == epoch {
                *g = Some(SnapshotCache {
                    snap: snap.clone(),
                    fetched_at: Instant::now(),
                });
            }
        }
        snap
    }

    fn fresh_snapshot(&self, force: bool) -> Option<FabricSnapshot> {
        let force = force
            || self
                .capability_registry
                .as_ref()
                .and_then(|r| r.read().ok())
                .is_some_and(|g| g.needs_snapshot_refresh());
        let ttl = Self::snapshot_ttl();
        if !force {
            if let Ok(g) = self.snapshot_cache.read() {
                if let Some(ref c) = *g {
                    if c.fetched_at.elapsed() < ttl {
                        return Some(c.snap.clone());
                    }
                }
            }
        }
        None
    }

    async fn cached_fabric_snapshot(&self, force: bool) -> FabricSnapshot {
        if let Some(snap) = self.fresh_snapshot(force) {
            return snap;
        }
        // One refresh per provider, shared by all sessions. Never hold the
        // synchronous registry/cache locks while awaiting network I/O.
        let _refresh = self.snapshot_refresh.lock().await;
        if let Some(snap) = self.fresh_snapshot(force) {
            return snap;
        }
        // Cancellation must not make an old snapshot reusable. Probes themselves
        // update the registry, so acknowledge their updates only after completion.
        self.invalidate_snapshot_cache();
        let epoch = self.snapshot_epoch.load(Ordering::Relaxed);
        let snap = self.refresh_fabric_snapshot(epoch).await;
        if let Some(reg) = &self.capability_registry {
            if let Ok(mut g) = reg.write() {
                if self.snapshot_epoch.load(Ordering::Relaxed) == epoch {
                    g.take_snapshot_invalidation();
                }
            }
        }
        snap
    }

    pub fn with_dispatch_guard(mut self, guard: Arc<dyn DispatchGuard>) -> Self {
        self.dispatch_guard = Some(guard);
        self
    }

    /// Placement gate used for Secret → local (M2-2 / R3-1).
    pub fn dispatch_guard(&self) -> Option<&Arc<dyn DispatchGuard>> {
        self.dispatch_guard.as_ref()
    }

    pub fn with_placement_sink(mut self, sink: Arc<dyn DispatchPlacementSink>) -> Self {
        self.placement_sink = Some(sink);
        self
    }

    #[cfg(test)]
    pub(crate) fn seed_snapshot_cache(&self, snap: FabricSnapshot) {
        if let Ok(mut g) = self.snapshot_cache.write() {
            *g = Some(SnapshotCache {
                snap,
                fetched_at: Instant::now(),
            });
        }
    }

    fn emit_placement(&self, req: &ChatRequest, report: DispatchPlacementReport) {
        if let Some(sink) = &self.placement_sink {
            sink.report(report);
        }
        let _ = req;
    }

    /// Dispatch-time revalidation: trust + capability freshness immediately before send (M5-3).
    fn revalidate_worker_at_dispatch(
        &self,
        attempt: &ChatRequest,
        worker_id: &str,
        trust: tetonic_domain::WorkerTrust,
        model: &ModelSelection,
        project_policy: &ProjectPlacementPolicy,
    ) -> Result<(), tetonic_domain::PlacementReason> {
        let epoch = self.policy_epoch.load(Ordering::Relaxed);
        let request =
            placement_request_from_chat(attempt, worker_id, epoch, project_policy.clone());
        let caps = self
            .capability_registry
            .as_ref()
            .and_then(|reg| reg.read().ok());
        let decision =
            evaluate_placement(&request, trust, caps.as_deref(), Some(model), Utc::now());
        match decision {
            PlacementDecision::Eligible { .. }
            | PlacementDecision::EligibleAfterRedaction { .. } => Ok(()),
            PlacementDecision::LocalOnly { reason } | PlacementDecision::Denied { reason } => {
                Err(reason)
            }
        }
    }

    pub fn project_placement_policy(&self) -> ProjectPlacementPolicy {
        self.dispatch_guard
            .as_ref()
            .map(|g| g.project_placement_policy())
            .unwrap_or_default()
    }

    /// Deprecated alias — use [`Self::with_dispatch_guard`].
    pub fn with_remote_policy<F>(mut self, check: F) -> Self
    where
        F: Fn(&FabricCallMeta, &str) -> bool + Send + Sync + 'static,
    {
        struct CallbackGuard<F>(F);
        impl<F> DispatchGuard for CallbackGuard<F>
        where
            F: Fn(&FabricCallMeta, &str) -> bool + Send + Sync,
        {
            fn evaluate(
                &self,
                request: &crate::DispatchRequest,
            ) -> Result<crate::DispatchDecision, crate::DispatchDenied> {
                use crate::{DispatchDecision, DispatchDenied, DispatchDestination};
                let worker_id = match &request.destination {
                    DispatchDestination::RemoteWorker { worker_id } => worker_id.as_str(),
                    _ => {
                        return Err(DispatchDenied::new(
                            "invalid_destination",
                            "callback guard only supports remote workers",
                        ));
                    }
                };
                let class = request
                    .session
                    .as_ref()
                    .or(request.payload.as_ref())
                    .map(|c| c.class)
                    .ok_or_else(|| {
                        DispatchDenied::new("missing_classification", "missing classification")
                    })?;
                let meta = FabricCallMeta {
                    data_class: class,
                    ..Default::default()
                };
                if (self.0)(&meta, worker_id) {
                    Ok(DispatchDecision::RemoteAllowed)
                } else {
                    Ok(DispatchDecision::LocalOnly)
                }
            }
        }
        self.dispatch_guard = Some(Arc::new(CallbackGuard(check)));
        self
    }

    /// Reset placement affinity when the session, user turn, run, or
    /// information context changes. Affinity is a worker preference for one
    /// bound execution, not a conversation another context may inherit.
    pub(crate) fn sync_turn(&self, fabric: Option<&FabricCallMeta>) {
        let Some(fabric) = fabric else {
            self.clear_placement_affinity();
            return;
        };
        let session = fabric.session_id.clone().filter(|id| !id.is_empty());
        let turn = fabric.turn_id.clone().filter(|id| !id.is_empty());
        let run = fabric.run_id.clone().filter(|id| !id.is_empty());
        let context = fabric
            .information_context_id
            .clone()
            .filter(|id| !id.is_empty());
        if session.is_none() && turn.is_none() && run.is_none() && context.is_none() {
            self.clear_placement_affinity();
            return;
        }
        let same_session = self
            .last_session_id
            .read()
            .ok()
            .and_then(|guard| guard.clone())
            == session;
        let same_turn = self
            .last_turn_id
            .read()
            .ok()
            .and_then(|guard| guard.clone())
            == turn;
        let same_run = self
            .last_run_id
            .read()
            .ok()
            .and_then(|guard| guard.clone())
            == run;
        let same_context = self
            .last_context_id
            .read()
            .ok()
            .and_then(|guard| guard.clone())
            == context;
        if same_session && same_turn && same_run && same_context {
            return;
        }
        if let Ok(mut last) = self.last_session_id.write() {
            *last = session;
        }
        if let Ok(mut last) = self.last_turn_id.write() {
            *last = turn;
        }
        if let Ok(mut last) = self.last_run_id.write() {
            *last = run;
        }
        if let Ok(mut last) = self.last_context_id.write() {
            *last = context;
        }
        if let Ok(mut affinity) = self.turn_affinity.write() {
            *affinity = None;
        }
    }

    fn clear_placement_affinity(&self) {
        if let Ok(mut last) = self.last_session_id.write() {
            *last = None;
        }
        if let Ok(mut last) = self.last_turn_id.write() {
            *last = None;
        }
        if let Ok(mut last) = self.last_run_id.write() {
            *last = None;
        }
        if let Ok(mut last) = self.last_context_id.write() {
            *last = None;
        }
        if let Ok(mut affinity) = self.turn_affinity.write() {
            *affinity = None;
        }
    }

    fn force_remote() -> bool {
        std::env::var("LOKAI_FABRIC_FORCE_REMOTE")
            .ok()
            .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    }

    pub fn cancel_session_jobs(&self, session_id: &str) {
        let jobs = self.job_registry.jobs_for_session(session_id);
        for remote in &self.remotes {
            let caps = remote.fabric_capabilities();
            if caps.legacy_v1_chat_only || !caps.supports_cancellation {
                tracing::debug!(
                    worker = %remote.node_id(),
                    "session cancel: legacy or no-cancel worker — coordinator registry only"
                );
                continue;
            }
            for (job_id, attempt_id, stored_run_id) in &jobs {
                let Some(run_id) = stored_run_id.as_deref() else {
                    let _ = job_id;
                    continue;
                };
                let cancel = tetonic_fabric_protocol::CancellationRequest {
                    run_id: tetonic_domain::ids::RunId::new(run_id),
                    task_id: tetonic_domain::ids::TaskId::new(attempt_id),
                    attempt_id: tetonic_domain::ids::AttemptId::new(attempt_id),
                    lease_id: tetonic_domain::ids::LeaseId::new(attempt_id),
                    lease_epoch: 1,
                    reason: format!("session cancel {session_id}"),
                    deadline: Utc::now() + chrono::Duration::seconds(30),
                };
                remote.request_jobs_cancel(cancel);
            }
        }
        self.job_registry.cancel_session(session_id);
    }

    /// When a registry entry exists, honor quarantine/expiry; unknown workers stay eligible until probed.
    fn remote_capability_schedulable(&self, worker_id: &str) -> bool {
        let Some(reg) = &self.capability_registry else {
            return true;
        };
        let Ok(guard) = reg.read() else {
            return false;
        };
        if guard.get(worker_id).is_none() {
            return false;
        }
        guard.schedulable(worker_id, Utc::now(), true).is_some()
    }

    fn model_on_node(&self, node: &crate::NodeInfo, selection: &ModelSelection) -> bool {
        if !node.healthy {
            return false;
        }
        if let Some(reg) = &self.capability_registry {
            if let Ok(guard) = reg.read() {
                if let Some(caps) = guard.get(&node.id) {
                    return tetonic_fabric_protocol::model_inventory_matches(
                        &caps.model_inventory,
                        selection,
                    );
                }
            }
        }
        if node.resident_models.is_empty() {
            return node.models_verified;
        }
        node.resident_models.iter().any(|m| {
            selection.local_name == *m || m.starts_with(&format!("{}:", selection.local_name))
        })
    }

    /// The compiled context class can be stricter than the host class. Placement
    /// uses the stricter one, so a secret context pack is not sent to a worker.
    fn effective_placement_class(
        fabric: Option<&crate::FabricCallMeta>,
    ) -> tetonic_domain::DataClass {
        let Some(fabric) = fabric else {
            return tetonic_domain::DataClass::default();
        };
        fabric
            .context_data_class
            .map(|context| fabric.data_class.max(context))
            .unwrap_or(fabric.data_class)
    }

    /// Ordered placement targets for a model (indices into `remotes`, or local).
    ///
    /// - `DataClass::Secret` → `[Local]` only (never remotes).
    /// - Non-empty `fallback_order` → walk that label list only (broker sole selector).
    /// - Else when `preferred` is set, pin that target first (M6-2 Option A).
    fn placement_order(
        &self,
        snap: &FabricSnapshot,
        selection: &ModelSelection,
        model_tier: Option<&str>,
        preferred: Option<&str>,
        fallback_order: &[String],
        data_class: tetonic_domain::DataClass,
    ) -> Vec<PlacementTarget> {
        let local_ok = snap
            .nodes
            .iter()
            .find(|n| n.id == LOCAL_NODE_ID)
            .is_some_and(|n| self.model_on_node(n, selection));

        if data_class == tetonic_domain::DataClass::Secret {
            return vec![PlacementTarget::Local];
        }

        if !fallback_order.is_empty() {
            let mut order = Vec::new();
            for label in fallback_order {
                if label == LOCAL_NODE_ID || label.eq_ignore_ascii_case("local") {
                    if local_ok && !order.contains(&PlacementTarget::Local) {
                        order.push(PlacementTarget::Local);
                    }
                    continue;
                }
                if let Some(idx) = self.remotes.iter().position(|r| r.node_id() == label) {
                    let id = self.remotes[idx].node_id();
                    let eligible = self.remote_capability_schedulable(id)
                        && snap
                            .nodes
                            .iter()
                            .find(|n| n.id == id)
                            .is_some_and(|n| self.model_on_node(n, selection));
                    if eligible {
                        let t = PlacementTarget::Remote(idx);
                        if !order.contains(&t) {
                            order.push(t);
                        }
                    }
                }
            }
            if order.is_empty() {
                return vec![PlacementTarget::Local];
            }
            return order;
        }

        if let Some(pref) = preferred {
            if pref == LOCAL_NODE_ID || pref.eq_ignore_ascii_case("local") {
                let remotes = self.ranked_remote_targets(snap, selection, None);
                if local_ok {
                    let mut order = vec![PlacementTarget::Local];
                    order.extend(remotes);
                    return order;
                }
                if !remotes.is_empty() {
                    return remotes;
                }
                return vec![PlacementTarget::Local];
            }

            // Explicit broker preferred remote: pin first, before local, no secondary ranking.
            if let Some(idx) = self.remotes.iter().position(|r| r.node_id() == pref) {
                let id = self.remotes[idx].node_id();
                let eligible = self.remote_capability_schedulable(id)
                    && snap
                        .nodes
                        .iter()
                        .find(|n| n.id == id)
                        .is_some_and(|n| self.model_on_node(n, selection));
                if eligible {
                    let mut order = vec![PlacementTarget::Remote(idx)];
                    order.extend(self.ranked_remote_targets(snap, selection, Some(idx)));
                    if local_ok {
                        order.push(PlacementTarget::Local);
                    }
                    return order;
                }
            }
            // Preferred missing/unusable — fall through to ordinary ranking.
        }

        let remote_targets = self.ranked_remote_targets(snap, selection, None);

        if Self::force_remote() {
            let mut order = remote_targets;
            if local_ok {
                order.push(PlacementTarget::Local);
            }
            return order;
        }

        // Hard tier: prefer enrolled workers with capacity before local (A9 v5 / fabric).
        if model_tier == Some("hard") && !remote_targets.is_empty() {
            let mut order = remote_targets;
            if local_ok {
                order.push(PlacementTarget::Local);
            }
            return order;
        }

        if local_ok {
            let mut order = vec![PlacementTarget::Local];
            order.extend(remote_targets);
            order
        } else if !remote_targets.is_empty() {
            remote_targets
        } else {
            vec![PlacementTarget::Local]
        }
    }

    fn ranked_remote_targets(
        &self,
        snap: &FabricSnapshot,
        selection: &ModelSelection,
        exclude: Option<usize>,
    ) -> Vec<PlacementTarget> {
        let mut remote_indices: Vec<usize> = (0..self.remotes.len())
            .filter(|&i| exclude != Some(i))
            .collect();

        // Turn affinity: prefer the worker used earlier in this turn.
        if let Some(aff) = self.turn_affinity.read().ok().and_then(|g| g.clone()) {
            if let Some(pos) = remote_indices
                .iter()
                .position(|&i| self.remotes[i].node_id() == aff)
            {
                let idx = remote_indices.remove(pos);
                remote_indices.insert(0, idx);
            }
        }

        // Resident model affinity, gates_ok (ES5-4), capability confidence, then least queue depth.
        remote_indices.sort_by_key(|&i| {
            let id = self.remotes[i].node_id();
            let node = snap.nodes.iter().find(|n| n.id == id);
            let capability_degraded = self
                .capability_registry
                .as_ref()
                .and_then(|r| r.read().ok())
                .is_some_and(|g| g.is_degraded(id));
            let gates_ok = node
                .and_then(|n| n.capacity.as_ref())
                .is_some_and(|c| c.gates_ok);
            let resident = node
                .map(|n| self.model_on_node(n, selection))
                .unwrap_or(false);
            let depth = node.map(|n| n.queue_depth).unwrap_or(u32::MAX);
            (capability_degraded, !gates_ok, !resident, depth)
        });

        remote_indices
            .into_iter()
            .filter(|&i| {
                let id = self.remotes[i].node_id();
                if !self.remote_capability_schedulable(id) {
                    return false;
                }
                snap.nodes
                    .iter()
                    .find(|n| n.id == id)
                    .is_some_and(|n| self.model_on_node(n, selection))
            })
            .map(PlacementTarget::Remote)
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlacementTarget {
    Local,
    Remote(usize),
}

fn stamp_failover_provenance(
    mut r: ChatResponse,
    redacted: bool,
    plan: Option<&crate::dispatch::RemoteAttemptPlan>,
) -> ChatResponse {
    r.provenance.prompt_redacted = redacted;
    if let Some(p) = plan {
        r.provenance.placement_decision = Some(match p.decision {
            DispatchDecision::LocalOnly => "local_only".into(),
            DispatchDecision::RemoteAllowed => "remote_allowed".into(),
            DispatchDecision::RemoteAllowedWithRedaction => "remote_allowed_with_redaction".into(),
            DispatchDecision::Denied => "denied".into(),
        });
        r.provenance.placement_reason_code = p.reason_code.map(String::from);
        if let Some(c) = &p.classification {
            r.provenance.placement_class = Some(c.class);
            r.provenance.classification_sources =
                c.sources.iter().map(|s| s.as_str().to_string()).collect();
        }
    }
    r
}

/// SEC2-E2-028: only the first placement target receives the full prompt on failover.
fn chat_request_for_placement(req: &ChatRequest, full_prompt_already_sent: bool) -> ChatRequest {
    if full_prompt_already_sent {
        let mut redacted = req.clone();
        redacted.messages = redact_messages_for_failover(&req.messages);
        redacted
    } else {
        req.clone()
    }
}

#[async_trait]
impl InferenceProvider for PooledProvider {
    async fn fabric_snapshot(&self) -> FabricSnapshot {
        self.cached_fabric_snapshot(false).await
    }

    async fn prewarm(&self, model: &str, keep_alive: Option<&str>) -> Result<(), InferenceError> {
        self.local.prewarm(model, keep_alive).await
    }

    async fn chat(
        &self,
        req: ChatRequest,
        on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        crate::require_outbound_scan(&req)?;
        let mut req = req;
        stamp_request_classification(&mut req);
        self.sync_turn(req.fabric.as_ref());
        let snap = self.cached_fabric_snapshot(false).await;
        let selection = ModelSelection::from_request(&req.model, req.model_digest.as_deref());
        let tier = req.fabric.as_ref().and_then(|f| f.model_tier.as_deref());
        let preferred = req
            .fabric
            .as_ref()
            .and_then(|f| f.preferred_target.as_deref());
        let fallback_order = req
            .fabric
            .as_ref()
            .map(|f| f.fallback_order.as_slice())
            .unwrap_or(&[]);
        let data_class = Self::effective_placement_class(req.fabric.as_ref());
        let order = self.placement_order(
            &snap,
            &selection,
            tier,
            preferred,
            fallback_order,
            data_class,
        );
        let job_id = new_fabric_job_id();
        let session_id = req.fabric.as_ref().and_then(|f| f.session_id.as_deref());
        let (bound_attempt, hop_run_id) = hop_identity_for_ajr(req.fabric.as_ref());
        let _job_guard = JobFinishGuard::new(&self.job_registry, &job_id);
        let mut attempt_id = self.job_registry.begin_or_bind_attempt(
            &job_id,
            session_id,
            bound_attempt.as_deref(),
            hop_run_id.as_deref(),
        );
        let fabric = req.fabric.clone();

        let mut last_err: Option<InferenceError> = None;
        let mut full_prompt_sent = false;
        for target in order {
            if full_prompt_sent && bound_attempt.is_none() {
                attempt_id = self.job_registry.supersede_attempt(&job_id);
            }
            let attempt = chat_request_for_placement(&req, full_prompt_sent);
            match target {
                PlacementTarget::Local => {
                    self.emit_placement(
                        &req,
                        DispatchPlacementReport {
                            session_id: req.fabric.as_ref().and_then(|f| f.session_id.clone()),
                            agent_id: req.fabric.as_ref().and_then(|f| f.agent_id.clone()),
                            target: LOCAL_NODE_ID.to_string(),
                            decision: DispatchDecision::LocalOnly,
                            reason_code: Some("local_placement"),
                            reason: None,
                            effective_class: req.fabric.as_ref().map(|f| f.data_class),
                            classification: req
                                .fabric
                                .as_ref()
                                .and_then(|f| f.payload_classification.clone()),
                            redacted: full_prompt_sent,
                            worker_trust: None,
                        },
                    );
                    match self.local.chat(attempt, on_token).await {
                        Ok(r) => {
                            if let Ok(mut g) = self.turn_affinity.write() {
                                *g = Some(LOCAL_NODE_ID.to_string());
                            }
                            return Ok(stamp_failover_provenance(r, full_prompt_sent, None));
                        }
                        Err(e) => last_err = Some(e),
                    }
                }
                PlacementTarget::Remote(i) => {
                    if req.outbound_scan.blocks_remote() {
                        last_err = Some(InferenceError::RemoteSecretDenied {
                            reason: "high-confidence secret finding; remote dispatch refused"
                                .into(),
                        });
                        continue;
                    }
                    let remote = &self.remotes[i];
                    if self.refresh_worker_trust_before_dispatch(remote.as_ref()) {
                        last_err = Some(InferenceError::Provider(
                            "worker trust or policy changed while work was queued; remote attempt canceled"
                                .into(),
                        ));
                        continue;
                    }
                    let Some(guard) = &self.dispatch_guard else {
                        tracing::warn!(
                            worker = %remote.label(),
                            "remote dispatch blocked: no dispatch guard configured"
                        );
                        last_err = Some(InferenceError::Provider(
                            "remote inference requires dispatch guard".into(),
                        ));
                        continue;
                    };
                    let resolved_trust = Self::resolved_remote_trust(remote.as_ref());
                    let plan = plan_remote_attempt(
                        guard.as_ref(),
                        &attempt,
                        remote.node_id(),
                        full_prompt_sent,
                        resolved_trust,
                    );
                    let selection = ModelSelection::from_request(
                        &attempt.model,
                        attempt.model_digest.as_deref(),
                    );
                    let project_policy = self.project_placement_policy();
                    let effective_class = plan
                        .classification
                        .as_ref()
                        .map(|c| c.class)
                        .or_else(|| attempt.fabric.as_ref().map(|f| f.data_class));

                    let mut plan = plan;
                    if plan.decision.allows_remote() && effective_class.is_some() {
                        if let Some(trust) = resolved_trust {
                            if let Err(reason) = self.revalidate_worker_at_dispatch(
                                &attempt,
                                remote.node_id(),
                                trust,
                                &selection,
                                &project_policy,
                            ) {
                                plan.decision = if placement_reason_is_local_only(&reason) {
                                    DispatchDecision::LocalOnly
                                } else {
                                    DispatchDecision::Denied
                                };
                                plan.reason_code = Some(reason.code());
                                plan.reason = Some(format!(
                                    "dispatch-time placement revalidation: {}",
                                    reason.code()
                                ));
                            }
                        }
                    }

                    let caps = remote.fabric_capabilities();
                    self.emit_placement(
                        &req,
                        DispatchPlacementReport {
                            session_id: req.fabric.as_ref().and_then(|f| f.session_id.clone()),
                            agent_id: req.fabric.as_ref().and_then(|f| f.agent_id.clone()),
                            target: remote.node_id().to_string(),
                            decision: plan.decision,
                            reason_code: if caps.legacy_v1_chat_only {
                                Some("legacy_v1_chat_only")
                            } else {
                                plan.reason_code
                            },
                            reason: if caps.legacy_v1_chat_only {
                                Some("legacy worker: infer-only, no lease/cancel semantics".into())
                            } else {
                                plan.reason.clone()
                            },
                            effective_class: plan.classification.as_ref().map(|c| c.class),
                            classification: plan.classification.as_ref().map(|c| c.summary()),
                            redacted: plan.redacted || full_prompt_sent,
                            worker_trust: resolved_trust,
                        },
                    );
                    if !plan.decision.allows_remote() {
                        tracing::warn!(
                            worker = %remote.label(),
                            decision = ?plan.decision,
                            reason_code = ?plan.reason_code,
                            "dispatch guard blocked remote placement"
                        );
                        last_err =
                            Some(InferenceError::Provider(plan.reason.unwrap_or_else(|| {
                                "dispatch guard: local-only for this payload".into()
                            })));
                        continue;
                    }
                    let plan_for_provenance = plan.clone();
                    let mut remote_attempt = plan.request;
                    stamp_request_classification(&mut remote_attempt);
                    if let Some(class) = effective_class {
                        self.job_registry
                            .mark_dispatched(
                                &job_id,
                                &attempt_id,
                                remote.node_id(),
                                class,
                                project_policy.clone(),
                            )
                            .map_err(|error| {
                                InferenceError::Provider(format!(
                                    "remote dispatch attempt became stale: {error}"
                                ))
                            })?;
                    }
                    match remote
                        .chat_on_fabric(
                            remote_attempt,
                            &job_id,
                            &attempt_id,
                            fabric.as_ref(),
                            self.turn_affinity
                                .read()
                                .ok()
                                .and_then(|g| g.clone())
                                .as_deref(),
                            Some(self.job_registry.as_ref()),
                            on_token,
                        )
                        .await
                    {
                        Ok(r) => {
                            if let Ok(mut g) = self.turn_affinity.write() {
                                *g = Some(remote.node_id().to_string());
                            }
                            return Ok(stamp_failover_provenance(
                                r,
                                plan_for_provenance.redacted || full_prompt_sent,
                                Some(&plan_for_provenance),
                            ));
                        }
                        Err(e) => {
                            let retry = matches!(
                                e,
                                InferenceError::Preempted { .. }
                                    | InferenceError::WorkerBusy { .. }
                            );
                            tracing::warn!(
                                "fabric failover: worker {} failed: {e}",
                                remote.label()
                            );
                            last_err = Some(e);
                            if !retry {
                                // hard errors still allow trying next target
                            }
                        }
                    }
                }
            }
            full_prompt_sent = true;
        }

        Err(last_err.unwrap_or_else(|| {
            InferenceError::Provider(format!("no capable node for model `{}`", req.model))
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeInfo;

    fn node(
        id: &str,
        healthy: bool,
        models: &[&str],
        depth: u32,
        capacity: Option<crate::NodeCapacityHealth>,
    ) -> NodeInfo {
        NodeInfo {
            id: id.into(),
            label: id.into(),
            vram_total_mb: 0,
            vram_free_mb: 0,
            resident_models: models.iter().map(|s| s.to_string()).collect(),
            queue_depth: depth,
            healthy,
            models_verified: !models.is_empty() || healthy,
            capacity,
            legacy_v1_chat_only: false,
            negotiated_protocol_version: None,
        }
    }

    #[test]
    fn local_preferred_when_model_available() {
        let local = Arc::new(OllamaProvider::new(
            "http://127.0.0.1:11434",
            Arc::new(tetonic_egress::EgressGuard::new()),
        ));
        let pooled = PooledProvider::new(local, vec![]);
        let snap = FabricSnapshot {
            nodes: vec![
                node(LOCAL_NODE_ID, true, &["qwen3.5:latest"], 0, None),
                node("worker_a", true, &["qwen3.5:latest"], 0, None),
            ],
            effective_concurrency: 2,
            generated_at: Utc::now(),
        };
        let order = pooled.placement_order(
            &snap,
            &ModelSelection::from_request("qwen3.5:latest", None),
            None,
            None,
            &[],
            tetonic_domain::DataClass::default(),
        );
        assert_eq!(order.first(), Some(&PlacementTarget::Local));
    }

    #[test]
    fn broker_preferred_worker_placed_first() {
        struct MockProvider {
            id: String,
        }

        #[async_trait::async_trait]
        impl crate::InferenceProvider for MockProvider {
            async fn fabric_snapshot(&self) -> FabricSnapshot {
                FabricSnapshot {
                    nodes: vec![],
                    effective_concurrency: 1,
                    generated_at: Utc::now(),
                }
            }

            async fn chat(
                &self,
                _req: crate::ChatRequest,
                _on_token: &mut crate::TokenSink<'_>,
            ) -> Result<crate::ChatResponse, crate::InferenceError> {
                unimplemented!()
            }
        }

        #[async_trait::async_trait]
        impl crate::FabricNodeProvider for MockProvider {
            fn node_id(&self) -> &str {
                &self.id
            }
            fn label(&self) -> &str {
                &self.id
            }
            fn fabric_capabilities(
                &self,
            ) -> tetonic_fabric_protocol::WorkerCapabilityAdvertisement {
                tetonic_fabric_protocol::WorkerCapabilityAdvertisement::legacy_v1_chat_infer_only()
            }
            async fn probe_node(&self) -> Option<crate::NodeInfo> {
                None
            }
            async fn chat_on_fabric(
                &self,
                _req: crate::ChatRequest,
                _job_id: &str,
                _attempt_id: &str,
                _session: Option<&crate::FabricCallMeta>,
                _turn_affinity: Option<&str>,
                _registry: Option<&crate::ActiveJobRegistry>,
                _on_token: &mut crate::TokenSink<'_>,
            ) -> Result<crate::ChatResponse, crate::InferenceError> {
                unimplemented!()
            }
        }

        let local = Arc::new(OllamaProvider::new(
            "http://127.0.0.1:11434",
            Arc::new(tetonic_egress::EgressGuard::new()),
        ));
        let remotes: Vec<Arc<dyn crate::FabricNodeProvider>> = vec![
            Arc::new(MockProvider {
                id: "worker_a".into(),
            }),
            Arc::new(MockProvider {
                id: "worker_b".into(),
            }),
        ];
        let pooled = PooledProvider::new_with_registry(
            local,
            remotes,
            Arc::new(std::sync::atomic::AtomicU64::new(0)),
            Arc::new(crate::ActiveJobRegistry::new()),
        );
        let snap = FabricSnapshot {
            nodes: vec![
                node(LOCAL_NODE_ID, true, &["qwen3.5:latest"], 0, None),
                node("worker_a", true, &["qwen3.5:latest"], 0, None),
                node("worker_b", true, &["qwen3.5:latest"], 0, None),
            ],
            effective_concurrency: 3,
            generated_at: Utc::now(),
        };
        let order = pooled.placement_order(
            &snap,
            &ModelSelection::from_request("qwen3.5:latest", None),
            None,
            Some("worker_b"),
            &[],
            tetonic_domain::DataClass::default(),
        );
        assert_eq!(order.first(), Some(&PlacementTarget::Remote(1)));
        assert!(order.iter().any(|t| matches!(t, PlacementTarget::Local)));
        let local_pos = order
            .iter()
            .position(|t| matches!(t, PlacementTarget::Local))
            .unwrap();
        assert!(local_pos > 0, "preferred remote must precede local");
    }

    #[test]
    fn hard_tier_skips_local_first_when_remotes_configured() {
        let local = Arc::new(OllamaProvider::new(
            "http://127.0.0.1:11434",
            Arc::new(tetonic_egress::EgressGuard::new()),
        ));
        // No live remotes in unit test — hard tier still returns local when alone.
        let pooled = PooledProvider::new(local, vec![]);
        let snap = FabricSnapshot {
            nodes: vec![node(LOCAL_NODE_ID, true, &["qwen3.6:latest"], 0, None)],
            effective_concurrency: 1,
            generated_at: Utc::now(),
        };
        let fast = pooled.placement_order(
            &snap,
            &ModelSelection::from_request("qwen3.6:latest", None),
            None,
            None,
            &[],
            tetonic_domain::DataClass::default(),
        );
        let hard = pooled.placement_order(
            &snap,
            &ModelSelection::from_request("qwen3.6:latest", None),
            Some("hard"),
            None,
            &[],
            tetonic_domain::DataClass::default(),
        );
        assert_eq!(fast.first(), Some(&PlacementTarget::Local));
        assert_eq!(hard.first(), Some(&PlacementTarget::Local));
    }

    #[test]
    fn falls_back_to_local_without_remote_providers() {
        let local = Arc::new(OllamaProvider::new(
            "http://127.0.0.1:11434",
            Arc::new(tetonic_egress::EgressGuard::new()),
        ));
        let pooled = PooledProvider::new(local, vec![]);
        let snap = FabricSnapshot {
            nodes: vec![
                node(LOCAL_NODE_ID, true, &["small:latest"], 0, None),
                node("worker_a", true, &["big:latest"], 0, None),
            ],
            effective_concurrency: 2,
            generated_at: Utc::now(),
        };
        let order = pooled.placement_order(
            &snap,
            &ModelSelection::from_request("big:latest", None),
            None,
            None,
            &[],
            tetonic_domain::DataClass::default(),
        );
        assert_eq!(order, vec![PlacementTarget::Local]);
    }

    #[test]
    fn hard_tier_prefers_gates_ok_worker() {
        use std::sync::Arc;

        use crate::NodeCapacityHealth;

        fn cap(gates_ok: bool) -> NodeCapacityHealth {
            NodeCapacityHealth {
                doctor: if gates_ok {
                    "healthy".into()
                } else {
                    "degraded".into()
                },
                active_profile_id: None,
                gates_ok,
                stale: false,
            }
        }

        struct MockProvider {
            id: String,
        }

        #[async_trait::async_trait]
        impl crate::InferenceProvider for MockProvider {
            async fn fabric_snapshot(&self) -> FabricSnapshot {
                FabricSnapshot {
                    nodes: vec![],
                    effective_concurrency: 1,
                    generated_at: chrono::Utc::now(),
                }
            }
            async fn chat(
                &self,
                _req: crate::ChatRequest,
                _on_token: &mut crate::TokenSink<'_>,
            ) -> Result<crate::ChatResponse, crate::InferenceError> {
                unimplemented!()
            }
        }

        #[async_trait::async_trait]
        impl crate::FabricNodeProvider for MockProvider {
            fn node_id(&self) -> &str {
                &self.id
            }
            fn label(&self) -> &str {
                &self.id
            }
            fn fabric_capabilities(
                &self,
            ) -> tetonic_fabric_protocol::WorkerCapabilityAdvertisement {
                tetonic_fabric_protocol::WorkerCapabilityAdvertisement::legacy_v1_chat_infer_only()
            }
            async fn probe_node(&self) -> Option<crate::NodeInfo> {
                None
            }
            async fn chat_on_fabric(
                &self,
                _req: crate::ChatRequest,
                _job_id: &str,
                _attempt_id: &str,
                _session: Option<&crate::FabricCallMeta>,
                _turn_affinity: Option<&str>,
                _registry: Option<&crate::ActiveJobRegistry>,
                _on_token: &mut crate::TokenSink<'_>,
            ) -> Result<crate::ChatResponse, crate::InferenceError> {
                unimplemented!()
            }
        }

        let local = Arc::new(OllamaProvider::new(
            "http://127.0.0.1:11434",
            Arc::new(tetonic_egress::EgressGuard::new()),
        ));
        let remotes: Vec<Arc<dyn crate::FabricNodeProvider>> = vec![
            Arc::new(MockProvider {
                id: "worker_bad".into(),
            }),
            Arc::new(MockProvider {
                id: "worker_good".into(),
            }),
        ];
        let pooled = PooledProvider::new_with_registry(
            local,
            remotes,
            Arc::new(std::sync::atomic::AtomicU64::new(0)),
            Arc::new(crate::ActiveJobRegistry::new()),
        );
        let snap = FabricSnapshot {
            nodes: vec![
                node(LOCAL_NODE_ID, true, &["qwen3.6:latest"], 0, None),
                node("worker_bad", true, &["qwen3.6:latest"], 0, Some(cap(false))),
                node("worker_good", true, &["qwen3.6:latest"], 0, Some(cap(true))),
            ],
            effective_concurrency: 2,
            generated_at: Utc::now(),
        };
        let order = pooled.placement_order(
            &snap,
            &ModelSelection::from_request("qwen3.6:latest", None),
            Some("hard"),
            None,
            &[],
            tetonic_domain::DataClass::default(),
        );
        assert_eq!(order.first(), Some(&PlacementTarget::Remote(1)));
    }

    #[test]
    fn quarantined_worker_excluded_from_placement() {
        use tetonic_domain::ids::WorkerId;
        use tetonic_fabric_protocol::{SandboxCapabilities, WorkerCapabilities};

        struct MockProvider {
            id: String,
        }

        #[async_trait::async_trait]
        impl crate::InferenceProvider for MockProvider {
            async fn fabric_snapshot(&self) -> FabricSnapshot {
                FabricSnapshot {
                    nodes: vec![],
                    effective_concurrency: 1,
                    generated_at: chrono::Utc::now(),
                }
            }
            async fn chat(
                &self,
                _req: crate::ChatRequest,
                _on_token: &mut crate::TokenSink<'_>,
            ) -> Result<crate::ChatResponse, crate::InferenceError> {
                unimplemented!()
            }
        }

        #[async_trait::async_trait]
        impl crate::FabricNodeProvider for MockProvider {
            fn node_id(&self) -> &str {
                &self.id
            }
            fn label(&self) -> &str {
                &self.id
            }
            fn fabric_capabilities(
                &self,
            ) -> tetonic_fabric_protocol::WorkerCapabilityAdvertisement {
                tetonic_fabric_protocol::WorkerCapabilityAdvertisement::legacy_v1_chat_infer_only()
            }
            async fn probe_node(&self) -> Option<crate::NodeInfo> {
                None
            }
            async fn chat_on_fabric(
                &self,
                _req: crate::ChatRequest,
                _job_id: &str,
                _attempt_id: &str,
                _session: Option<&crate::FabricCallMeta>,
                _turn_affinity: Option<&str>,
                _registry: Option<&crate::ActiveJobRegistry>,
                _on_token: &mut crate::TokenSink<'_>,
            ) -> Result<crate::ChatResponse, crate::InferenceError> {
                unimplemented!()
            }
        }

        let local = Arc::new(OllamaProvider::new(
            "http://127.0.0.1:11434",
            Arc::new(tetonic_egress::EgressGuard::new()),
        ));
        let remotes: Vec<Arc<dyn crate::FabricNodeProvider>> = vec![Arc::new(MockProvider {
            id: "worker_quarantined".into(),
        })];
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        {
            let mut caps = WorkerCapabilities::legacy_infer_profile(
                WorkerId::new("worker_quarantined"),
                "boot_1",
                1,
                0,
                &["qwen3.6:latest".into()],
                &["qwen3.6:latest".into()],
                8192,
                4096,
                0,
                0,
                2,
            );
            caps.sandbox = SandboxCapabilities {
                process_tree: tetonic_fabric_protocol::ControlSupport::Enforced,
                ..SandboxCapabilities::default()
            };
            let wid = WorkerId::new("worker_quarantined");
            registry
                .write()
                .unwrap()
                .upsert_validated(caps, &wid, 0, Utc::now())
                .unwrap();
            assert!(registry
                .read()
                .unwrap()
                .is_quarantined("worker_quarantined"));
        }
        let pooled = PooledProvider::new_with_registry(
            local,
            remotes,
            Arc::new(std::sync::atomic::AtomicU64::new(0)),
            Arc::new(crate::ActiveJobRegistry::new()),
        )
        .with_capability_registry(registry);
        let snap = FabricSnapshot {
            nodes: vec![
                node(LOCAL_NODE_ID, true, &["qwen3.6:latest"], 0, None),
                node("worker_quarantined", true, &["qwen3.6:latest"], 0, None),
            ],
            effective_concurrency: 2,
            generated_at: Utc::now(),
        };
        let order = pooled.placement_order(
            &snap,
            &ModelSelection::from_request("qwen3.6:latest", None),
            None,
            None,
            &[],
            tetonic_domain::DataClass::default(),
        );
        assert!(!order
            .iter()
            .any(|t| matches!(t, PlacementTarget::Remote(_))));
        assert_eq!(order.first(), Some(&PlacementTarget::Local));
    }

    #[test]
    fn degraded_worker_deprioritized_in_placement() {
        use tetonic_domain::ids::WorkerId;
        use tetonic_fabric_protocol::{ProbeSessionInput, WorkerCapabilities};

        struct MockProvider {
            id: String,
        }

        #[async_trait::async_trait]
        impl crate::InferenceProvider for MockProvider {
            async fn fabric_snapshot(&self) -> FabricSnapshot {
                FabricSnapshot {
                    nodes: vec![],
                    effective_concurrency: 1,
                    generated_at: chrono::Utc::now(),
                }
            }
            async fn chat(
                &self,
                _req: crate::ChatRequest,
                _on_token: &mut crate::TokenSink<'_>,
            ) -> Result<crate::ChatResponse, crate::InferenceError> {
                unimplemented!()
            }
        }

        #[async_trait::async_trait]
        impl crate::FabricNodeProvider for MockProvider {
            fn node_id(&self) -> &str {
                &self.id
            }
            fn label(&self) -> &str {
                &self.id
            }
            fn fabric_capabilities(
                &self,
            ) -> tetonic_fabric_protocol::WorkerCapabilityAdvertisement {
                tetonic_fabric_protocol::WorkerCapabilityAdvertisement::legacy_v1_chat_infer_only()
            }
            async fn probe_node(&self) -> Option<crate::NodeInfo> {
                None
            }
            async fn chat_on_fabric(
                &self,
                _req: crate::ChatRequest,
                _job_id: &str,
                _attempt_id: &str,
                _session: Option<&crate::FabricCallMeta>,
                _turn_affinity: Option<&str>,
                _registry: Option<&crate::ActiveJobRegistry>,
                _on_token: &mut crate::TokenSink<'_>,
            ) -> Result<crate::ChatResponse, crate::InferenceError> {
                unimplemented!()
            }
        }

        let local = Arc::new(OllamaProvider::new(
            "http://127.0.0.1:11434",
            Arc::new(tetonic_egress::EgressGuard::new()),
        ));
        let remotes: Vec<Arc<dyn crate::FabricNodeProvider>> = vec![
            Arc::new(MockProvider {
                id: "worker_degraded".into(),
            }),
            Arc::new(MockProvider {
                id: "worker_healthy".into(),
            }),
        ];
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let now = Utc::now();
        for (id, health_ok) in [("worker_degraded", false), ("worker_healthy", true)] {
            let caps = WorkerCapabilities::legacy_infer_profile(
                WorkerId::new(id),
                "boot_1",
                1,
                0,
                &["qwen3.6:latest".into()],
                &["qwen3.6:latest".into()],
                8192,
                4096,
                0,
                0,
                2,
            );
            let wid = WorkerId::new(id);
            registry
                .write()
                .unwrap()
                .upsert_probe_session(ProbeSessionInput {
                    caps,
                    channel_worker_id: &wid,
                    known_revocation_epoch: 0,
                    now,
                    previous: None,
                    health_ok,
                    models_verified: true,
                    verified_model_names: None,
                })
                .unwrap();
        }
        assert!(registry.read().unwrap().is_degraded("worker_degraded"));
        assert!(!registry.read().unwrap().is_degraded("worker_healthy"));
        let pooled = PooledProvider::new_with_registry(
            local,
            remotes,
            Arc::new(std::sync::atomic::AtomicU64::new(0)),
            Arc::new(crate::ActiveJobRegistry::new()),
        )
        .with_capability_registry(registry);
        let snap = FabricSnapshot {
            nodes: vec![
                node("worker_degraded", true, &["qwen3.6:latest"], 0, None),
                node("worker_healthy", true, &["qwen3.6:latest"], 0, None),
            ],
            effective_concurrency: 2,
            generated_at: now,
        };
        let order = pooled.placement_order(
            &snap,
            &ModelSelection::from_request("qwen3.6:latest", None),
            None,
            None,
            &[],
            tetonic_domain::DataClass::default(),
        );
        assert_eq!(order.first(), Some(&PlacementTarget::Remote(1)));
    }

    #[test]
    fn sync_turn_clears_affinity_on_new_user_turn() {
        use crate::FabricCallMeta;

        let local = Arc::new(OllamaProvider::new(
            "http://127.0.0.1:11434",
            Arc::new(tetonic_egress::EgressGuard::new()),
        ));
        let pooled = PooledProvider::new(local, vec![]);
        *pooled.turn_affinity.write().unwrap() = Some("worker_a".into());
        *pooled.last_turn_id.write().unwrap() = Some("turn_1".into());

        pooled.sync_turn(Some(&FabricCallMeta {
            turn_id: Some("turn_2".into()),
            ..Default::default()
        }));

        assert!(pooled.turn_affinity.read().unwrap().is_none());
        assert_eq!(
            pooled.last_turn_id.read().unwrap().as_deref(),
            Some("turn_2")
        );
    }

    #[test]
    fn sync_turn_drops_affinity_when_the_session_changes() {
        use crate::FabricCallMeta;

        let local = Arc::new(OllamaProvider::new(
            "http://127.0.0.1:11434",
            Arc::new(tetonic_egress::EgressGuard::new()),
        ));
        let pooled = PooledProvider::new(local, vec![]);
        *pooled.turn_affinity.write().unwrap() = Some("worker_private".into());
        *pooled.last_session_id.write().unwrap() = Some("private-session".into());
        *pooled.last_turn_id.write().unwrap() = Some("turn_1".into());

        pooled.sync_turn(Some(&FabricCallMeta {
            session_id: Some("team-session".into()),
            turn_id: Some("turn_1".into()),
            ..Default::default()
        }));
        assert!(pooled.turn_affinity.read().unwrap().is_none());
        assert_eq!(
            pooled.last_session_id.read().unwrap().as_deref(),
            Some("team-session")
        );

        *pooled.turn_affinity.write().unwrap() = Some("worker_team".into());
        pooled.sync_turn(Some(&FabricCallMeta {
            session_id: Some("team-session".into()),
            turn_id: Some("turn_1".into()),
            ..Default::default()
        }));
        assert_eq!(
            pooled.turn_affinity.read().unwrap().as_deref(),
            Some("worker_team")
        );

        pooled.sync_turn(None);
        assert!(pooled.turn_affinity.read().unwrap().is_none());
        assert!(pooled.last_session_id.read().unwrap().is_none());
        assert!(pooled.last_run_id.read().unwrap().is_none());
    }

    #[test]
    fn sync_turn_drops_affinity_when_the_run_changes() {
        use crate::FabricCallMeta;

        let local = Arc::new(OllamaProvider::new(
            "http://127.0.0.1:11434",
            Arc::new(tetonic_egress::EgressGuard::new()),
        ));
        let pooled = PooledProvider::new(local, vec![]);
        *pooled.turn_affinity.write().unwrap() = Some("worker_private".into());
        *pooled.last_session_id.write().unwrap() = Some("session".into());
        *pooled.last_turn_id.write().unwrap() = Some("turn_1".into());
        *pooled.last_run_id.write().unwrap() = Some("run-private".into());

        pooled.sync_turn(Some(&FabricCallMeta {
            session_id: Some("session".into()),
            turn_id: Some("turn_1".into()),
            run_id: Some("run-team".into()),
            ..Default::default()
        }));
        assert!(pooled.turn_affinity.read().unwrap().is_none());
        assert_eq!(
            pooled.last_run_id.read().unwrap().as_deref(),
            Some("run-team")
        );

        *pooled.turn_affinity.write().unwrap() = Some("worker_team".into());
        pooled.sync_turn(Some(&FabricCallMeta {
            session_id: Some("session".into()),
            turn_id: Some("turn_1".into()),
            run_id: Some("run-team".into()),
            ..Default::default()
        }));
        assert_eq!(
            pooled.turn_affinity.read().unwrap().as_deref(),
            Some("worker_team")
        );
    }

    #[test]
    fn sync_turn_drops_affinity_when_the_information_context_changes() {
        use crate::FabricCallMeta;

        let local = Arc::new(OllamaProvider::new(
            "http://127.0.0.1:11434",
            Arc::new(tetonic_egress::EgressGuard::new()),
        ));
        let pooled = PooledProvider::new(local, vec![]);
        *pooled.turn_affinity.write().unwrap() = Some("worker_private".into());
        *pooled.last_session_id.write().unwrap() = Some("session".into());
        *pooled.last_turn_id.write().unwrap() = Some("turn_1".into());
        *pooled.last_run_id.write().unwrap() = Some("run-1".into());
        *pooled.last_context_id.write().unwrap() = Some("private".into());

        pooled.sync_turn(Some(&FabricCallMeta {
            session_id: Some("session".into()),
            turn_id: Some("turn_1".into()),
            run_id: Some("run-1".into()),
            information_context_id: Some("participation/org/team/member".into()),
            ..Default::default()
        }));
        assert!(pooled.turn_affinity.read().unwrap().is_none());
        assert_eq!(
            pooled.last_context_id.read().unwrap().as_deref(),
            Some("participation/org/team/member")
        );

        *pooled.turn_affinity.write().unwrap() = Some("worker_participation".into());
        pooled.sync_turn(Some(&FabricCallMeta {
            session_id: Some("session".into()),
            turn_id: Some("turn_1".into()),
            run_id: Some("run-1".into()),
            information_context_id: Some("participation/org/team/member".into()),
            ..Default::default()
        }));
        assert_eq!(
            pooled.turn_affinity.read().unwrap().as_deref(),
            Some("worker_participation")
        );
    }

    #[tokio::test]
    async fn snapshot_cache_reuses_entry_within_ttl() {
        std::env::set_var("LOKAI_FABRIC_SNAPSHOT_TTL_SECS", "3600");
        let local = Arc::new(OllamaProvider::new(
            "http://127.0.0.1:11434",
            Arc::new(tetonic_egress::EgressGuard::new()),
        ));
        let pooled = PooledProvider::new(local, vec![]);
        assert_eq!(pooled.snapshot_generation(), 0);
        let _ = pooled.fabric_snapshot().await;
        assert_eq!(pooled.snapshot_generation(), 1);
        let _ = pooled.fabric_snapshot().await;
        assert_eq!(pooled.snapshot_generation(), 1);
        pooled.invalidate_snapshot_cache();
        let _ = pooled.fabric_snapshot().await;
        assert_eq!(pooled.snapshot_generation(), 2);
        std::env::remove_var("LOKAI_FABRIC_SNAPSHOT_TTL_SECS");
    }

    #[test]
    fn trust_downgrade_blocks_sensitive_at_dispatch_revalidation() {
        use crate::{ChatRequest, FabricCallMeta, Message};
        use tetonic_domain::WorkerTrust;

        let local = Arc::new(OllamaProvider::new(
            "http://127.0.0.1:11434",
            Arc::new(tetonic_egress::EgressGuard::new()),
        ));
        let pooled = PooledProvider::new(local, vec![]);
        let model = ModelSelection::from_request("qwen:7b", None);
        let project = ProjectPlacementPolicy::default();
        let req = ChatRequest {
            max_tokens: None,
            model: "qwen:7b".into(),
            model_digest: None,
            messages: vec![Message::user("summarize")],
            tools: vec![],
            temperature: 0.0,
            num_ctx: None,
            draft_model: None,
            draft_count: None,
            keep_alive: None,
            fabric: Some(FabricCallMeta {
                data_class: tetonic_domain::DataClass::SensitiveSource,
                ..Default::default()
            }),
            response_format: None,
            outbound_scan: Default::default(),
        };
        assert_eq!(
            pooled
                .revalidate_worker_at_dispatch(
                    &req,
                    "w1",
                    WorkerTrust::ExternalUntrusted,
                    &model,
                    &project,
                )
                .unwrap_err(),
            tetonic_domain::PlacementReason::WorkerTrustInsufficient
        );
    }

    #[test]
    fn stale_revocation_epoch_blocks_dispatch_revalidation() {
        use crate::{ChatRequest, FabricCallMeta, Message};
        use tetonic_domain::{ids::WorkerId, DataClass, WorkerTrust};
        use tetonic_fabric_protocol::WorkerCapabilities;

        let local = Arc::new(OllamaProvider::new(
            "http://127.0.0.1:11434",
            Arc::new(tetonic_egress::EgressGuard::new()),
        ));
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        {
            let now = Utc::now();
            let mut caps = WorkerCapabilities::legacy_infer_profile(
                WorkerId::new("w1"),
                "boot",
                1,
                2,
                &["qwen:7b".into()],
                &["qwen:7b".into()],
                8192,
                4096,
                0,
                0,
                2,
            );
            caps.generated_at = now;
            caps.valid_until = now + chrono::Duration::hours(1);
            registry
                .write()
                .unwrap()
                .upsert_validated(caps, &WorkerId::new("w1"), 0, now)
                .unwrap();
        }
        let pooled = PooledProvider::new_with_registry(
            local,
            vec![],
            Arc::new(AtomicU64::new(5)),
            Arc::new(ActiveJobRegistry::new()),
        )
        .with_capability_registry(registry);
        let model = ModelSelection::from_request("qwen:7b", None);
        let project = ProjectPlacementPolicy::default();
        let req = ChatRequest {
            max_tokens: None,
            model: "qwen:7b".into(),
            model_digest: None,
            messages: vec![Message::user("hello")],
            tools: vec![],
            temperature: 0.0,
            num_ctx: None,
            draft_model: None,
            draft_count: None,
            keep_alive: None,
            fabric: Some(FabricCallMeta {
                data_class: DataClass::RepositorySource,
                ..Default::default()
            }),
            response_format: None,
            outbound_scan: Default::default(),
        };
        assert_eq!(
            pooled
                .revalidate_worker_at_dispatch(
                    &req,
                    "w1",
                    WorkerTrust::OwnerControlledEstate,
                    &model,
                    &project,
                )
                .unwrap_err(),
            tetonic_domain::PlacementReason::WorkerRevoked
        );
    }

    #[test]
    fn on_policy_invalidation_cancels_pending_attempts() {
        let local = Arc::new(OllamaProvider::new(
            "http://127.0.0.1:11434",
            Arc::new(tetonic_egress::EgressGuard::new()),
        ));
        let jobs = Arc::new(crate::ActiveJobRegistry::new());
        let attempt = jobs.begin_attempt("job_x", None);
        let pooled = PooledProvider::new_with_registry(
            local,
            vec![],
            Arc::new(AtomicU64::new(0)),
            jobs.clone(),
        );
        pooled.on_policy_invalidation();
        assert!(jobs.validate("job_x", &attempt).is_err());
    }

    #[test]
    fn stale_cached_trust_blocks_at_dispatch_after_downgrade() {
        use crate::{ChatRequest, FabricCallMeta, Message};
        use tetonic_domain::WorkerTrust;

        struct MockProvider {
            id: String,
            trust: RwLock<WorkerTrust>,
        }

        #[async_trait::async_trait]
        impl crate::InferenceProvider for MockProvider {
            async fn fabric_snapshot(&self) -> FabricSnapshot {
                FabricSnapshot {
                    nodes: vec![],
                    effective_concurrency: 1,
                    generated_at: Utc::now(),
                }
            }
            async fn chat(
                &self,
                _req: crate::ChatRequest,
                _on_token: &mut crate::TokenSink<'_>,
            ) -> Result<crate::ChatResponse, crate::InferenceError> {
                unimplemented!()
            }
        }

        #[async_trait::async_trait]
        impl crate::FabricNodeProvider for MockProvider {
            fn node_id(&self) -> &str {
                &self.id
            }
            fn label(&self) -> &str {
                &self.id
            }
            fn fabric_capabilities(
                &self,
            ) -> tetonic_fabric_protocol::WorkerCapabilityAdvertisement {
                tetonic_fabric_protocol::WorkerCapabilityAdvertisement::legacy_v1_chat_infer_only()
            }
            fn worker_trust(&self) -> WorkerTrust {
                *self.trust.read().unwrap()
            }
            fn set_worker_trust(&self, trust: WorkerTrust) {
                *self.trust.write().unwrap() = trust;
            }
            async fn probe_node(&self) -> Option<crate::NodeInfo> {
                None
            }
            async fn chat_on_fabric(
                &self,
                _req: crate::ChatRequest,
                _job_id: &str,
                _attempt_id: &str,
                _session: Option<&crate::FabricCallMeta>,
                _turn_affinity: Option<&str>,
                _registry: Option<&crate::ActiveJobRegistry>,
                _on_token: &mut crate::TokenSink<'_>,
            ) -> Result<crate::ChatResponse, crate::InferenceError> {
                unimplemented!()
            }
        }

        let local = Arc::new(OllamaProvider::new(
            "http://127.0.0.1:11434",
            Arc::new(tetonic_egress::EgressGuard::new()),
        ));
        let mock = Arc::new(MockProvider {
            id: "w1".into(),
            trust: RwLock::new(WorkerTrust::OwnerControlledEstate),
        });
        let pooled = PooledProvider::new(local, vec![mock.clone()]);
        let model = ModelSelection::from_request("qwen:7b", None);
        let project = ProjectPlacementPolicy::default();
        let req = ChatRequest {
            max_tokens: None,
            model: "qwen:7b".into(),
            model_digest: None,
            messages: vec![Message::user("hello")],
            tools: vec![],
            temperature: 0.0,
            num_ctx: None,
            draft_model: None,
            draft_count: None,
            keep_alive: None,
            fabric: Some(FabricCallMeta {
                data_class: tetonic_domain::DataClass::SensitiveSource,
                ..Default::default()
            }),
            response_format: None,
            outbound_scan: Default::default(),
        };
        assert!(pooled
            .revalidate_worker_at_dispatch(
                &req,
                "w1",
                WorkerTrust::OwnerControlledEstate,
                &model,
                &project,
            )
            .is_ok());
        mock.set_worker_trust(WorkerTrust::ExternalUntrusted);
        assert_eq!(
            pooled
                .revalidate_worker_at_dispatch(&req, "w1", mock.worker_trust(), &model, &project,)
                .unwrap_err(),
            tetonic_domain::PlacementReason::WorkerTrustInsufficient
        );
    }

    #[test]
    fn dispatch_refresh_applies_persisted_trust_and_cancels_queued_attempt() {
        use tetonic_domain::WorkerTrust;

        struct MockProvider {
            id: String,
            trust: RwLock<WorkerTrust>,
        }

        #[async_trait::async_trait]
        impl crate::InferenceProvider for MockProvider {
            async fn fabric_snapshot(&self) -> FabricSnapshot {
                FabricSnapshot {
                    nodes: vec![],
                    effective_concurrency: 1,
                    generated_at: Utc::now(),
                }
            }
            async fn chat(
                &self,
                _req: crate::ChatRequest,
                _on_token: &mut crate::TokenSink<'_>,
            ) -> Result<crate::ChatResponse, crate::InferenceError> {
                unimplemented!()
            }
        }

        #[async_trait::async_trait]
        impl crate::FabricNodeProvider for MockProvider {
            fn node_id(&self) -> &str {
                &self.id
            }
            fn label(&self) -> &str {
                &self.id
            }
            fn fabric_capabilities(
                &self,
            ) -> tetonic_fabric_protocol::WorkerCapabilityAdvertisement {
                tetonic_fabric_protocol::WorkerCapabilityAdvertisement::legacy_v1_chat_infer_only()
            }
            fn worker_trust(&self) -> WorkerTrust {
                *self.trust.read().unwrap()
            }
            fn set_worker_trust(&self, trust: WorkerTrust) {
                *self.trust.write().unwrap() = trust;
            }
            async fn probe_node(&self) -> Option<crate::NodeInfo> {
                None
            }
            async fn chat_on_fabric(
                &self,
                _req: crate::ChatRequest,
                _job_id: &str,
                _attempt_id: &str,
                _session: Option<&crate::FabricCallMeta>,
                _turn_affinity: Option<&str>,
                _registry: Option<&crate::ActiveJobRegistry>,
                _on_token: &mut crate::TokenSink<'_>,
            ) -> Result<crate::ChatResponse, crate::InferenceError> {
                unimplemented!()
            }
        }

        let local = Arc::new(OllamaProvider::new(
            "http://127.0.0.1:11434",
            Arc::new(tetonic_egress::EgressGuard::new()),
        ));
        let mock = Arc::new(MockProvider {
            id: "w1".into(),
            trust: RwLock::new(WorkerTrust::OwnerControlledEstate),
        });
        let jobs = Arc::new(crate::ActiveJobRegistry::new());
        let _attempt = jobs.begin_attempt("job_1", Some("sess"));
        let pooled = PooledProvider::new_with_registry(
            local,
            vec![mock.clone()],
            Arc::new(std::sync::atomic::AtomicU64::new(0)),
            jobs.clone(),
        )
        .with_worker_trust_resolver(Arc::new(|worker_id| {
            (worker_id == "w1").then_some((WorkerTrust::ExternalUntrusted, 7))
        }));
        assert!(pooled.refresh_worker_trust_before_dispatch(mock.as_ref()));
        assert_eq!(mock.worker_trust(), WorkerTrust::ExternalUntrusted);
        assert_eq!(pooled.policy_epoch().load(Ordering::Relaxed), 7);
        assert!(jobs.validate("job_1", &_attempt).is_err());
    }

    #[test]
    fn fabric_job_ids_are_distinct() {
        let a = crate::new_fabric_job_id();
        let b = crate::new_fabric_job_id();
        assert_ne!(a, b);
        assert!(a.starts_with("job_"));
        assert!(b.starts_with("job_"));
    }

    #[test]
    fn failover_redacts_after_first_worker() {
        use crate::{ChatRequest, Message, ToolSchema};

        let req = ChatRequest {
            max_tokens: None,
            model: "m".into(),
            model_digest: None,
            messages: vec![
                Message::system("sys"),
                Message::user("secret prompt content"),
            ],
            tools: Vec::<ToolSchema>::new(),
            temperature: 0.2,
            num_ctx: None,
            draft_model: None,
            draft_count: None,
            keep_alive: None,
            fabric: None,
            response_format: None,
            outbound_scan: Default::default(),
        };
        let first = chat_request_for_placement(&req, false);
        assert_eq!(first.messages.len(), 2);
        let second = chat_request_for_placement(&req, true);
        assert_eq!(second.messages.len(), 1);
        assert!(second.messages[0].content.contains("withheld"));
        assert!(!second.messages[0].content.contains("secret"));
    }

    #[test]
    fn revocation_epoch_propagates() {
        use std::sync::atomic::Ordering;

        let registry = Arc::new(crate::ComputeTargetRegistry::new());
        let local = Arc::new(OllamaProvider::new(
            "http://127.0.0.1:11434",
            Arc::new(tetonic_egress::EgressGuard::new()),
        ));
        let pooled = PooledProvider::new_with_registry(
            local,
            vec![],
            registry.policy_epoch(),
            Arc::new(ActiveJobRegistry::new()),
        );
        registry.bump_epoch(7);
        pooled.on_revocation(7);
        assert_eq!(pooled.policy_epoch().load(Ordering::Relaxed), 7);
    }

    #[test]
    fn unverified_empty_models_not_capable() {
        let local = Arc::new(OllamaProvider::new(
            "http://127.0.0.1:11434",
            Arc::new(tetonic_egress::EgressGuard::new()),
        ));
        let pooled = PooledProvider::new(local, vec![]);
        let snap = FabricSnapshot {
            nodes: vec![node("worker_x", true, &[], 0, None)],
            effective_concurrency: 1,
            generated_at: Utc::now(),
        };
        // node() sets models_verified when healthy && models non-empty — override:
        let mut n = snap.nodes[0].clone();
        n.models_verified = false;
        let snap = FabricSnapshot {
            nodes: vec![n],
            ..snap
        };
        let order = pooled.placement_order(
            &snap,
            &ModelSelection::from_request("big:latest", None),
            None,
            None,
            &[],
            tetonic_domain::DataClass::default(),
        );
        assert_eq!(order, vec![PlacementTarget::Local]);
    }

    #[test]
    fn chat_registers_and_cleans_job_registry() {
        let local = Arc::new(OllamaProvider::new(
            "http://127.0.0.1:11434",
            Arc::new(tetonic_egress::EgressGuard::new()),
        ));
        let reg = Arc::new(ActiveJobRegistry::new());
        let pooled = PooledProvider::new_with_registry(
            local,
            vec![],
            Arc::new(AtomicU64::new(0)),
            reg.clone(),
        );
        assert_eq!(reg.len(), 0);
        let _ = pooled
            .job_registry()
            .begin_attempt("job_test", Some("sess"));
        assert_eq!(reg.len(), 1);
        reg.finish_job("job_test");
        assert_eq!(reg.len(), 0);
        pooled.cancel_session_jobs("sess");
    }

    #[test]
    fn fallback_order_walks_only_listed_targets() {
        struct MockProvider {
            id: String,
        }

        #[async_trait::async_trait]
        impl crate::InferenceProvider for MockProvider {
            async fn fabric_snapshot(&self) -> FabricSnapshot {
                FabricSnapshot {
                    nodes: vec![],
                    effective_concurrency: 1,
                    generated_at: Utc::now(),
                }
            }

            async fn chat(
                &self,
                _req: crate::ChatRequest,
                _on_token: &mut crate::TokenSink<'_>,
            ) -> Result<crate::ChatResponse, crate::InferenceError> {
                unimplemented!()
            }
        }

        #[async_trait::async_trait]
        impl crate::FabricNodeProvider for MockProvider {
            fn node_id(&self) -> &str {
                &self.id
            }
            fn label(&self) -> &str {
                &self.id
            }
            fn fabric_capabilities(
                &self,
            ) -> tetonic_fabric_protocol::WorkerCapabilityAdvertisement {
                tetonic_fabric_protocol::WorkerCapabilityAdvertisement::legacy_v1_chat_infer_only()
            }
            async fn probe_node(&self) -> Option<crate::NodeInfo> {
                None
            }
            async fn chat_on_fabric(
                &self,
                _req: crate::ChatRequest,
                _job_id: &str,
                _attempt_id: &str,
                _session: Option<&crate::FabricCallMeta>,
                _turn_affinity: Option<&str>,
                _registry: Option<&crate::ActiveJobRegistry>,
                _on_token: &mut crate::TokenSink<'_>,
            ) -> Result<crate::ChatResponse, crate::InferenceError> {
                unimplemented!()
            }
        }

        let local = Arc::new(OllamaProvider::new(
            "http://127.0.0.1:11434",
            Arc::new(tetonic_egress::EgressGuard::new()),
        ));
        let remotes: Vec<Arc<dyn crate::FabricNodeProvider>> = vec![
            Arc::new(MockProvider {
                id: "worker_a".into(),
            }),
            Arc::new(MockProvider {
                id: "worker_b".into(),
            }),
        ];
        let pooled = PooledProvider::new_with_registry(
            local,
            remotes,
            Arc::new(std::sync::atomic::AtomicU64::new(0)),
            Arc::new(crate::ActiveJobRegistry::new()),
        );
        let snap = FabricSnapshot {
            nodes: vec![
                node(LOCAL_NODE_ID, true, &["qwen3.5:latest"], 0, None),
                node("worker_a", true, &["qwen3.5:latest"], 0, None),
                node("worker_b", true, &["qwen3.5:latest"], 0, None),
            ],
            effective_concurrency: 3,
            generated_at: Utc::now(),
        };
        let order_labels = vec!["worker_b".into(), "local".into()];
        let order = pooled.placement_order(
            &snap,
            &ModelSelection::from_request("qwen3.5:latest", None),
            None,
            None,
            &order_labels,
            tetonic_domain::DataClass::RepositorySource,
        );
        assert_eq!(
            order,
            vec![PlacementTarget::Remote(1), PlacementTarget::Local]
        );
        assert!(
            !order
                .iter()
                .any(|t| matches!(t, PlacementTarget::Remote(0))),
            "worker_a must be omitted when not in fallback_order"
        );
    }

    #[test]
    fn secret_placement_is_local_only() {
        struct MockProvider {
            id: String,
        }

        #[async_trait::async_trait]
        impl crate::InferenceProvider for MockProvider {
            async fn fabric_snapshot(&self) -> FabricSnapshot {
                FabricSnapshot {
                    nodes: vec![],
                    effective_concurrency: 1,
                    generated_at: Utc::now(),
                }
            }

            async fn chat(
                &self,
                _req: crate::ChatRequest,
                _on_token: &mut crate::TokenSink<'_>,
            ) -> Result<crate::ChatResponse, crate::InferenceError> {
                unimplemented!()
            }
        }

        #[async_trait::async_trait]
        impl crate::FabricNodeProvider for MockProvider {
            fn node_id(&self) -> &str {
                &self.id
            }
            fn label(&self) -> &str {
                &self.id
            }
            fn fabric_capabilities(
                &self,
            ) -> tetonic_fabric_protocol::WorkerCapabilityAdvertisement {
                tetonic_fabric_protocol::WorkerCapabilityAdvertisement::legacy_v1_chat_infer_only()
            }
            async fn probe_node(&self) -> Option<crate::NodeInfo> {
                None
            }
            async fn chat_on_fabric(
                &self,
                _req: crate::ChatRequest,
                _job_id: &str,
                _attempt_id: &str,
                _session: Option<&crate::FabricCallMeta>,
                _turn_affinity: Option<&str>,
                _registry: Option<&crate::ActiveJobRegistry>,
                _on_token: &mut crate::TokenSink<'_>,
            ) -> Result<crate::ChatResponse, crate::InferenceError> {
                unimplemented!()
            }
        }

        let local = Arc::new(OllamaProvider::new(
            "http://127.0.0.1:11434",
            Arc::new(tetonic_egress::EgressGuard::new()),
        ));
        let remotes: Vec<Arc<dyn crate::FabricNodeProvider>> = vec![Arc::new(MockProvider {
            id: "worker_a".into(),
        })];
        let pooled = PooledProvider::new_with_registry(
            local,
            remotes,
            Arc::new(std::sync::atomic::AtomicU64::new(0)),
            Arc::new(crate::ActiveJobRegistry::new()),
        );
        let snap = FabricSnapshot {
            nodes: vec![
                node(LOCAL_NODE_ID, true, &["qwen3.5:latest"], 0, None),
                node("worker_a", true, &["qwen3.5:latest"], 0, None),
            ],
            effective_concurrency: 2,
            generated_at: Utc::now(),
        };
        let order_labels = vec!["worker_a".into(), "local".into()];
        let order = pooled.placement_order(
            &snap,
            &ModelSelection::from_request("qwen3.5:latest", None),
            None,
            Some("worker_a"),
            &order_labels,
            tetonic_domain::DataClass::Secret,
        );
        assert_eq!(order, vec![PlacementTarget::Local]);
        let secret_context = crate::FabricCallMeta {
            data_class: tetonic_domain::DataClass::RepositorySource,
            context_data_class: Some(tetonic_domain::DataClass::Secret),
            ..Default::default()
        };
        let class = PooledProvider::effective_placement_class(Some(&secret_context));
        let context_order = pooled.placement_order(
            &snap,
            &ModelSelection::from_request("qwen3.5:latest", None),
            Some("hard"),
            Some("worker_a"),
            &order_labels,
            class,
        );
        assert_eq!(context_order, vec![PlacementTarget::Local]);
        let ordinary = PooledProvider::effective_placement_class(Some(&crate::FabricCallMeta {
            data_class: tetonic_domain::DataClass::RepositorySource,
            ..Default::default()
        }));
        assert_eq!(ordinary, tetonic_domain::DataClass::RepositorySource);
    }

    #[tokio::test]
    async fn pooled_chat_uses_fabric_attempt_id_on_remote() {
        use crate::{ChatResponse, FabricCallMeta, InferenceProvenance, Message};
        use std::sync::Mutex;

        struct RecordingRemote {
            id: String,
            seen: Mutex<Option<String>>,
        }

        #[async_trait::async_trait]
        impl crate::InferenceProvider for RecordingRemote {
            async fn fabric_snapshot(&self) -> FabricSnapshot {
                FabricSnapshot {
                    nodes: vec![],
                    effective_concurrency: 1,
                    generated_at: Utc::now(),
                }
            }
            async fn chat(
                &self,
                _req: crate::ChatRequest,
                _on_token: &mut crate::TokenSink<'_>,
            ) -> Result<ChatResponse, crate::InferenceError> {
                unimplemented!()
            }
        }

        #[async_trait::async_trait]
        impl crate::FabricNodeProvider for RecordingRemote {
            fn node_id(&self) -> &str {
                &self.id
            }
            fn label(&self) -> &str {
                &self.id
            }
            fn fabric_capabilities(
                &self,
            ) -> tetonic_fabric_protocol::WorkerCapabilityAdvertisement {
                tetonic_fabric_protocol::WorkerCapabilityAdvertisement::legacy_v1_chat_infer_only()
            }
            async fn probe_node(&self) -> Option<crate::NodeInfo> {
                Some(node(&self.id, true, &["qwen:7b"], 0, None))
            }
            async fn chat_on_fabric(
                &self,
                _req: crate::ChatRequest,
                _job_id: &str,
                attempt_id: &str,
                _session: Option<&crate::FabricCallMeta>,
                _turn_affinity: Option<&str>,
                _registry: Option<&crate::ActiveJobRegistry>,
                _on_token: &mut crate::TokenSink<'_>,
            ) -> Result<ChatResponse, crate::InferenceError> {
                *self.seen.lock().unwrap() = Some(attempt_id.to_string());
                Ok(ChatResponse {
                    message: Message::assistant("ok"),
                    usage: Default::default(),
                    provenance: InferenceProvenance {
                        attempt_id: Some(attempt_id.to_string()),
                        ..Default::default()
                    },
                })
            }
        }

        let local = Arc::new(OllamaProvider::new(
            "http://127.0.0.1:11434",
            Arc::new(tetonic_egress::EgressGuard::new()),
        ));
        let remote = Arc::new(RecordingRemote {
            id: "worker_a".into(),
            seen: Mutex::new(None),
        });
        let remotes: Vec<Arc<dyn crate::FabricNodeProvider>> = vec![remote.clone()];
        let pooled = PooledProvider::new(local, remotes).with_remote_policy(|_, _| true);
        pooled.seed_snapshot_cache(FabricSnapshot {
            nodes: vec![node("worker_a", true, &["qwen:7b"], 0, None)],
            effective_concurrency: 1,
            generated_at: Utc::now(),
        });
        let req = crate::ChatRequest {
            max_tokens: None,
            model: "qwen:7b".into(),
            model_digest: None,
            messages: vec![Message::user("hi")],
            tools: vec![],
            temperature: 0.0,
            num_ctx: None,
            draft_model: None,
            draft_count: None,
            keep_alive: None,
            fabric: Some(FabricCallMeta {
                attempt_id: Some("att_leased".into()),
                hop_attempt_id: Some("att_hop".into()),
                hop_run_id: Some("run_hop".into()),
                preferred_target: Some("worker_a".into()),
                fallback_order: vec!["worker_a".into()],
                data_class: tetonic_domain::DataClass::RepositorySource,
                ..Default::default()
            }),
            response_format: None,
            outbound_scan: crate::OutboundScan::from_scan(false),
        };
        let mut sink = |_t: &str| {};
        let resp = pooled.chat(req, &mut sink).await.expect("remote chat");
        assert_eq!(remote.seen.lock().unwrap().as_deref(), Some("att_hop"));
        assert_eq!(resp.provenance.attempt_id.as_deref(), Some("att_hop"));
    }

    #[tokio::test]
    async fn high_confidence_scan_refuses_remote_and_sends_nothing() {
        use crate::{ChatResponse, FabricCallMeta, InferenceError, InferenceProvenance, Message};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Mutex;

        struct FlagRemote {
            id: String,
            sent: AtomicBool,
            seen_pem: Mutex<bool>,
        }

        #[async_trait::async_trait]
        impl crate::InferenceProvider for FlagRemote {
            async fn fabric_snapshot(&self) -> FabricSnapshot {
                FabricSnapshot {
                    nodes: vec![],
                    effective_concurrency: 1,
                    generated_at: Utc::now(),
                }
            }
            async fn chat(
                &self,
                _req: crate::ChatRequest,
                _on_token: &mut crate::TokenSink<'_>,
            ) -> Result<ChatResponse, crate::InferenceError> {
                unimplemented!()
            }
        }

        #[async_trait::async_trait]
        impl crate::FabricNodeProvider for FlagRemote {
            fn node_id(&self) -> &str {
                &self.id
            }
            fn label(&self) -> &str {
                &self.id
            }
            fn fabric_capabilities(
                &self,
            ) -> tetonic_fabric_protocol::WorkerCapabilityAdvertisement {
                tetonic_fabric_protocol::WorkerCapabilityAdvertisement::legacy_v1_chat_infer_only()
            }
            async fn probe_node(&self) -> Option<crate::NodeInfo> {
                Some(node(&self.id, true, &["qwen:7b"], 0, None))
            }
            async fn chat_on_fabric(
                &self,
                req: crate::ChatRequest,
                _job_id: &str,
                _attempt_id: &str,
                _session: Option<&crate::FabricCallMeta>,
                _turn_affinity: Option<&str>,
                _registry: Option<&crate::ActiveJobRegistry>,
                _on_token: &mut crate::TokenSink<'_>,
            ) -> Result<ChatResponse, crate::InferenceError> {
                self.sent.store(true, Ordering::SeqCst);
                *self.seen_pem.lock().unwrap() = req.messages.iter().any(|m| {
                    m.content.contains("BEGIN RSA PRIVATE KEY") || m.content.contains("AKIA")
                });
                Ok(ChatResponse {
                    message: Message::assistant("ok"),
                    usage: Default::default(),
                    provenance: InferenceProvenance::default(),
                })
            }
        }

        let local = Arc::new(OllamaProvider::new(
            "http://127.0.0.1:11434",
            Arc::new(tetonic_egress::EgressGuard::new()),
        ));
        let remote = Arc::new(FlagRemote {
            id: "worker_a".into(),
            sent: AtomicBool::new(false),
            seen_pem: Mutex::new(false),
        });
        let remotes: Vec<Arc<dyn crate::FabricNodeProvider>> = vec![remote.clone()];
        let pooled = PooledProvider::new(local, remotes).with_remote_policy(|_, _| true);
        pooled.seed_snapshot_cache(FabricSnapshot {
            nodes: vec![node("worker_a", true, &["qwen:7b"], 0, None)],
            effective_concurrency: 1,
            generated_at: Utc::now(),
        });
        let req = crate::ChatRequest {
            max_tokens: None,
            model: "qwen:7b".into(),
            messages: vec![Message::user("hi")],
            fabric: Some(FabricCallMeta {
                preferred_target: Some("worker_a".into()),
                fallback_order: vec!["worker_a".into()],
                data_class: tetonic_domain::DataClass::RepositorySource,
                ..Default::default()
            }),
            outbound_scan: crate::OutboundScan::from_scan(true),
            ..Default::default()
        };
        let mut sink = |_t: &str| {};
        let err = pooled.chat(req, &mut sink).await.unwrap_err();
        assert!(
            matches!(err, InferenceError::RemoteSecretDenied { .. }),
            "got {err}"
        );
        assert!(
            !remote.sent.load(Ordering::SeqCst),
            "remote must not receive the request"
        );
        assert!(!*remote.seen_pem.lock().unwrap());
    }

    #[test]
    fn cmp02_pooled_prefers_hop_attempt_id() {
        let both = FabricCallMeta {
            run_id: Some("agent_run".into()),
            attempt_id: Some("agent_att".into()),
            hop_run_id: Some("hop_run".into()),
            hop_attempt_id: Some("hop_att".into()),
            ..Default::default()
        };
        let (att, run) = hop_identity_for_ajr(Some(&both));
        assert_eq!(att.as_deref(), Some("hop_att"));
        assert_eq!(run.as_deref(), Some("hop_run"));

        let agent_only = FabricCallMeta {
            run_id: Some("agent_run".into()),
            attempt_id: Some("agent_att".into()),
            ..Default::default()
        };
        let (att, run) = hop_identity_for_ajr(Some(&agent_only));
        assert_eq!(att, None, "must not fall back to agent attempt_id");
        assert_eq!(run, None, "must not fall back to agent run_id");

        let minted = crate::ActiveJobRegistry::new().begin_or_bind_attempt(
            "job_cmp02",
            None,
            att.as_deref(),
            run.as_deref(),
        );
        assert_ne!(minted, "agent_att");
    }
}

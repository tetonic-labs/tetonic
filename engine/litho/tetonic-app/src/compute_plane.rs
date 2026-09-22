//! Shared inference compute plane for CLI and daemon (remainder R3-1).
//!
//! Always wraps local or pooled inference in `BrokerInferenceProvider` so Infer
//! enters ComputeBroker (admission, scheduler spans, PolicyDispatchGuard) and
//! H1-1 outbound secret scanning.

use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use tetonic_broker::{
    BrokerInferenceProvider, BudgetLimits, DefaultComputeBroker, FairnessPolicy,
    HierarchicalAdmissionController, HierarchicalBudgetLedger, InMemoryReservationStore,
    InMemorySchedulerStore, InferenceTargetAdapter, MemoryReservationStore, MemorySchedulerStore,
    QueueLimits, QueueManager, ReservationStore, SchedulerDecisionStore,
};
use tetonic_domain::secrets::{OutboundRedactionSink, SecretScanner};
use tetonic_domain::WorkerTrust;
use tetonic_egress::EgressGuard;
use tetonic_enroll::KeyPair;
use tetonic_fabric_client::{CapabilityRegistry, RemoteNodeProvider};
use tetonic_inference::{
    AuthorizedComputeTarget, ComputeTargetRegistry, OllamaProvider, PooledProvider,
};
use tetonic_memory::{SharedStore, Store};
use tetonic_policy::{PolicyDispatchGuard, PolicyEngine};

/// Production compute plane behind `InferenceProvider`.
pub struct ComputePlane {
    pub provider: Arc<BrokerInferenceProvider>,
    pub pooled: Option<Arc<PooledProvider>>,
    /// Always present: local-only Infer still goes through ComputeBroker.
    pub compute_broker: Arc<DefaultComputeBroker>,
    pub compute_registry: Option<Arc<ComputeTargetRegistry>>,
    pub fabric_remotes: Vec<Arc<RemoteNodeProvider>>,
    pub coordinator: Option<Arc<KeyPair>>,
    pub worker_activity_targets: Vec<(IpAddr, u16, Vec<u8>)>,
}

/// Inputs shared by `lokai-cli` and `lokaid`.
pub struct ComputePlaneRequest {
    pub guard: Arc<EgressGuard>,
    pub ollama_base: String,
    pub policy: Arc<PolicyEngine>,
    pub workspace_root: PathBuf,
    pub artifact_store: Arc<dyn tetonic_domain::artifact::ArtifactStore>,
    pub store: Option<SharedStore>,
    pub coordinator: Option<Arc<KeyPair>>,
    pub placement_sink: Option<Arc<dyn tetonic_inference::DispatchPlacementSink>>,
    pub previous_pooled: Option<Arc<PooledProvider>>,
}

/// Build the same plane the daemon uses: PooledProvider + PolicyDispatchGuard + broker.
pub async fn build_compute_plane(req: ComputePlaneRequest) -> ComputePlane {
    let local = Arc::new(OllamaProvider::new(&req.ollama_base, req.guard.clone()));
    let mut worker_activity_targets = Vec::new();
    let coordinator = req.coordinator.clone();

    if let Some(store) = req.store.clone() {
        {
            let guard = req.guard.clone();
            worker_activity_targets = store
                .read(move |db| {
                    if let Err(e) = crate::estate_enrollment::reload_enrollment_egress(db, &guard) {
                        tracing::warn!("enrollment egress reload: {e}");
                    }
                    load_worker_activity_targets(db)
                })
                .await
                .unwrap_or_default();
        }

        if let Some(coord) = coordinator.clone() {
            let registry = store
                .read(|db| load_compute_registry(db).ok())
                .await
                .unwrap_or_default();
            if let Some(registry) = registry {
                let owner = store
                    .read({
                        let coord_pk = coord.public().0.clone();
                        move |db| db.ensure_owner_identity(&coord_pk, "default")
                    })
                    .await
                    .ok()
                    .and_then(|res| res.ok());

                if let Some(owner) = owner {
                    let built = build_pooled_provider(
                        registry.clone(),
                        req.guard.clone(),
                        local.clone(),
                        req.policy.clone(),
                        store.clone(),
                        req.workspace_root.clone(),
                        req.artifact_store.clone(),
                        req.placement_sink.clone(),
                        req.previous_pooled.clone(),
                        coord.clone(),
                        owner,
                    );
                    match built {
                        Ok((pooled, fabric_remotes)) => {
                            let caps = pooled.capability_registry();
                            let (provider, compute_broker) =
                                wrap_pooled_with_broker(pooled.clone(), Some(store), caps);
                            return ComputePlane {
                                provider,
                                pooled: Some(pooled),
                                compute_broker,
                                compute_registry: Some(registry),
                                fabric_remotes,
                                coordinator: Some(coord),
                                worker_activity_targets,
                            };
                        }
                        Err(e) => tracing::warn!("fabric pooled provider: {e}"),
                    }
                }
            }
        }
    }

    let local_pooled = Arc::new(
        PooledProvider::new(local, Vec::new())
            .with_dispatch_guard(Arc::new(PolicyDispatchGuard::new(req.policy))),
    );
    let (provider, compute_broker) = wrap_pooled_with_broker(local_pooled.clone(), req.store, None);
    ComputePlane {
        provider,
        pooled: Some(local_pooled),
        compute_broker,
        compute_registry: None,
        fabric_remotes: Vec::new(),
        coordinator,
        worker_activity_targets,
    }
}

/// Wrap any pooled provider with ComputeBroker + BrokerInferenceProvider.
pub(crate) fn wrap_pooled_with_broker(
    pooled: Arc<PooledProvider>,
    store: Option<SharedStore>,
    capability_registry: Option<Arc<RwLock<CapabilityRegistry>>>,
) -> (Arc<BrokerInferenceProvider>, Arc<DefaultComputeBroker>) {
    let adapter = Arc::new(InferenceTargetAdapter::new(pooled.clone()));
    let budgets = Arc::new(HierarchicalBudgetLedger::new(BudgetLimits::default()));
    let queue = Arc::new(QueueManager::new(
        QueueLimits::default(),
        FairnessPolicy::default(),
    ));
    let admission = Arc::new(HierarchicalAdmissionController::new(budgets, queue));
    let persist: Arc<dyn ReservationStore> = match store.clone() {
        Some(s) => Arc::new(MemoryReservationStore::new(s)),
        None => Arc::new(InMemoryReservationStore::default()),
    };
    let redaction_store = store.clone();
    let scheduler_store: Arc<dyn SchedulerDecisionStore> = match store {
        Some(s) => Arc::new(MemorySchedulerStore::new(s)),
        None => Arc::new(InMemorySchedulerStore::new()),
    };
    let broker = Arc::new(DefaultComputeBroker::new(
        admission,
        persist,
        Some(adapter),
        None,
    ));
    broker.set_scheduler_store(scheduler_store);
    if let Some(reg) = capability_registry {
        broker.set_capability_registry(reg);
    }
    {
        let release = broker.clone();
        pooled.set_worker_loss_hook(Arc::new(move |worker_id: &str| {
            let _continue_ids = release.on_worker_lost(worker_id);
        }));
    }
    if let Err(e) = broker.recover_reservations() {
        tracing::warn!("compute reservation recovery: {e}");
    }
    if let Err(e) = broker.recover_scheduler_decisions() {
        tracing::warn!("scheduler decision recovery: {e}");
    }
    let scanner: Arc<dyn SecretScanner> =
        crate::secret_scanner_factory::scanner_from_shared_store(&redaction_store);
    let sink: Arc<dyn OutboundRedactionSink> = match redaction_store {
        Some(s) => Arc::new(crate::redaction_audit::StoreRedactionSink::new(s)),
        None => Arc::new(crate::redaction_audit::MissingStoreRedactionSink),
    };
    let provider =
        Arc::new(BrokerInferenceProvider::new(broker.clone()).with_outbound_scanner(scanner, sink));
    (provider, broker)
}

fn load_worker_activity_targets(store: &Store) -> Vec<(IpAddr, u16, Vec<u8>)> {
    store
        .list_worker_enrollments()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|w| {
            let cert = w.fabric_tls_cert.filter(|c| !c.is_empty())?;
            let ip: IpAddr = w.ip.parse().ok()?;
            Some((ip, w.fabric_port, cert))
        })
        .collect()
}

fn load_compute_registry(store: &Store) -> Result<Arc<ComputeTargetRegistry>, anyhow::Error> {
    let workers = store.list_worker_enrollments()?;
    if workers.is_empty() {
        anyhow::bail!("no enrolled workers");
    }
    let registry = Arc::new(ComputeTargetRegistry::new());
    let targets = workers
        .into_iter()
        .filter_map(|w| {
            let cert = w.fabric_tls_cert.filter(|c| !c.is_empty())?;
            let ip: IpAddr = w.ip.parse().ok()?;
            Some(AuthorizedComputeTarget {
                id: w.id,
                label: w.label,
                ip,
                fabric_port: w.fabric_port,
                fabric_tls_cert: Arc::from(cert.into_boxed_slice()),
                worker_trust: w
                    .worker_trust
                    .as_deref()
                    .and_then(WorkerTrust::parse)
                    .unwrap_or(WorkerTrust::OwnerControlledEstate),
            })
        })
        .collect::<Vec<_>>();
    if targets.is_empty() {
        anyhow::bail!("workers enrolled but none have fabric TLS certs — re-enroll");
    }
    registry.replace_targets(targets);
    let max_epoch = store.max_worker_trust_policy_epoch().unwrap_or(0);
    if max_epoch > 0 {
        registry.bump_epoch(max_epoch);
    }
    Ok(registry)
}

#[allow(clippy::too_many_arguments)]
fn build_pooled_provider(
    registry: Arc<ComputeTargetRegistry>,
    guard: Arc<EgressGuard>,
    local: Arc<OllamaProvider>,
    policy: Arc<PolicyEngine>,
    trust_store: SharedStore,
    workspace_root: PathBuf,
    artifact_store: Arc<dyn tetonic_domain::artifact::ArtifactStore>,
    placement_sink: Option<Arc<dyn tetonic_inference::DispatchPlacementSink>>,
    previous_pooled: Option<Arc<PooledProvider>>,
    coordinator: Arc<KeyPair>,
    owner: tetonic_memory::OwnerIdentityRow,
) -> Result<(Arc<PooledProvider>, Vec<Arc<RemoteNodeProvider>>), anyhow::Error> {
    if let Some(prev) = previous_pooled {
        prev.mark_all_remotes_revoked();
    }
    let policy_epoch = registry.policy_epoch();

    let dispatch_guard: Arc<dyn tetonic_inference::DispatchGuard> =
        Arc::new(PolicyDispatchGuard::new(policy.clone()));
    let guard_for_remotes = dispatch_guard.clone();
    let capability_registry = Arc::new(RwLock::new(CapabilityRegistry::new()));

    let concrete_remotes: Vec<Arc<RemoteNodeProvider>> = registry
        .list()
        .into_iter()
        .map(|t| {
            Arc::new(
                RemoteNodeProvider::new(
                    t.id,
                    t.label,
                    t.ip,
                    t.fabric_port,
                    t.fabric_tls_cert.to_vec(),
                    coordinator.clone(),
                    guard.clone(),
                    owner.id.clone(),
                    policy_epoch.clone(),
                )
                .with_capability_registry(capability_registry.clone())
                .with_worker_trust(t.worker_trust)
                .with_dispatch_state(workspace_root.clone(), artifact_store.clone())
                .with_dispatch_guard(guard_for_remotes.clone())
                .with_disposition_persistence(std::sync::Arc::new(
                    tetonic_fabric_client::StoreDispositionPersist::new(trust_store.clone()),
                )),
            )
        })
        .collect();
    let remotes: Vec<Arc<dyn tetonic_inference::FabricNodeProvider>> = concrete_remotes
        .iter()
        .map(|r| r.clone() as Arc<dyn tetonic_inference::FabricNodeProvider>)
        .collect();

    tracing::info!(
        "fabric pooled provider: {} remote worker(s) + local Ollama",
        remotes.len()
    );
    let mut pooled = PooledProvider::new_with_registry(
        local,
        remotes,
        policy_epoch,
        Arc::new(tetonic_inference::ActiveJobRegistry::new()),
    )
    .with_capability_registry(capability_registry)
    .with_worker_trust_resolver(Arc::new(move |worker_id| {
        let store = trust_store.clone();
        let id = worker_id.to_string();
        // Sync resolver called from the Infer path: do not pin a tokio worker
        // on SQLite I/O (H2-2). Prefer block_in_place when a runtime is present.
        let lookup = || {
            store
                .read_sync(move |db| {
                    let worker = db.find_worker_enrollment(&id).ok()??;
                    let trust = worker
                        .worker_trust
                        .as_deref()
                        .and_then(WorkerTrust::parse)
                        .unwrap_or(WorkerTrust::OwnerControlledEstate);
                    let epoch = db.max_worker_trust_policy_epoch().ok()?;
                    Some((trust, epoch))
                })
                .ok()
                .flatten()
        };
        match tokio::runtime::Handle::try_current() {
            Ok(_) => tokio::task::block_in_place(lookup),
            Err(_) => lookup(),
        }
    }))
    .with_dispatch_guard(dispatch_guard);
    if let Some(sink) = placement_sink {
        pooled = pooled.with_placement_sink(sink);
    }
    Ok((Arc::new(pooled), concrete_remotes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tetonic_domain::{
        Classification, ClassificationSource, DataClass, DispatchDecision, DispatchDestination,
        DispatchRequest,
    };
    fn plane_request() -> ComputePlaneRequest {
        let dir = std::env::temp_dir().join(format!("lokai-plane-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        ComputePlaneRequest {
            guard: Arc::new(EgressGuard::new()),
            ollama_base: "http://127.0.0.1:11434".into(),
            policy: Arc::new(PolicyEngine::default()),
            workspace_root: dir.clone(),
            artifact_store: Arc::new(
                tetonic_artifact::LocalArtifactStore::new(
                    dir.join("artifacts"),
                    crate::secret_scanner_factory::artifact_scan_policy(&None),
                )
                .unwrap(),
            ),
            store: None,
            coordinator: None,
            placement_sink: None,
            previous_pooled: None,
        }
    }

    #[tokio::test]
    async fn local_plane_wraps_broker_and_keeps_secret_local() {
        let plane = build_compute_plane(plane_request()).await;
        let name = std::any::type_name_of_val(plane.provider.as_ref());
        assert!(
            name.contains("BrokerInferenceProvider"),
            "CLI/daemon Infer must enter BrokerInferenceProvider, got {name}"
        );
        assert!(
            plane.provider.has_secret_scanner(),
            "build_compute_plane must attach ScannerEngine at BrokerInferenceProvider"
        );
        let pooled = plane.pooled.expect("local pooled");
        let guard = pooled
            .dispatch_guard()
            .expect("PolicyDispatchGuard required on CLI/daemon pooled provider");
        let decision = guard
            .evaluate(&DispatchRequest {
                payload: Some(Classification::new(
                    DataClass::Secret,
                    vec![ClassificationSource::UserDesignation],
                )),
                session: None,
                destination: DispatchDestination::RemoteWorker {
                    worker_id: "w_test".into(),
                },
                post_redaction: false,
                worker_trust: None,
                project_policy: Default::default(),
            })
            .expect("secret evaluate");
        assert_eq!(decision, DispatchDecision::LocalOnly);
        assert!(!decision.allows_remote());
    }
}

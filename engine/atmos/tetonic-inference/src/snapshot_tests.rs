//! Probe sharing is topology-independent and must not defeat invalidation.
use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use tokio::sync::Semaphore;

struct Probe {
    id: String,
    calls: AtomicUsize,
    release: Semaphore,
    healthy: AtomicBool,
    service_delay: Option<Duration>,
}

#[async_trait]
impl InferenceProvider for Probe {
    async fn fabric_snapshot(&self) -> FabricSnapshot {
        unreachable!("the pool uses probe_node")
    }

    async fn chat(
        &self,
        _: ChatRequest,
        _: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        unreachable!("status discovery must not perform inference")
    }
}

#[async_trait]
impl FabricNodeProvider for Probe {
    fn node_id(&self) -> &str {
        &self.id
    }
    fn label(&self) -> &str {
        &self.id
    }
    fn fabric_capabilities(&self) -> tetonic_fabric_protocol::WorkerCapabilityAdvertisement {
        tetonic_fabric_protocol::WorkerCapabilityAdvertisement::legacy_v1_chat_infer_only()
    }
    async fn probe_node(&self) -> Option<crate::NodeInfo> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let permit = self.release.acquire().await.unwrap();
        if let Some(delay) = self.service_delay {
            tokio::time::sleep(delay).await;
        } else {
            permit.forget();
        }
        self.healthy
            .load(Ordering::SeqCst)
            .then(|| crate::NodeInfo {
                id: self.id.clone(),
                label: self.id.clone(),
                vram_total_mb: 0,
                vram_free_mb: 0,
                resident_models: vec![],
                queue_depth: 0,
                healthy: true,
                models_verified: true,
                capacity: None,
                legacy_v1_chat_only: true,
                negotiated_protocol_version: None,
            })
    }
    async fn chat_on_fabric(
        &self,
        _: ChatRequest,
        _: &str,
        _: &str,
        _: Option<&FabricCallMeta>,
        _: Option<&str>,
        _: Option<&ActiveJobRegistry>,
        _: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        unreachable!("status discovery must not perform inference")
    }
}

fn fixture(count: usize) -> (Arc<PooledProvider>, Vec<Arc<Probe>>) {
    fixture_with_delay(count, None)
}

fn fixture_with_delay(
    count: usize,
    delay: Option<Duration>,
) -> (Arc<PooledProvider>, Vec<Arc<Probe>>) {
    // Unenrolled port: local discovery fails without touching a real runtime.
    let local = Arc::new(OllamaProvider::new(
        "http://127.0.0.1:9",
        Arc::new(tetonic_egress::EgressGuard::new()),
    ));
    let probes: Vec<_> = (0..count)
        .map(|i| {
            Arc::new(Probe {
                id: format!("worker-{i}"),
                calls: AtomicUsize::new(0),
                release: Semaphore::new(usize::from(delay.is_some())),
                healthy: AtomicBool::new(true),
                service_delay: delay,
            })
        })
        .collect();
    let pool = PooledProvider::new(
        local,
        probes
            .iter()
            .map(|p| p.clone() as Arc<dyn FabricNodeProvider>)
            .collect(),
    );
    (Arc::new(pool), probes)
}

#[tokio::test]
#[ignore = "simulated discovery contention; not inference/task acceleration"]
async fn benchmark_shared_cluster_discovery() {
    let (pool, probes) = fixture_with_delay(8, Some(Duration::from_millis(20)));
    let start = Instant::now();
    let old = join_all((0..16).map(|_| pool.refresh_fabric_snapshot(0))).await;
    let before = start.elapsed().as_secs_f64();
    assert_eq!(
        probes
            .iter()
            .map(|p| p.calls.load(Ordering::SeqCst))
            .sum::<usize>(),
        128
    );
    pool.invalidate_snapshot_cache();
    let start = Instant::now();
    let new = join_all((0..16).map(|_| pool.fabric_snapshot())).await;
    let after = start.elapsed().as_secs_f64();
    assert_eq!(
        probes
            .iter()
            .map(|p| p.calls.load(Ordering::SeqCst))
            .sum::<usize>(),
        136
    );
    assert!(old.iter().chain(new.iter()).all(|s| s.nodes.len() == 9));
    println!("16 readers / 8 workers / 20ms serialized probe service: unshared={:.2}ms shared={:.2}ms ratio={:.2}x; probes 128 -> 8; discovery only", before*1000.0, after*1000.0, before/after);
}

async fn wait_calls(probes: &[Arc<Probe>], count: usize) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while probes
            .iter()
            .any(|p| p.calls.load(Ordering::SeqCst) < count)
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("probe never started");
}

fn fetch(pool: &Arc<PooledProvider>) -> tokio::task::JoinHandle<FabricSnapshot> {
    let pool = pool.clone();
    tokio::spawn(async move { pool.fabric_snapshot().await })
}

#[tokio::test]
async fn sixteen_sessions_share_discovery_on_local_single_worker_and_eight_worker_topologies() {
    for nodes in [0, 1, 8] {
        let (pool, probes) = fixture(nodes);
        let jobs: Vec<_> = (0..16).map(|_| fetch(&pool)).collect();
        wait_calls(&probes, 1).await;
        for p in &probes {
            p.release.add_permits(1);
        }
        let snapshots = tokio::time::timeout(Duration::from_secs(5), join_all(jobs))
            .await
            .expect("duplicate probes waited for permits")
            .into_iter()
            .map(Result::unwrap)
            .collect::<Vec<_>>();
        assert!(snapshots.iter().all(|s| s == &snapshots[0]));
        assert_eq!(snapshots[0].nodes.len(), nodes + 1);
        assert_eq!(pool.snapshot_generation(), 1);
        assert!(probes.iter().all(|p| p.calls.load(Ordering::SeqCst) == 1));
    }
}

#[tokio::test]
async fn canceled_refresh_releases_waiters_without_installing_partial_state() {
    let (pool, probes) = fixture(1);
    let first = fetch(&pool);
    wait_calls(&probes, 1).await;
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    assert!(pool.snapshot_cache.read().unwrap().is_none());
    let second = fetch(&pool);
    wait_calls(&probes, 2).await;
    probes[0].release.add_permits(1);
    let snapshot = tokio::time::timeout(Duration::from_secs(5), second)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.nodes.len(), 2);
    assert_eq!(pool.snapshot_generation(), 1);
}

#[tokio::test]
async fn invalidation_during_probe_cannot_repopulate_stale_cache() {
    let (pool, probes) = fixture(1);
    let first = fetch(&pool);
    wait_calls(&probes, 1).await;
    pool.invalidate_snapshot_cache();
    probes[0].release.add_permits(1);
    first.await.unwrap();
    assert!(pool.snapshot_cache.read().unwrap().is_none());
    probes[0].healthy.store(false, Ordering::SeqCst);
    let second = fetch(&pool);
    wait_calls(&probes, 2).await;
    probes[0].release.add_permits(1);
    let snapshot = tokio::time::timeout(Duration::from_secs(5), second)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        snapshot.nodes.len(),
        1,
        "fresh discovery must reflect worker loss"
    );
    assert_eq!(pool.snapshot_generation(), 2);
}

#[tokio::test]
async fn revocation_during_probe_leaves_registry_invalidation_pending() {
    let (pool, probes) = fixture(1);
    let registry = Arc::new(RwLock::new(CapabilityRegistry::default()));
    let pool = Arc::new(
        Arc::try_unwrap(pool)
            .ok()
            .unwrap()
            .with_capability_registry(registry.clone()),
    );
    let first = fetch(&pool);
    wait_calls(&probes, 1).await;
    pool.on_revocation(1);
    probes[0].release.add_permits(1);
    first.await.unwrap();
    assert!(registry.read().unwrap().needs_snapshot_refresh());
    assert!(pool.snapshot_cache.read().unwrap().is_none());
}

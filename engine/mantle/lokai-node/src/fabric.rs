//! Fabric HTTP routes (`/v1/health`, `/v1/capabilities`, `/v1/models/verified`, `/v1/capacity/status`, `/v1/chat`, `/v1/revoke`, `/v1/owner/activity`).

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use ed25519_dalek::SigningKey;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::{Bytes, Incoming};
use hyper::{Method, Request, Response, StatusCode};
use lokai_capacity::worker_capacity_wire;
use lokai_domain::ids::{KeyId, WorkerId};
use lokai_egress::EgressGuard;
use lokai_fabric_client::{key_id_from_public, OwnerActivityRequest};
use lokai_fabric_protocol::{
    CancellationState, ControlSupport, ModelCapability, SandboxCapabilities as FabricSandboxCaps,
    WorkerCapabilities, WorkerCapabilityAdvertisement,
};
use lokai_inference::{DataClass, InferenceProvider, OllamaProvider};
use lokai_memory::WorkerStore;
use lokai_sandbox::platform_backend;
use rand::rngs::OsRng;
use serde_json::json;
use tokio::sync::Mutex;

use crate::event::IngressDecision;
use crate::job_ingress::JobIngressManager;
use crate::lease_table::LeaseTable;
use crate::limits::{BODY_TIMEOUT, MAX_FABRIC_BODY_BYTES};
use crate::revoke::RevokeRequest;
use crate::scheduler::WorkerScheduler;
use crate::tls::ed25519_pubkey_from_cert;
use crate::trust::TrustStore;

pub(crate) type FabricBody = http_body_util::combinators::BoxBody<Bytes, Infallible>;

pub struct FabricState {
    pub peer_id: String,
    pub coordinator_pk: [u8; 32],
    pub inference: Arc<dyn InferenceProvider + Send + Sync>,
    pub ollama: Arc<OllamaProvider>,
    pub ollama_base: String,
    pub trust: Arc<TrustStore>,
    pub worker_db_path: std::path::PathBuf,
    pub estate_id: String,
    pub scheduler: Arc<WorkerScheduler>,
    pub job_ingress: Arc<JobIngressManager>,
    pub boot_id: String,
    pub capability_revision: Arc<AtomicU64>,
    pub inventory_fingerprint: Arc<AtomicU64>,
    /// Dedicated result-signing key (M5-4); narrower than enrollment identity.
    pub result_signing_key: Arc<SigningKey>,
    pub result_key_id: KeyId,
    pub result_public_key: Vec<u8>,
    /// Typed-path cancellation / lease gate (R7-1).
    pub cancel_state: Arc<Mutex<CancellationState>>,
    /// attempt_id → job_id for in-flight typed jobs (cancel + lease lookup).
    pub attempt_jobs: Arc<Mutex<HashMap<String, String>>>,
    /// Soft leases with expiry; missed renewals cancel the job (R7-1 heartbeat).
    pub leases: Arc<Mutex<LeaseTable>>,
}

pub struct FabricOutcome {
    pub response: Response<FabricBody>,
    pub decision: IngressDecision,
    pub reason: String,
    pub route: String,
    pub peer_id: String,
}

pub async fn handle_with_audit(state: Arc<FabricState>, req: Request<Incoming>) -> FabricOutcome {
    let route = req.uri().path().to_string();
    let peer_id = state.peer_id.clone();
    let response = handle(state, req).await;
    let (decision, reason) = ingress_decision(response.status());
    FabricOutcome {
        response,
        decision,
        reason: reason.to_string(),
        route,
        peer_id,
    }
}

pub async fn handle(state: Arc<FabricState>, req: Request<Incoming>) -> Response<FabricBody> {
    let path = req.uri().path();
    let method = req.method();

    match (method, path) {
        (&Method::GET, "/v1/health") => health(&state).await,
        (&Method::GET, "/v1/capabilities") => capabilities(&state).await,
        (&Method::GET, "/v1/models/verified") => models_verified(&state).await,
        (&Method::GET, "/v1/capacity/status") => capacity_status(&state).await,
        (&Method::GET, "/v1/ledger") => not_implemented("ledger"),
        (&Method::POST, "/v1/chat") => crate::fabric_chat::chat(state, req).await,
        (&Method::POST, "/v1/jobs") => crate::fabric_chat::jobs(state, req).await,
        (&Method::POST, "/v1/jobs/cancel") => crate::fabric_chat::jobs_cancel(state, req).await,
        (&Method::POST, "/v1/jobs/lease") => crate::fabric_chat::jobs_lease(state, req).await,
        (&Method::POST, "/v1/revoke") => revoke(state, req).await,
        (&Method::POST, "/v1/owner/activity") => owner_activity(&state, req).await,
        (&Method::POST, "/v1/estate/policy") => not_implemented("estate_policy"),
        (&Method::POST, "/v1/negotiate") => negotiate(req).await,
        _ => json_response(
            StatusCode::NOT_FOUND,
            json!({"ok": false, "error": "not found"}),
        ),
    }
}

fn not_implemented(feature: &str) -> Response<FabricBody> {
    json_response(
        StatusCode::NOT_IMPLEMENTED,
        json!({
            "ok": false,
            "error": format!("{feature} not implemented on worker (N1+)"),
            "code": "not_implemented",
        }),
    )
}

fn ingress_decision(status: StatusCode) -> (IngressDecision, &'static str) {
    match status {
        StatusCode::FORBIDDEN
        | StatusCode::SERVICE_UNAVAILABLE
        | StatusCode::PAYLOAD_TOO_LARGE
        | StatusCode::REQUEST_TIMEOUT => (IngressDecision::Deny, "policy_denied"),
        _ => (IngressDecision::Allow, "authorized"),
    }
}

pub(crate) enum BodyCollectError {
    BadBody,
    TooLarge,
    Timeout,
}

pub(crate) async fn collect_limited_body(
    req: Request<Incoming>,
) -> Result<Bytes, BodyCollectError> {
    let limited = Limited::new(req.into_body(), MAX_FABRIC_BODY_BYTES);
    let collected = tokio::time::timeout(BODY_TIMEOUT, limited.collect())
        .await
        .map_err(|_| BodyCollectError::Timeout)?
        .map_err(|e| {
            if e.to_string().contains("limit") {
                BodyCollectError::TooLarge
            } else {
                BodyCollectError::BadBody
            }
        })?;
    Ok(collected.to_bytes())
}

pub(crate) fn body_limit_response(err: BodyCollectError) -> Response<FabricBody> {
    match err {
        BodyCollectError::Timeout => json_response(
            StatusCode::REQUEST_TIMEOUT,
            json!({"ok": false, "error": "request body deadline exceeded", "code": "request_timeout"}),
        ),
        BodyCollectError::TooLarge => json_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            json!({
                "ok": false,
                "error": format!("body exceeds max size ({MAX_FABRIC_BODY_BYTES} bytes)"),
                "code": "payload_too_large",
            }),
        ),
        BodyCollectError::BadBody => json_response(
            StatusCode::BAD_REQUEST,
            json!({"ok": false, "error": "bad body"}),
        ),
    }
}

async fn health(state: &FabricState) -> Response<FabricBody> {
    let reachable = state.ollama.reachable().await;
    let queue_depth = state.scheduler.queue_depth();
    json_response(
        StatusCode::OK,
        json!({
            "ok": reachable,
            "peer_id": state.peer_id,
            "queue_depth": queue_depth,
            "healthy": reachable,
            "owner_active": state.scheduler.owner_active().await,
        }),
    )
}

async fn capabilities(state: &FabricState) -> Response<FabricBody> {
    let snap = state.ollama.local_fabric_snapshot().await;
    let node = snap.nodes.first();
    let (vram_total, vram_free, resident) = node
        .map(|n| (n.vram_total_mb, n.vram_free_mb, n.resident_models.clone()))
        .unwrap_or_default();
    let resident_for_inventory = if resident.is_empty() {
        node.map(|n| n.resident_models.clone()).unwrap_or_default()
    } else {
        resident.clone()
    };
    let model_inventory = state
        .ollama
        .model_inventory_capabilities(&resident_for_inventory)
        .await;
    let models: Vec<String> = model_inventory
        .iter()
        .map(|m| m.local_name.clone())
        .collect();
    let fingerprint = inventory_fingerprint(&model_inventory);
    let prev_fp = state.inventory_fingerprint.load(Ordering::Relaxed);
    if prev_fp != 0 && prev_fp != fingerprint {
        state.capability_revision.fetch_add(1, Ordering::Relaxed);
    }
    state
        .inventory_fingerprint
        .store(fingerprint, Ordering::Relaxed);
    let queue_depth = state.scheduler.queue_depth();
    let active_jobs = queue_depth;
    let revision = state.capability_revision.load(Ordering::Relaxed).max(1);
    let mut worker_caps = WorkerCapabilities::typed_infer_profile(
        WorkerId::new(&state.peer_id),
        state.boot_id.clone(),
        revision,
        0,
        &models,
        &resident_for_inventory,
        vram_total,
        vram_free,
        active_jobs,
        queue_depth,
        1,
    );
    worker_caps.model_inventory = model_inventory;
    worker_caps.sandbox = fabric_sandbox_from_platform();
    let protocol_caps = worker_caps
        .legacy_advertisement
        .clone()
        .unwrap_or_else(WorkerCapabilityAdvertisement::legacy_v1_chat_infer_only);
    json_response(
        StatusCode::OK,
        json!({
            "ok": snap.effective_concurrency > 0,
            "models": models,
            "vram_total_mb": vram_total,
            "vram_free_mb": vram_free,
            "resident_models": resident,
            "fabric_protocol": protocol_caps,
            "worker_capabilities": worker_caps,
        }),
    )
}

async fn models_verified(state: &FabricState) -> Response<FabricBody> {
    match state.ollama.list_model_tags().await {
        Ok(tags) => {
            let models: Vec<_> = tags
                .into_iter()
                .map(|tag| {
                    let mut entry = json!({ "name": tag.name });
                    if let Some(digest) = tag.digest {
                        entry["digest"] = json!(digest);
                    }
                    entry
                })
                .collect();
            json_response(StatusCode::OK, json!({ "ok": true, "models": models }))
        }
        Err(_) => json_response(StatusCode::OK, json!({ "ok": false, "models": [] })),
    }
}

fn bool_to_control(enforced: bool) -> ControlSupport {
    if enforced {
        ControlSupport::Enforced
    } else {
        ControlSupport::Unsupported
    }
}

/// Map platform sandbox capabilities into the fabric advertisement document.
pub(crate) fn fabric_sandbox_from_platform() -> FabricSandboxCaps {
    let caps = platform_backend().capabilities();
    FabricSandboxCaps {
        process_tree: bool_to_control(caps.process_tree_containment),
        runtime_limits: bool_to_control(caps.runtime_enforcement || caps.output_limits),
        memory_limits: bool_to_control(caps.memory_limits),
        filesystem_read_scope: bool_to_control(caps.filesystem_read_restrictions),
        filesystem_write_scope: bool_to_control(caps.filesystem_write_restrictions),
        network_denial: bool_to_control(caps.network_denial),
        network_allowlist: bool_to_control(caps.network_allowlisting),
        environment_filtering: bool_to_control(caps.environment_filtering),
    }
}

async fn capacity_status(state: &FabricState) -> Response<FabricBody> {
    let db_path = state.worker_db_path.clone();
    let ollama_base = state.ollama_base.clone();
    let client: Arc<dyn lokai_capacity::InferenceClient> = Arc::new(
        lokai_capacity::OllamaInferenceClient::new(&ollama_base, state.ollama.clone()),
    );
    let ollama_version = if client.reachable().await {
        client.version().await
    } else {
        None
    };
    let (status, _) =
        lokai_capacity::capacity_status_for_worker_path(db_path, client, ollama_version).await;
    json_response(StatusCode::OK, json!(worker_capacity_wire(&status)))
}

async fn owner_activity(state: &FabricState, req: Request<Incoming>) -> Response<FabricBody> {
    let body = match collect_limited_body(req).await {
        Ok(b) => b,
        Err(e) => return body_limit_response(e),
    };
    let req: OwnerActivityRequest =
        serde_json::from_slice(&body).unwrap_or(OwnerActivityRequest { ttl_sec: 30 });
    state
        .scheduler
        .signal_owner_activity(std::time::Duration::from_secs(req.ttl_sec.max(1)))
        .await;
    json_response(StatusCode::OK, json!({"ok": true, "ttl_sec": req.ttl_sec}))
}

async fn revoke(state: Arc<FabricState>, req: Request<Incoming>) -> Response<FabricBody> {
    let body = match collect_limited_body(req).await {
        Ok(b) => b,
        Err(e) => return body_limit_response(e),
    };
    let Ok(revoke): Result<RevokeRequest, _> = serde_json::from_slice(&body) else {
        return json_response(
            StatusCode::BAD_REQUEST,
            json!({"ok": false, "error": "invalid json"}),
        );
    };

    if revoke.coordinator_pubkey.0.len() != 32 {
        return json_response(
            StatusCode::BAD_REQUEST,
            json!({"ok": false, "error": "bad coordinator pubkey"}),
        );
    }
    let mut target = [0u8; 32];
    target.copy_from_slice(&revoke.coordinator_pubkey.0);

    if target != state.coordinator_pk {
        return json_response(
            StatusCode::FORBIDDEN,
            json!({"ok": false, "error": "revoke must match authenticated coordinator"}),
        );
    }

    let estate_id = state.estate_id.clone();
    let db_path = state.worker_db_path.clone();
    let epoch = revoke.epoch;
    let db_result = tokio::task::spawn_blocking(move || {
        let store = WorkerStore::open(&db_path)?;
        store.bump_coordinator_epoch(&estate_id, &target, epoch)?;
        store.remove_coordinator_pin(&estate_id, &target)?;
        Ok::<(), lokai_memory::WorkerStoreError>(())
    })
    .await;

    match db_result {
        Ok(Ok(())) => {
            state.trust.revoke_pubkey(&target);
            state.scheduler.cancel_running().await;
            json_response(StatusCode::OK, json!({"ok": true, "epoch": revoke.epoch}))
        }
        Ok(Err(e)) => {
            tracing::error!("revoke db update failed: {e}");
            json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({
                    "ok": false,
                    "error": format!("revoke persistence failed: {e}"),
                    "code": "revoke_db_failed",
                }),
            )
        }
        Err(e) => {
            tracing::error!("revoke db task join failed: {e}");
            json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({
                    "ok": false,
                    "error": "revoke persistence task failed",
                    "code": "revoke_db_failed",
                }),
            )
        }
    }
}

pub(crate) fn json_response(status: StatusCode, body: serde_json::Value) -> Response<FabricBody> {
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(
            Full::new(Bytes::from(body.to_string()))
                .map_err(|never: Infallible| match never {})
                .boxed(),
        )
        .unwrap()
}

pub fn default_ollama(base: &str) -> Arc<OllamaProvider> {
    Arc::new(OllamaProvider::new(
        base,
        Arc::new(EgressGuard::pinned_to_inference_url(base)),
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn fabric_state(
    peer_id: String,
    coordinator_pk: [u8; 32],
    ollama: Arc<OllamaProvider>,
    ollama_base: String,
    trust: Arc<TrustStore>,
    worker_db_path: std::path::PathBuf,
    estate_id: String,
    scheduler: Arc<WorkerScheduler>,
) -> Arc<FabricState> {
    let inference: Arc<dyn InferenceProvider + Send + Sync> = ollama.clone();
    let job_ingress = JobIngressManager::load(worker_db_path.clone());
    let boot_id = format!(
        "boot_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let result_signing_key = SigningKey::generate(&mut OsRng);
    let result_public_key = result_signing_key.verifying_key().to_bytes().to_vec();
    let result_key_id = key_id_from_public(&result_public_key);
    Arc::new(FabricState {
        peer_id,
        coordinator_pk,
        inference,
        ollama,
        ollama_base,
        trust,
        worker_db_path,
        estate_id,
        scheduler,
        job_ingress,
        boot_id,
        capability_revision: Arc::new(AtomicU64::new(1)),
        inventory_fingerprint: Arc::new(AtomicU64::new(0)),
        result_signing_key: Arc::new(result_signing_key),
        result_key_id,
        result_public_key,
        cancel_state: Arc::new(Mutex::new(CancellationState::default())),
        attempt_jobs: Arc::new(Mutex::new(HashMap::new())),
        leases: Arc::new(Mutex::new(LeaseTable::default())),
    })
}

fn inventory_fingerprint(inventory: &[ModelCapability]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for model in inventory {
        model.local_name.hash(&mut hasher);
        model.model_digest.hash(&mut hasher);
        model.quantization.hash(&mut hasher);
    }
    hasher.finish()
}

pub fn coordinator_pk_from_tls_certs(
    certs: &[rustls::pki_types::CertificateDer<'_>],
) -> Option<[u8; 32]> {
    certs
        .first()
        .and_then(|c| ed25519_pubkey_from_cert(c.as_ref()).ok())
}

/// Worker-side mirror of coordinator policy (M2-2): never execute secret-class jobs remotely.
pub fn worker_accepts_data_class(class: DataClass) -> bool {
    !matches!(class, DataClass::Secret)
}

pub(crate) async fn reject_stale_policy_epoch(
    state: &FabricState,
    job_epoch: u64,
) -> Option<Response<FabricBody>> {
    let estate_id = state.estate_id.clone();
    let db_path = state.worker_db_path.clone();
    let pk = state.coordinator_pk;
    let pinned = tokio::task::spawn_blocking(move || {
        let store = WorkerStore::open(&db_path)?;
        store.coordinator_pin_epoch(&estate_id, &pk)
    })
    .await
    .ok()
    .and_then(|r| r.ok())
    .flatten();
    let Some(min_epoch) = pinned else {
        return Some(json_response(
            StatusCode::FORBIDDEN,
            json!({"ok": false, "error": "coordinator not enrolled"}),
        ));
    };
    if job_epoch < min_epoch {
        return Some(json_response(
            StatusCode::FORBIDDEN,
            json!({
                "ok": false,
                "error": format!(
                    "policy epoch {} below pinned {}",
                    job_epoch, min_epoch
                ),
                "code": "stale_policy_epoch",
            }),
        ));
    }
    None
}

async fn negotiate(req: Request<Incoming>) -> Response<FabricBody> {
    let body = match collect_limited_body(req).await {
        Ok(b) => b,
        Err(e) => return body_limit_response(e),
    };
    match negotiate_from_body(&body) {
        Ok(resp) => json_response(StatusCode::OK, serde_json::to_value(resp).unwrap()),
        Err(e) => json_response(
            StatusCode::BAD_REQUEST,
            json!({
                "ok": false,
                "error": e.message,
                "code": e.code,
            }),
        ),
    }
}

/// Production negotiate path: decode peer offer and agree a version (R7-3).
fn negotiate_from_body(
    body: &[u8],
) -> Result<lokai_fabric_protocol::VersionNegotiationResponse, lokai_fabric_protocol::FabricError> {
    use lokai_fabric_protocol::{
        negotiate_versions, FabricError, FabricErrorCode, VersionNegotiationRequest,
        MAX_SUPPORTED_VERSION, MIN_SUPPORTED_VERSION,
    };
    let negotiation_req: VersionNegotiationRequest =
        serde_json::from_slice(body).map_err(|_| FabricError {
            code: FabricErrorCode::InvalidEnvelope,
            message: "invalid negotiate request json".into(),
            details: None,
        })?;
    negotiate_versions(
        &negotiation_req,
        MIN_SUPPORTED_VERSION,
        MAX_SUPPORTED_VERSION,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use lokai_fabric_protocol::{
        default_message_limits, FabricErrorCode, VersionNegotiationRequest,
        IMMUTABLE_SECURITY_FEATURES, MAX_SUPPORTED_VERSION, MIN_SUPPORTED_VERSION,
        PROTOCOL_VERSION,
    };

    #[test]
    fn worker_rejects_secret_data_class() {
        assert!(!worker_accepts_data_class(DataClass::Secret));
        assert!(worker_accepts_data_class(DataClass::RepositorySource));
        assert!(worker_accepts_data_class(DataClass::SensitiveSource));
    }

    #[test]
    fn negotiate_matching_versions_records_agreed_version() {
        let req = VersionNegotiationRequest {
            min_supported_version: MIN_SUPPORTED_VERSION,
            max_supported_version: MAX_SUPPORTED_VERSION,
            required_features: IMMUTABLE_SECURITY_FEATURES
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            optional_features: vec![],
            software_version: "test".into(),
            message_size_limits: default_message_limits(),
        };
        let body = serde_json::to_vec(&req).unwrap();
        let resp = negotiate_from_body(&body).expect("match");
        assert_eq!(resp.negotiated_version, PROTOCOL_VERSION);
        for feat in IMMUTABLE_SECURITY_FEATURES {
            assert!(resp.active_features.iter().any(|f| f == feat));
        }
    }

    #[test]
    fn negotiate_skewed_peer_version_fails_closed() {
        let req = VersionNegotiationRequest {
            min_supported_version: MAX_SUPPORTED_VERSION + 1,
            max_supported_version: MAX_SUPPORTED_VERSION + 1,
            required_features: IMMUTABLE_SECURITY_FEATURES
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            optional_features: vec![],
            software_version: "skew".into(),
            message_size_limits: default_message_limits(),
        };
        let body = serde_json::to_vec(&req).unwrap();
        let err = negotiate_from_body(&body).expect_err("skew");
        assert_eq!(err.code, FabricErrorCode::UnsupportedProtocolVersion);
    }

    #[test]
    fn ingress_decision_maps_policy_statuses() {
        assert_eq!(
            ingress_decision(StatusCode::FORBIDDEN).0,
            IngressDecision::Deny
        );
        assert_eq!(ingress_decision(StatusCode::OK).0, IngressDecision::Allow);
    }

    #[test]
    fn advertised_network_denial_matches_platform_backend() {
        let fabric = fabric_sandbox_from_platform();
        let platform = platform_backend().capabilities();
        let expected = if platform.network_denial {
            ControlSupport::Enforced
        } else {
            ControlSupport::Unsupported
        };
        assert_eq!(fabric.network_denial, expected);
        assert_eq!(
            fabric.process_tree,
            bool_to_control(platform.process_tree_containment)
        );
        assert_eq!(
            fabric.environment_filtering,
            bool_to_control(platform.environment_filtering)
        );
    }

    #[tokio::test]
    async fn models_verified_route_returns_list_tags_shape() {
        let ollama = default_ollama("http://127.0.0.1:1");
        let db_path = std::env::temp_dir().join(format!(
            "lokai_node_verified_{}.db",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let result_signing_key = SigningKey::generate(&mut OsRng);
        let result_public_key = result_signing_key.verifying_key().to_bytes().to_vec();
        let result_key_id = key_id_from_public(&result_public_key);
        let state = Arc::new(FabricState {
            peer_id: "test_peer".into(),
            coordinator_pk: [1u8; 32],
            inference: ollama.clone(),
            ollama,
            ollama_base: "http://127.0.0.1:1".into(),
            trust: Arc::new(
                TrustStore::from_pinned_pubkeys(std::iter::once([1u8; 32].to_vec())).unwrap(),
            ),
            worker_db_path: db_path.clone(),
            estate_id: "estate".into(),
            scheduler: Arc::new(WorkerScheduler::new()),
            job_ingress: JobIngressManager::load(db_path),
            boot_id: "boot_test".into(),
            capability_revision: Arc::new(AtomicU64::new(1)),
            inventory_fingerprint: Arc::new(AtomicU64::new(0)),
            result_signing_key: Arc::new(result_signing_key),
            result_key_id,
            result_public_key,
            cancel_state: Arc::new(Mutex::new(CancellationState::default())),
            attempt_jobs: Arc::new(Mutex::new(HashMap::new())),
            leases: Arc::new(Mutex::new(crate::lease_table::LeaseTable::default())),
        });
        let resp = models_verified(&state).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = http_body_util::BodyExt::collect(resp.into_body())
            .await
            .unwrap()
            .to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(v.get("ok").and_then(|o| o.as_bool()).is_some());
        assert!(v.get("models").and_then(|m| m.as_array()).is_some());
    }

    #[test]
    fn default_ollama_allow_rules_contain_configured_bind() {
        let loopback = default_ollama("http://127.0.0.1:11434");
        assert!(
            loopback
                .egress_guard()
                .allow_rules()
                .iter()
                .any(|r| r.ip == "127.0.0.1".parse::<std::net::IpAddr>().unwrap()
                    && r.port == Some(11434)),
            "worker Infer client must pin the configured loopback bind"
        );
        let remote = default_ollama("http://10.0.0.9:11434");
        assert!(
            remote
                .egress_guard()
                .allow_rules()
                .iter()
                .any(|r| r.ip == "10.0.0.9".parse::<std::net::IpAddr>().unwrap()
                    && r.port == Some(11434)),
            "worker Infer client must pin a non-loopback Ollama bind"
        );
    }
}

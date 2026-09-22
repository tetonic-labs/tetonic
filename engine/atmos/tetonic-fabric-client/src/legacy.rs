//! Remote worker inference via fabric-transport-v1 (`POST /v1/chat`).

use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use tetonic_domain::result_integrity::WorkerBehaviorSignals;
use tetonic_domain::ResultVerificationRequirement;
use tetonic_domain::{artifact::ArtifactStore, ids::WorkerId, WorkerTrust};
use tetonic_egress::EgressGuard;
use tetonic_enroll::KeyPair;
use tetonic_fabric_protocol::{
    default_message_limits, ArtifactReference, CancellationRequest, DispositionStore,
    FabricTraceContext, IdempotencyKey, JobRequirements, LeaseRenewalRequest, LifecycleContext,
    ProtocolVersion, ResultSigningKeyRegistry, VersionNegotiationRequest,
    VersionNegotiationResponse, WorkerCapabilities, WorkerCapabilityAdvertisement,
    IMMUTABLE_SECURITY_FEATURES, MAX_SUPPORTED_VERSION, MIN_SUPPORTED_VERSION,
};

use crate::capability_registry::CapabilityRegistry;

use crate::client::{fabric_request, fabric_request_ndjson, FabricClientError, FabricHttpResponse};
use crate::legacy_adapter::{
    build_infer_job_envelope, infer_job_to_legacy_chat, LegacyInferParams,
};
pub(crate) use crate::legacy_result::validate_result_identity;
use crate::legacy_result::{
    deliver_stream_after_accept, handle_chat_response_body, AcceptedChatTiming, ChatResponseBody,
};
use crate::lifecycle_client::{deliver_cancellation, request_lease_renewal};
use crate::protocol::{
    default_limits, encode_cancellation_envelope, encode_job_offer_envelope,
    encode_lease_renewal_envelope, legacy_worker_capabilities, validate_outbound_job,
    validate_revoked,
};
use crate::verification::requirement_from_placement;
use tetonic_inference::{
    aggregate_chat_classification, evaluate_remote_dispatch, evaluate_typed_job_placement,
    new_fabric_attempt_id, new_fabric_job_id, require_outbound_scan, ActiveJobRegistry,
    ChatRequest, ChatResponse, DataClass, DisclosureTier, DispatchGuard, FabricCallMeta,
    FabricJobResult, FabricNodeProvider, FabricSnapshot, GenUsage, InferenceError,
    InferenceProvenance, InferenceProvider, JobStatus, NodeCapacityHealth, NodeInfo, SampleOptions,
    TokenSink, NETWORK_DENY_ALL_CAPABILITY,
};

/// Session agreed during `/v1/negotiate` (R7-3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NegotiatedSession {
    pub negotiated_version: u32,
    pub active_features: Vec<String>,
}

/// One enrolled worker reachable over mTLS fabric (typed `/v1/jobs` + legacy `/v1/chat`).
pub struct RemoteNodeProvider {
    pub worker_id: String,
    pub label: String,
    ip: IpAddr,
    port: u16,
    server_cert: Vec<u8>,
    coordinator: Arc<KeyPair>,
    guard: Arc<EgressGuard>,
    pub(crate) estate_id: String,
    policy_epoch: Arc<AtomicU64>,
    dispatch_guard: Option<Arc<dyn DispatchGuard>>,
    capabilities: Arc<RwLock<WorkerCapabilityAdvertisement>>,
    pub(crate) capability_registry: Arc<RwLock<CapabilityRegistry>>,
    worker_trust: RwLock<WorkerTrust>,
    trust_resolved: AtomicBool,
    worker_revoked: Arc<AtomicBool>,
    pub(crate) dispatch_workspace_root: Option<PathBuf>,
    artifact_store: Option<Arc<dyn ArtifactStore>>,
    pub(crate) result_keys: Arc<RwLock<ResultSigningKeyRegistry>>,
    pub(crate) result_dispositions: Arc<RwLock<DispositionStore>>,
    pub(crate) behavior_signals: Arc<RwLock<WorkerBehaviorSignals>>,
    pub(crate) disposition_persist: Option<crate::legacy_result::ArcDispositionPersistence>,
    pub(crate) run_bridge: RwLock<Option<crate::legacy_result::ArcRunBridge>>,
    /// Protocol version agreed via `/v1/negotiate`. `None` until probe succeeds.
    negotiated: Arc<RwLock<Option<NegotiatedSession>>>,
}

impl RemoteNodeProvider {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        worker_id: String,
        label: String,
        ip: IpAddr,
        port: u16,
        server_cert: Vec<u8>,
        coordinator: Arc<KeyPair>,
        guard: Arc<EgressGuard>,
        estate_id: String,
        policy_epoch: Arc<AtomicU64>,
    ) -> Self {
        Self {
            worker_id,
            label,
            ip,
            port,
            server_cert,
            coordinator,
            guard,
            estate_id,
            policy_epoch,
            dispatch_guard: None,
            capabilities: Arc::new(RwLock::new(legacy_worker_capabilities())),
            capability_registry: Arc::new(RwLock::new(CapabilityRegistry::new())),
            worker_trust: RwLock::new(WorkerTrust::OwnerControlledEstate),
            trust_resolved: AtomicBool::new(false),
            worker_revoked: Arc::new(AtomicBool::new(false)),
            dispatch_workspace_root: None,
            artifact_store: None,
            result_keys: Arc::new(RwLock::new(ResultSigningKeyRegistry::new())),
            result_dispositions: Arc::new(RwLock::new(DispositionStore::new())),
            behavior_signals: Arc::new(RwLock::new(WorkerBehaviorSignals::default())),
            disposition_persist: None,
            run_bridge: RwLock::new(None),
            negotiated: Arc::new(RwLock::new(None)),
        }
    }

    /// Agreed fabric protocol version from the last successful `/v1/negotiate`.
    pub fn negotiated_protocol_version(&self) -> Option<u32> {
        self.negotiated
            .read()
            .ok()
            .and_then(|g| g.as_ref().map(|s| s.negotiated_version))
    }

    /// Full negotiated session (version + active features), if any.
    pub fn negotiated_session(&self) -> Option<NegotiatedSession> {
        self.negotiated.read().ok().and_then(|g| g.clone())
    }

    pub fn with_worker_trust(self, trust: WorkerTrust) -> Self {
        *self.worker_trust.write().unwrap() = trust;
        self.trust_resolved.store(true, Ordering::SeqCst);
        self
    }

    /// Late-bind RunSupervisor authority after the application kernel is constructed.
    pub fn set_run_bridge(&self, bridge: crate::legacy_result::ArcRunBridge) {
        if let Ok(mut slot) = self.run_bridge.write() {
            *slot = Some(bridge);
        }
    }

    /// Coordinator-owned sources used for final workspace/artifact binding checks.
    pub fn with_dispatch_state(
        mut self,
        workspace_root: PathBuf,
        artifact_store: Arc<dyn ArtifactStore>,
    ) -> Self {
        self.dispatch_workspace_root = Some(workspace_root);
        self.artifact_store = Some(artifact_store);
        self
    }

    pub fn set_worker_trust(&self, trust: WorkerTrust) {
        *self.worker_trust.write().unwrap() = trust;
        self.trust_resolved.store(true, Ordering::SeqCst);
    }

    pub fn worker_trust(&self) -> WorkerTrust {
        *self.worker_trust.read().unwrap()
    }

    pub fn fabric_capabilities(&self) -> WorkerCapabilityAdvertisement {
        self.capabilities
            .read()
            .map(|g| g.clone())
            .unwrap_or_else(|_| legacy_worker_capabilities())
    }

    /// Test/helper: force cached advertisement (e.g. typed vs legacy).
    pub fn set_fabric_capabilities(&self, caps: WorkerCapabilityAdvertisement) {
        if let Ok(mut g) = self.capabilities.write() {
            *g = caps;
        }
    }

    /// Mark worker identity revoked — in-flight results will be rejected (M5-1).
    pub fn mark_worker_revoked(&self) {
        self.worker_revoked.store(true, Ordering::Release);
        if let Ok(mut keys) = self.result_keys.write() {
            keys.revoke_worker(&WorkerId::new(&self.worker_id));
        }
    }

    pub fn is_worker_revoked(&self) -> bool {
        self.worker_revoked.load(Ordering::Acquire)
    }

    pub fn capability_registry(&self) -> Arc<RwLock<CapabilityRegistry>> {
        self.capability_registry.clone()
    }

    pub fn with_capability_registry(mut self, registry: Arc<RwLock<CapabilityRegistry>>) -> Self {
        self.capability_registry = registry;
        self
    }

    pub fn with_dispatch_guard(mut self, guard: Arc<dyn DispatchGuard>) -> Self {
        self.dispatch_guard = Some(guard);
        self
    }

    async fn revalidate_dispatch_state(
        &self,
        job: &tetonic_fabric_protocol::JobEnvelope,
    ) -> Result<(), InferenceError> {
        if let Some(expected) = &job.workspace_version {
            let root = self.dispatch_workspace_root.clone().ok_or_else(|| {
                InferenceError::Provider(
                    "workspace-bound remote dispatch has no coordinator workspace source".into(),
                )
            })?;
            let actual = tokio::task::spawn_blocking(move || {
                tetonic_transaction::version::capture_workspace_version(&root, &[])
            })
            .await
            .map_err(|error| {
                InferenceError::Provider(format!("workspace revalidation task failed: {error}"))
            })?
            .map_err(|error| {
                InferenceError::Provider(format!("workspace revalidation failed: {error}"))
            })?;
            if &actual != expected {
                return Err(InferenceError::Provider(
                    "workspace changed after placement; remote dispatch canceled".into(),
                ));
            }
        }

        if !job.input_artifacts.is_empty() {
            let store = self.artifact_store.as_ref().ok_or_else(|| {
                InferenceError::Provider(
                    "artifact-bearing remote dispatch has no coordinator artifact source".into(),
                )
            })?;
            for expected in &job.input_artifacts {
                let id = tetonic_domain::ids::ArtifactId::new(&expected.artifact_id);
                let metadata = store.metadata(&id).await.map_err(|error| {
                    InferenceError::Provider(format!(
                        "artifact {} revalidation failed: {error}",
                        expected.artifact_id
                    ))
                })?;
                if metadata.content_digest != expected.digest {
                    return Err(InferenceError::Provider(format!(
                        "artifact {} changed after placement; remote dispatch canceled",
                        expected.artifact_id
                    )));
                }
            }
        }
        Ok(())
    }

    pub fn policy_epoch(&self) -> Arc<AtomicU64> {
        self.policy_epoch.clone()
    }

    pub async fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
        initiator: &str,
    ) -> Result<FabricHttpResponse, InferenceError> {
        fabric_request(
            &self.guard,
            self.ip,
            self.port,
            &self.server_cert,
            &self.coordinator,
            method,
            path,
            body,
            initiator,
        )
        .await
        .map_err(map_fabric_err)
    }

    /// Build the coordinator `/v1/negotiate` request (R7-3).
    pub(crate) fn version_negotiation_request() -> VersionNegotiationRequest {
        VersionNegotiationRequest {
            min_supported_version: MIN_SUPPORTED_VERSION,
            max_supported_version: MAX_SUPPORTED_VERSION,
            required_features: IMMUTABLE_SECURITY_FEATURES
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            optional_features: vec![],
            software_version: env!("CARGO_PKG_VERSION").to_string(),
            message_size_limits: default_message_limits(),
        }
    }

    fn clear_negotiation(&self) {
        if let Ok(mut slot) = self.negotiated.write() {
            *slot = None;
        }
    }

    pub(crate) fn record_negotiation(&self, resp: VersionNegotiationResponse) {
        if let Ok(mut slot) = self.negotiated.write() {
            *slot = Some(NegotiatedSession {
                negotiated_version: resp.negotiated_version,
                active_features: resp.active_features,
            });
        }
    }

    /// POST `/v1/negotiate` and record the agreed version. Fail-closed on mismatch.
    async fn run_negotiate(&self) -> bool {
        let req = Self::version_negotiation_request();
        let Ok(body) = serde_json::to_string(&req) else {
            self.clear_negotiation();
            return false;
        };
        match self
            .request("POST", "/v1/negotiate", Some(&body), "fabric:negotiate")
            .await
        {
            Ok(r) if r.status == 200 => {
                match serde_json::from_str::<VersionNegotiationResponse>(&r.body) {
                    Ok(resp) => {
                        self.record_negotiation(resp);
                        true
                    }
                    Err(_) => {
                        self.clear_negotiation();
                        false
                    }
                }
            }
            _ => {
                self.clear_negotiation();
                false
            }
        }
    }

    /// POST typed `/v1/jobs/cancel` after protocol validation (R7-1).
    pub fn spawn_jobs_cancel(&self, cancel: CancellationRequest) {
        let caps = self.fabric_capabilities();
        if deliver_cancellation(&caps, &cancel).is_err() {
            return;
        }
        let worker_id = WorkerId::new(&self.worker_id);
        let envelope = encode_cancellation_envelope(
            tetonic_fabric_protocol::CoordinatorId(self.estate_id.clone()),
            worker_id,
            cancel,
            self.policy_epoch.load(Ordering::Relaxed),
        );
        let Ok(body) = serde_json::to_string(&envelope) else {
            return;
        };
        let guard = self.guard.clone();
        let ip = self.ip;
        let port = self.port;
        let server_cert = self.server_cert.clone();
        let coordinator = self.coordinator.clone();
        tokio::spawn(async move {
            let _ = fabric_request(
                &guard,
                ip,
                port,
                &server_cert,
                &coordinator,
                "POST",
                "/v1/jobs/cancel",
                Some(&body),
                "fabric:jobs_cancel",
            )
            .await;
        });
    }
}

#[async_trait]
impl InferenceProvider for RemoteNodeProvider {
    async fn fabric_snapshot(&self) -> FabricSnapshot {
        FabricSnapshot {
            nodes: self.probe_node().await.into_iter().collect(),
            effective_concurrency: 1,
            generated_at: Utc::now(),
        }
    }

    async fn chat(
        &self,
        req: ChatRequest,
        on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        require_outbound_scan(&req)?;
        let job_id = new_fabric_job_id();
        let attempt_id = req
            .fabric
            .as_ref()
            .and_then(|f| f.attempt_id.clone())
            .filter(|id| !id.is_empty())
            .unwrap_or_else(new_fabric_attempt_id);
        self.chat_on_fabric(
            req,
            &job_id,
            &attempt_id,
            None,
            Some(self.node_id()),
            None,
            on_token,
        )
        .await
    }
}

#[async_trait]
impl FabricNodeProvider for RemoteNodeProvider {
    fn node_id(&self) -> &str {
        &self.worker_id
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn fabric_capabilities(&self) -> WorkerCapabilityAdvertisement {
        RemoteNodeProvider::fabric_capabilities(self)
    }

    fn worker_trust(&self) -> WorkerTrust {
        RemoteNodeProvider::worker_trust(self)
    }

    fn worker_trust_resolved(&self) -> bool {
        self.trust_resolved.load(Ordering::SeqCst)
    }

    fn set_worker_trust(&self, trust: WorkerTrust) {
        RemoteNodeProvider::set_worker_trust(self, trust);
    }

    fn mark_fabric_revoked(&self) {
        self.mark_worker_revoked();
    }

    fn request_jobs_cancel(&self, cancel: tetonic_fabric_protocol::CancellationRequest) {
        self.spawn_jobs_cancel(cancel);
    }

    /// Probe `/v1/negotiate` + `/v1/health` + `/v1/capabilities` for fabric snapshot merge.
    ///
    /// Negotiation is required (R7-3): version skew or missing features clears the
    /// session and marks the node unhealthy so Infer is not scheduled.
    async fn probe_node(&self) -> Option<NodeInfo> {
        let negotiated_ok = self.run_negotiate().await;
        // These read-only observations are independent after negotiation.
        // In particular, an unavailable enrolled endpoint must not impose three
        // consecutive connection timeouts on an otherwise local inference call.
        let (health, caps, capacity_response) = tokio::join!(
            self.request("GET", "/v1/health", None, "fabric:health"),
            self.request("GET", "/v1/capabilities", None, "fabric:capabilities"),
            self.request("GET", "/v1/capacity/status", None, "fabric:capacity"),
        );

        let (mut healthy, queue_depth) = match &health {
            Ok(r) if r.status == 200 => {
                let parsed: HealthBody = serde_json::from_str(&r.body).unwrap_or(HealthBody {
                    ok: false,
                    healthy: false,
                    queue_depth: 0,
                });
                (parsed.ok && parsed.healthy, parsed.queue_depth)
            }
            _ => (false, 0),
        };
        if !negotiated_ok {
            healthy = false;
        }

        let (models, vram_total, vram_free, resident, caps_ok) = match &caps {
            Ok(r) if r.status == 200 => {
                let parsed: CapsBody = serde_json::from_str(&r.body).unwrap_or_default();
                (
                    parsed.models,
                    parsed.vram_total_mb,
                    parsed.vram_free_mb,
                    parsed.resident_models,
                    true,
                )
            }
            _ => (vec![], 0, 0, vec![], false),
        };

        let verified_model_names = if caps_ok {
            fetch_verified_model_names(self).await
        } else {
            None
        };
        let inventory_names_for_verify: Vec<String> = {
            let from_caps = caps
                .as_ref()
                .ok()
                .filter(|r| r.status == 200)
                .and_then(|r| serde_json::from_str::<CapsBody>(&r.body).ok())
                .and_then(|b| b.worker_capabilities)
                .map(|doc| {
                    doc.model_inventory
                        .iter()
                        .map(|m| m.local_name.clone())
                        .collect::<Vec<_>>()
                });
            from_caps.unwrap_or_else(|| models.clone())
        };
        let models_verified = inventory_matches_verified(
            &inventory_names_for_verify,
            verified_model_names.as_deref(),
        );

        let capacity = capacity_response.ok().and_then(|r| {
            if r.status != 200 {
                return None;
            }
            serde_json::from_str::<CapacityWireBody>(&r.body)
                .ok()
                .map(|w| w.into_node_health())
        });

        let mut legacy_v1_chat_only = self.fabric_capabilities().legacy_v1_chat_only;
        if let Ok(r) = &caps {
            if r.status == 200
                && tetonic_fabric_protocol::validate_capability_document_bytes(r.body.as_bytes())
                    .is_ok()
            {
                if let Ok(parsed) = serde_json::from_str::<CapsBody>(&r.body) {
                    if let Some(proto) = parsed.fabric_protocol {
                        legacy_v1_chat_only = proto.legacy_v1_chat_only;
                        if let Ok(mut cached) = self.capabilities.write() {
                            *cached = proto;
                        }
                    }
                    if let Some(doc) = parsed.worker_capabilities {
                        let worker_id = WorkerId::new(&self.worker_id);
                        let epoch = self.policy_epoch.load(Ordering::Relaxed);
                        if let Ok(mut reg) = self.capability_registry.write() {
                            let previous = reg.get(&self.worker_id).cloned();
                            let outcome = reg.upsert_probe_session(
                                tetonic_fabric_protocol::ProbeSessionInput {
                                    caps: doc,
                                    channel_worker_id: &worker_id,
                                    known_revocation_epoch: epoch,
                                    now: Utc::now(),
                                    previous: previous.as_ref(),
                                    health_ok: healthy,
                                    models_verified,
                                    verified_model_names: verified_model_names.as_deref(),
                                },
                            );
                            if let Ok(refresh) = &outcome {
                                tracing::debug!(
                                    worker = %self.worker_id,
                                    ?refresh,
                                    quarantined = reg.is_quarantined(&self.worker_id),
                                    degraded = reg.is_degraded(&self.worker_id),
                                    verified = !reg
                                        .verified_evidence(&self.worker_id)
                                        .unwrap_or(&[])
                                        .is_empty(),
                                    "capability probe session"
                                );
                            }
                        }
                    }
                }
            }
        }

        Some(NodeInfo {
            id: self.worker_id.clone(),
            label: self.label.clone(),
            vram_total_mb: vram_total,
            vram_free_mb: vram_free,
            resident_models: if resident.is_empty() {
                models
            } else {
                resident
            },
            queue_depth,
            healthy,
            models_verified,
            capacity,
            legacy_v1_chat_only,
            negotiated_protocol_version: self.negotiated_protocol_version(),
        })
    }

    async fn chat_on_fabric(
        &self,
        req: ChatRequest,
        job_id: &str,
        attempt_id: &str,
        fabric: Option<&FabricCallMeta>,
        turn_affinity: Option<&str>,
        registry: Option<&ActiveJobRegistry>,
        on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        validate_revoked(self.is_worker_revoked())
            .map_err(|e| InferenceError::Provider(format!("revoked worker: {e}")))?;
        if self.negotiated_protocol_version().is_none() {
            return Err(InferenceError::Provider(
                "UnsupportedProtocolVersion: fabric handshake not negotiated".into(),
            ));
        }
        if let Some(guard) = &self.dispatch_guard {
            match evaluate_remote_dispatch(
                guard.as_ref(),
                &req,
                &self.worker_id,
                false,
                Some(self.worker_trust()),
            ) {
                Ok(decision) if !decision.allows_remote() => {
                    return Err(InferenceError::Provider(
                        "dispatch guard blocked remote fabric job".into(),
                    ));
                }
                Err(deny) => {
                    return Err(InferenceError::Provider(deny.reason));
                }
                _ => {}
            }
        } else {
            return Err(InferenceError::Provider(
                "remote provider invoked without dispatch guard".into(),
            ));
        }

        let (session_id, agent_id, step_index) = fabric
            .map(|f| {
                (
                    f.session_id.clone(),
                    f.agent_id.clone().unwrap_or_else(|| "agent".into()),
                    f.step_index,
                )
            })
            .unwrap_or((None, "agent".into(), 0));
        let data_class = aggregate_chat_classification(&req)
            .map(|classification| classification.class)
            .unwrap_or(DataClass::RepositorySource);
        let disclosure_tier = fabric
            .map(|f| f.disclosure_tier)
            .unwrap_or(DisclosureTier::Auditable);
        let mut protocol_job = build_infer_job_envelope(
            &LegacyInferParams {
                job_id: job_id.to_string(),
                attempt_id: attempt_id.to_string(),
                data_class,
                run_id: fabric
                    .and_then(|f| f.run_id.clone())
                    .or_else(|| session_id.clone()),
                task_id: fabric.and_then(|f| f.task_id.clone()),
            },
            &req.messages,
            &req.model,
        )
        .map_err(|e| InferenceError::Provider(e.to_string()))?;
        if let Some(meta) = fabric {
            protocol_job.workspace_version = meta.workspace_version.clone();
            protocol_job.input_artifacts = meta
                .input_artifacts
                .iter()
                .map(|artifact| ArtifactReference {
                    artifact_id: artifact.artifact_id.clone(),
                    digest: tetonic_domain::workspace::ContentDigest::new(&artifact.digest),
                })
                .collect();
            let wants_net = meta
                .required_capabilities
                .iter()
                .any(|c| c == NETWORK_DENY_ALL_CAPABILITY)
                || !req.tools.is_empty();
            protocol_job.required_capabilities = JobRequirements {
                tools: meta
                    .required_capabilities
                    .iter()
                    .filter(|c| *c != NETWORK_DENY_ALL_CAPABILITY)
                    .cloned()
                    .collect(),
                environment: if wants_net {
                    // Tool-bearing Infer implies ModelRequestedShell DenyAll (R7-2).
                    vec![NETWORK_DENY_ALL_CAPABILITY.into()]
                } else {
                    vec![]
                },
            };
        } else if !req.tools.is_empty() {
            protocol_job.required_capabilities.environment =
                vec![NETWORK_DENY_ALL_CAPABILITY.into()];
        }
        if let tetonic_fabric_protocol::VersionedJobPayload::V1Infer(payload) =
            &mut protocol_job.payload
        {
            payload["tools"] = serde_json::to_value(&req.tools)
                .map_err(|error| InferenceError::Decode(error.to_string()))?;
            if let Some(model_digest) = &req.model_digest {
                payload["model_digest"] = serde_json::Value::String(model_digest.clone());
            }
        }
        let capability_registry = self.capability_registry.as_ref();
        let trace = fabric
            .map(|meta| FabricTraceContext {
                trace_id: meta.trace_context.trace_id.clone(),
                span_id: meta.trace_context.span_id.clone(),
                scheduler_decision_id: meta
                    .scheduler_decision_id
                    .clone()
                    .or_else(|| meta.trace_context.scheduler_decision_id.clone()),
            })
            .unwrap_or_default();
        let project_policy = self
            .dispatch_guard
            .as_ref()
            .map(|guard| guard.project_placement_policy())
            .unwrap_or_default();
        let typed_placement = {
            let registry = capability_registry
                .read()
                .map_err(|_| InferenceError::Provider("capability registry poisoned".into()))?;
            evaluate_typed_job_placement(
                &protocol_job,
                &self.worker_id,
                self.worker_trust(),
                self.policy_epoch.load(Ordering::Relaxed),
                project_policy.clone(),
                &trace,
                None,
                &registry,
                Utc::now(),
            )
        };
        if !typed_placement.allows_remote() {
            return Err(InferenceError::Provider(format!(
                "typed placement blocked remote fabric job: {typed_placement:?}"
            )));
        }
        let required_result_verification = match &typed_placement {
            tetonic_domain::placement::PlacementDecision::Eligible {
                required_verification:
                    tetonic_domain::placement::VerificationRequirement::Required { policy_id },
                ..
            } => requirement_from_placement(true, policy_id.as_deref()),
            _ => ResultVerificationRequirement::StructuralValidation,
        };
        let worker_id = tetonic_domain::ids::WorkerId::new(&self.worker_id);
        let offer = encode_job_offer_envelope(
            tetonic_fabric_protocol::CoordinatorId(self.estate_id.clone()),
            worker_id.clone(),
            protocol_job.clone(),
            self.policy_epoch.load(Ordering::Relaxed),
        );
        validate_outbound_job(&worker_id, &offer, &default_limits())
            .map_err(|e| InferenceError::Provider(e.to_string()))?;
        self.revalidate_dispatch_state(&protocol_job).await?;
        let caps = self.fabric_capabilities();
        let legacy_v1_chat_only = caps.legacy_v1_chat_only;

        if !legacy_v1_chat_only {
            {
                let registry = capability_registry
                    .read()
                    .map_err(|_| InferenceError::Provider("capability registry poisoned".into()))?;
                let probe_req = LeaseRenewalRequest {
                    context: LifecycleContext {
                        job_id: protocol_job.job_id.clone(),
                        task_id: protocol_job.task_id.clone(),
                        attempt_id: protocol_job.attempt_id.clone(),
                        lease_id: protocol_job.lease_id.clone(),
                        lease_epoch: protocol_job.lease_epoch,
                        worker_id: worker_id.clone(),
                        sequence_number: 0,
                        protocol_version: ProtocolVersion(
                            tetonic_fabric_protocol::PROTOCOL_VERSION,
                        ),
                        idempotency_key: IdempotencyKey(format!(
                            "lease-probe:{}:{}",
                            protocol_job.attempt_id.0, protocol_job.lease_epoch
                        )),
                    },
                    requested_expires_at: Utc::now() + chrono::Duration::minutes(5),
                };
                request_lease_renewal(
                    &caps,
                    &probe_req,
                    &protocol_job,
                    self.worker_trust(),
                    self.policy_epoch.load(Ordering::Relaxed),
                    project_policy.clone(),
                    &registry,
                    Utc::now(),
                )
                .map_err(|e| {
                    InferenceError::Provider(format!("typed lease renewal refused: {e}"))
                })?;
            }

            let lease_ttl = typed_fabric_lease_ttl();
            let renew_every = typed_fabric_renew_interval(lease_ttl);
            let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
            let lease_fail: Arc<std::sync::Mutex<Option<String>>> =
                Arc::new(std::sync::Mutex::new(None));
            let lease_fail_task = lease_fail.clone();
            let lease_guard = self.guard.clone();
            let lease_ip = self.ip;
            let lease_port = self.port;
            let lease_cert = self.server_cert.clone();
            let lease_coord = self.coordinator.clone();
            let lease_estate = self.estate_id.clone();
            let lease_epoch_policy = self.policy_epoch.load(Ordering::Relaxed);
            let lease_job = protocol_job.clone();
            let lease_worker = worker_id.clone();
            let mut stop_rx_loop = stop_rx.clone();
            let lease_task = tokio::spawn(async move {
                // Allow worker to register attempt before first renew.
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                let mut seq = 1u64;
                loop {
                    if *stop_rx_loop.borrow() {
                        break;
                    }
                    let lease_req = LeaseRenewalRequest {
                        context: LifecycleContext {
                            job_id: lease_job.job_id.clone(),
                            task_id: lease_job.task_id.clone(),
                            attempt_id: lease_job.attempt_id.clone(),
                            lease_id: lease_job.lease_id.clone(),
                            lease_epoch: lease_job.lease_epoch,
                            worker_id: lease_worker.clone(),
                            sequence_number: seq,
                            protocol_version: ProtocolVersion(
                                tetonic_fabric_protocol::PROTOCOL_VERSION,
                            ),
                            idempotency_key: IdempotencyKey(format!(
                                "lease:{}:{}:{}",
                                lease_job.attempt_id.0, lease_job.lease_epoch, seq
                            )),
                        },
                        requested_expires_at: Utc::now()
                            + chrono::Duration::from_std(lease_ttl)
                                .unwrap_or_else(|_| chrono::Duration::seconds(30)),
                    };
                    seq = seq.saturating_add(1);
                    let lease_env = encode_lease_renewal_envelope(
                        tetonic_fabric_protocol::CoordinatorId(lease_estate.clone()),
                        lease_worker.clone(),
                        lease_req,
                        lease_epoch_policy,
                    );
                    let Ok(lease_body) = serde_json::to_string(&lease_env) else {
                        break;
                    };
                    match fabric_request(
                        &lease_guard,
                        lease_ip,
                        lease_port,
                        &lease_cert,
                        &lease_coord,
                        "POST",
                        "/v1/jobs/lease",
                        Some(&lease_body),
                        "fabric:jobs_lease",
                    )
                    .await
                    {
                        Ok(resp) if resp.status == 200 => {}
                        Ok(resp) if resp.status == 404 => {
                            // Job finished between renewals.
                            break;
                        }
                        Ok(resp) if resp.status == 409 || resp.status == 403 => {
                            if let Ok(mut g) = lease_fail_task.lock() {
                                *g = Some(format!(
                                    "typed lease renewal denied (HTTP {}): {}",
                                    resp.status, resp.body
                                ));
                            }
                            break;
                        }
                        Ok(resp) => {
                            if let Ok(mut g) = lease_fail_task.lock() {
                                *g = Some(format!(
                                    "typed lease renewal failed (HTTP {}): {}",
                                    resp.status, resp.body
                                ));
                            }
                            break;
                        }
                        Err(e) => {
                            if let Ok(mut g) = lease_fail_task.lock() {
                                *g = Some(format!("typed lease renewal transport error: {e}"));
                            }
                            break;
                        }
                    }
                    tokio::select! {
                        changed = stop_rx_loop.changed() => {
                            if changed.is_err() || *stop_rx_loop.borrow() {
                                break;
                            }
                        }
                        _ = tokio::time::sleep(renew_every) => {}
                    }
                }
            });

            let serialize_start = std::time::Instant::now();
            let body =
                serde_json::to_string(&offer).map_err(|e| InferenceError::Decode(e.to_string()))?;
            let endpoint = "/v1/jobs";
            let req_event = "fabric:jobs";
            let serialize_in_ms = serialize_start.elapsed().as_millis() as u64;
            tetonic_telemetry::record_compute_stage(
                tetonic_telemetry::span_names::TRANSFER_INPUT,
                Some("infer"),
                None,
                None,
                Some("remote"),
                fabric.and_then(|f| f.scheduler_decision_id.as_deref()),
                None,
                Some(serialize_in_ms),
                false,
            );

            let expected_job_id = job_id.to_string();
            let expected_attempt_id = attempt_id.to_string();
            let mut legacy: Option<ChatResponseBody> = None;
            let mut stream_result: Option<FabricJobResult> = None;
            let mut stream_result_pubkey: Option<String> = None;
            let mut stream_ok = false;
            let mut stream_error: Option<String> = None;
            let mut stream_tokens: Vec<String> = Vec::new();

            let transfer_start = std::time::Instant::now();
            let status = fabric_request_ndjson(
                &self.guard,
                self.ip,
                self.port,
                &self.server_cert,
                &self.coordinator,
                "POST",
                endpoint,
                Some(&body),
                req_event,
                |line| {
                    let v: serde_json::Value = serde_json::from_str(line)
                        .map_err(|e| FabricClientError::Http(format!("ndjson parse: {e}")))?;
                    if let Some(jid) = v.get("job_id").and_then(|j| j.as_str()) {
                        if jid != expected_job_id {
                            return Err(FabricClientError::Http(format!(
                                "ndjson job_id mismatch: expected {expected_job_id}, got {jid}"
                            )));
                        }
                    }
                    if let Some(aid) = v.get("attempt_id").and_then(|j| j.as_str()) {
                        if aid != expected_attempt_id {
                            return Err(FabricClientError::Http(format!(
                                "ndjson attempt_id mismatch: expected {expected_attempt_id}, got {aid}"
                            )));
                        }
                    }
                    if v.get("event").is_none() {
                        let parsed: ChatResponseBody = serde_json::from_value(v).map_err(|e| {
                            FabricClientError::Http(format!("legacy chat json: {e}"))
                        })?;
                        if let Some(ref result) = parsed.result {
                            validate_result_identity(
                                &expected_job_id,
                                Some(&expected_attempt_id),
                                result,
                            )?;
                        }
                        legacy = Some(parsed);
                        return Ok(());
                    }
                    match v.get("event").and_then(|e| e.as_str()) {
                        Some("token") => {
                            if let Some(delta) = v.get("delta").and_then(|d| d.as_str()) {
                                stream_tokens.push(delta.to_string());
                            }
                        }
                        Some("done") => {
                            stream_ok = v.get("ok").and_then(|b| b.as_bool()).unwrap_or(false);
                            stream_error =
                                v.get("error").and_then(|e| e.as_str()).map(String::from);
                            stream_result_pubkey = v
                                .get("result_signing_public_key")
                                .and_then(|e| e.as_str())
                                .map(String::from);
                            if let Some(r) = v.get("result") {
                                let parsed: FabricJobResult =
                                    serde_json::from_value(r.clone()).map_err(|e| {
                                        FabricClientError::Http(format!("result json: {e}"))
                                    })?;
                                validate_result_identity(
                                    &expected_job_id,
                                    Some(&expected_attempt_id),
                                    &parsed,
                                )?;
                                stream_result = Some(parsed);
                            }
                        }
                        _ => {}
                    }
                    Ok(())
                },
            )
            .await
            .map_err(map_fabric_err)?;

            // AC4: non-legacy worker that does not speak typed jobs — refuse, never fall back to /v1/chat.
            if typed_jobs_status_refuses_infer(status) {
                let _ = stop_tx.send(true);
                let _ = lease_task.await;
                return Err(InferenceError::Provider(format!(
                    "typed Infer refused: worker {} returned HTTP {status} on /v1/jobs (no legacy chat fallback)",
                    self.label
                )));
            }

            let _ = stop_tx.send(true);
            let _ = lease_task.await;
            if let Ok(guard) = lease_fail.lock() {
                if let Some(msg) = guard.as_ref() {
                    return Err(InferenceError::Provider(msg.clone()));
                }
            }

            let transfer_ms = transfer_start.elapsed().as_millis() as u64;
            tetonic_telemetry::record_compute_stage(
                tetonic_telemetry::span_names::TRANSFER_OUTPUT,
                Some("infer"),
                None,
                None,
                Some("remote"),
                fabric.and_then(|f| f.scheduler_decision_id.as_deref()),
                None,
                Some(transfer_ms),
                false,
            );

            return finish_chat_on_fabric(
                self,
                FinishChatArgs {
                    legacy,
                    status,
                    stream_ok,
                    stream_error,
                    stream_result,
                    stream_result_pubkey,
                    stream_tokens,
                    expected_job_id,
                    expected_attempt_id,
                    protocol_job,
                    required_result_verification,
                    registry,
                    session_id,
                    fabric_trace_id: fabric.map(|f| f.trace_context.trace_id.clone()),
                    model: req.model.clone(),
                    transfer_ms,
                },
                on_token,
            )
            .await;
        }

        let serialize_start = std::time::Instant::now();
        let job = infer_job_to_legacy_chat(
            &protocol_job,
            &self.estate_id,
            req.messages,
            req.tools,
            &req.model,
            self.policy_epoch.load(Ordering::Relaxed),
            session_id.as_deref(),
            &agent_id,
            step_index,
            SampleOptions {
                temperature: req.temperature,
                num_ctx: req.num_ctx,
                response_format: req.response_format.clone(),
                stream: Some(true),
                ..Default::default()
            },
            disclosure_tier,
            turn_affinity.map(String::from),
        )
        .map_err(|e| InferenceError::Provider(e.to_string()))?;
        let body =
            serde_json::to_string(&job).map_err(|e| InferenceError::Decode(e.to_string()))?;
        let endpoint = "/v1/chat";
        let req_event = "fabric:chat";
        let serialize_in_ms = serialize_start.elapsed().as_millis() as u64;
        tetonic_telemetry::record_compute_stage(
            tetonic_telemetry::span_names::TRANSFER_INPUT,
            Some("infer"),
            None,
            None,
            Some("remote"),
            fabric.and_then(|f| f.scheduler_decision_id.as_deref()),
            None,
            Some(serialize_in_ms),
            false,
        );

        let expected_job_id = job_id.to_string();
        let expected_attempt_id = attempt_id.to_string();
        let mut legacy: Option<ChatResponseBody> = None;
        let mut stream_result: Option<FabricJobResult> = None;
        let mut stream_result_pubkey: Option<String> = None;
        let mut stream_ok = false;
        let mut stream_error: Option<String> = None;
        let mut stream_tokens: Vec<String> = Vec::new();

        let transfer_start = std::time::Instant::now();
        let status = fabric_request_ndjson(
            &self.guard,
            self.ip,
            self.port,
            &self.server_cert,
            &self.coordinator,
            "POST",
            endpoint,
            Some(&body),
            req_event,
            |line| {
                let v: serde_json::Value = serde_json::from_str(line)
                    .map_err(|e| FabricClientError::Http(format!("ndjson parse: {e}")))?;
                if let Some(jid) = v.get("job_id").and_then(|j| j.as_str()) {
                    if jid != expected_job_id {
                        return Err(FabricClientError::Http(format!(
                            "ndjson job_id mismatch: expected {expected_job_id}, got {jid}"
                        )));
                    }
                }
                if let Some(aid) = v.get("attempt_id").and_then(|j| j.as_str()) {
                    if aid != expected_attempt_id {
                        return Err(FabricClientError::Http(format!(
                            "ndjson attempt_id mismatch: expected {expected_attempt_id}, got {aid}"
                        )));
                    }
                }
                if v.get("event").is_none() {
                    let parsed: ChatResponseBody = serde_json::from_value(v)
                        .map_err(|e| FabricClientError::Http(format!("legacy chat json: {e}")))?;
                    if let Some(ref result) = parsed.result {
                        validate_result_identity(
                            &expected_job_id,
                            Some(&expected_attempt_id),
                            result,
                        )?;
                    }
                    legacy = Some(parsed);
                    return Ok(());
                }
                match v.get("event").and_then(|e| e.as_str()) {
                    Some("token") => {
                        if let Some(delta) = v.get("delta").and_then(|d| d.as_str()) {
                            // Buffer until signed accept succeeds (R2-2).
                            stream_tokens.push(delta.to_string());
                        }
                    }
                    Some("done") => {
                        stream_ok = v.get("ok").and_then(|b| b.as_bool()).unwrap_or(false);
                        stream_error = v.get("error").and_then(|e| e.as_str()).map(String::from);
                        stream_result_pubkey = v
                            .get("result_signing_public_key")
                            .and_then(|e| e.as_str())
                            .map(String::from);
                        if let Some(r) = v.get("result") {
                            let parsed: FabricJobResult = serde_json::from_value(r.clone())
                                .map_err(|e| {
                                    FabricClientError::Http(format!("result json: {e}"))
                                })?;
                            validate_result_identity(
                                &expected_job_id,
                                Some(&expected_attempt_id),
                                &parsed,
                            )?;
                            stream_result = Some(parsed);
                        }
                    }
                    _ => {}
                }
                Ok(())
            },
        )
        .await
        .map_err(map_fabric_err)?;
        let transfer_ms = transfer_start.elapsed().as_millis() as u64;
        tetonic_telemetry::record_compute_stage(
            tetonic_telemetry::span_names::TRANSFER_OUTPUT,
            Some("infer"),
            None,
            None,
            Some("remote"),
            fabric.and_then(|f| f.scheduler_decision_id.as_deref()),
            None,
            Some(transfer_ms),
            false,
        );

        finish_chat_on_fabric(
            self,
            FinishChatArgs {
                legacy,
                status,
                stream_ok,
                stream_error,
                stream_result,
                stream_result_pubkey,
                stream_tokens,
                expected_job_id,
                expected_attempt_id,
                protocol_job,
                required_result_verification,
                registry,
                session_id,
                fabric_trace_id: fabric.map(|f| f.trace_context.trace_id.clone()),
                model: req.model.clone(),
                transfer_ms,
            },
            on_token,
        )
        .await
    }
}

struct FinishChatArgs<'a> {
    legacy: Option<ChatResponseBody>,
    status: u16,
    stream_ok: bool,
    stream_error: Option<String>,
    stream_result: Option<FabricJobResult>,
    stream_result_pubkey: Option<String>,
    stream_tokens: Vec<String>,
    expected_job_id: String,
    expected_attempt_id: String,
    protocol_job: tetonic_fabric_protocol::JobEnvelope,
    required_result_verification: ResultVerificationRequirement,
    registry: Option<&'a ActiveJobRegistry>,
    session_id: Option<String>,
    fabric_trace_id: Option<String>,
    model: String,
    transfer_ms: u64,
}

async fn finish_chat_on_fabric(
    provider: &RemoteNodeProvider,
    args: FinishChatArgs<'_>,
    on_token: &mut TokenSink<'_>,
) -> Result<ChatResponse, InferenceError> {
    let FinishChatArgs {
        legacy,
        status,
        stream_ok,
        stream_error,
        stream_result,
        stream_result_pubkey,
        stream_tokens,
        expected_job_id,
        expected_attempt_id,
        protocol_job,
        required_result_verification,
        registry,
        session_id,
        fabric_trace_id,
        model,
        transfer_ms,
    } = args;

    if let Some(parsed) = legacy {
        let mut accept_timing = AcceptedChatTiming::default();
        if parsed.ok {
            if let Some(ref result) = parsed.result {
                crate::legacy_result::validate_chat_payload(result)?;
                let envelope_json = result.result_envelope.clone().ok_or_else(|| {
                    InferenceError::Provider("unsigned remote result rejected (M5-4)".into())
                })?;
                accept_timing = provider
                    .accept_signed_chat_result(
                        protocol_job.clone(),
                        envelope_json,
                        parsed.result_signing_public_key.clone(),
                        required_result_verification.clone(),
                        registry,
                        session_id.clone(),
                        fabric_trace_id.clone(),
                    )
                    .await?;
            }
        }
        let mut resp = handle_chat_response_body(
            parsed,
            &expected_job_id,
            Some(&expected_attempt_id),
            &provider.worker_id,
            registry,
            &model,
            on_token,
        )?;
        stamp_chat_timing(&mut resp, accept_timing, transfer_ms);
        return Ok(resp);
    }

    if status == 503 {
        return Err(InferenceError::WorkerBusy {
            node_id: provider.worker_id.clone(),
        });
    }
    if status != 200 {
        return Err(InferenceError::Provider(format!(
            "worker {} returned HTTP {}: {}",
            provider.label,
            status,
            stream_error.unwrap_or_else(|| "fabric chat failed".into())
        )));
    }
    if !stream_ok {
        if stream_result
            .as_ref()
            .is_some_and(|r| r.status == JobStatus::Preempted)
        {
            return Err(InferenceError::Preempted {
                node_id: provider.worker_id.clone(),
            });
        }
        return Err(InferenceError::Provider(
            stream_error.unwrap_or_else(|| "worker rejected job".into()),
        ));
    }
    let result = stream_result
        .ok_or_else(|| InferenceError::Decode("missing result in fabric chat stream".into()))?;
    validate_result_identity(&expected_job_id, Some(&expected_attempt_id), &result)
        .map_err(|e| InferenceError::Decode(e.to_string()))?;
    validate_revoked(provider.is_worker_revoked())
        .map_err(|e| InferenceError::Provider(format!("revoked worker result rejected: {e}")))?;
    if result.status == JobStatus::Preempted {
        return Err(InferenceError::Preempted {
            node_id: provider.worker_id.clone(),
        });
    }
    if result.status != JobStatus::Ok {
        return Err(InferenceError::Provider(
            result
                .error
                .unwrap_or_else(|| format!("job status {:?}", result.status)),
        ));
    }
    crate::legacy_result::validate_chat_payload(&result)?;
    // Tokens are buffered until accept_signed_chat_result succeeds (R2-2).
    let envelope_json = result
        .result_envelope
        .clone()
        .ok_or_else(|| InferenceError::Provider("unsigned remote result rejected (M5-4)".into()))?;
    let accept = provider
        .accept_signed_chat_result(
            protocol_job.clone(),
            envelope_json,
            stream_result_pubkey,
            required_result_verification,
            registry,
            session_id.clone(),
            fabric_trace_id,
        )
        .await;
    let accept_timing =
        deliver_stream_after_accept(accept, &stream_tokens, &result.message.content, on_token)?;
    let mut resp = ChatResponse {
        message: result.message,
        usage: GenUsage::from(result.usage),
        provenance: InferenceProvenance {
            provider_kind: "remote".into(),
            worker_id: Some(provider.worker_id.clone()),
            model,
            job_id: Some(expected_job_id),
            attempt_id: Some(expected_attempt_id),
            prompt_redacted: false,
            ..Default::default()
        },
    };
    stamp_chat_timing(&mut resp, accept_timing, transfer_ms);
    Ok(resp)
}

fn stamp_chat_timing(resp: &mut ChatResponse, timing: AcceptedChatTiming, transfer_ms: u64) {
    resp.provenance.worker_queue_ms = timing.queue_ms;
    resp.provenance.worker_execute_ms = timing.execute_ms;
    resp.provenance.verification_ms = timing.verification_ms;
    resp.provenance.transfer_ms = Some(transfer_ms);
}

/// Infer wire path from cached worker advertisement (R7-1).
pub fn fabric_infer_endpoint(legacy_v1_chat_only: bool) -> &'static str {
    if legacy_v1_chat_only {
        "/v1/chat"
    } else {
        "/v1/jobs"
    }
}

/// AC4: non-legacy worker returned a non-typed jobs status — refuse Infer (no chat fallback).
pub fn typed_jobs_status_refuses_infer(status: u16) -> bool {
    matches!(status, 404 | 501 | 405)
}

/// Soft lease TTL for typed Infer (must match worker `lease_table::default_lease_ttl`).
pub fn typed_fabric_lease_ttl() -> std::time::Duration {
    let ms = std::env::var("LOKAI_FABRIC_LEASE_TTL_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(30_000);
    std::time::Duration::from_millis(ms.max(200))
}

pub fn typed_fabric_renew_interval(ttl: std::time::Duration) -> std::time::Duration {
    let third = ttl / 3;
    if third.is_zero() {
        std::time::Duration::from_millis(50)
    } else {
        third
    }
}

fn map_fabric_err(e: FabricClientError) -> InferenceError {
    match e {
        FabricClientError::Protocol(p) => InferenceError::Provider(p.to_string()),
        FabricClientError::Egress(inner) => InferenceError::Egress(inner),
        FabricClientError::NoServerCert => {
            InferenceError::Provider("missing worker fabric TLS certificate — re-enroll".into())
        }
        FabricClientError::Timeout { phase, path } => {
            InferenceError::Provider(format!("fabric {phase} timeout on {path}"))
        }
        other => InferenceError::Provider(other.to_string()),
    }
}

/// True when every advertised inventory name appears in the coordinator-fetched verified list.
pub(crate) fn inventory_matches_verified(
    inventory_names: &[String],
    verified_names: Option<&[String]>,
) -> bool {
    let Some(verified) = verified_names else {
        return false;
    };
    inventory_names
        .iter()
        .all(|name| verified.iter().any(|v| v == name))
}

async fn fetch_verified_model_names(provider: &RemoteNodeProvider) -> Option<Vec<String>> {
    let resp = provider
        .request("GET", "/v1/models/verified", None, "fabric:models_verified")
        .await
        .ok()?;
    if resp.status != 200 {
        return None;
    }
    let parsed: VerifiedModelsBody = serde_json::from_str(&resp.body).ok()?;
    if !parsed.ok {
        return None;
    }
    Some(parsed.models.into_iter().map(|m| m.name).collect())
}

#[derive(Debug, Deserialize, Default)]
struct HealthBody {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    healthy: bool,
    #[serde(default)]
    queue_depth: u32,
}

#[derive(Debug, Deserialize, Default)]
struct CapsBody {
    #[allow(dead_code)]
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    models: Vec<String>,
    #[serde(default)]
    vram_total_mb: u32,
    #[serde(default)]
    vram_free_mb: u32,
    #[serde(default)]
    resident_models: Vec<String>,
    #[serde(default)]
    fabric_protocol: Option<WorkerCapabilityAdvertisement>,
    #[serde(default)]
    worker_capabilities: Option<WorkerCapabilities>,
}

#[derive(Debug, Deserialize, Default)]
struct VerifiedModelsBody {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    models: Vec<VerifiedModelEntry>,
}

#[derive(Debug, Deserialize, Default)]
struct VerifiedModelEntry {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize, Default)]
struct CapacityWireBody {
    #[serde(default)]
    doctor: String,
    #[serde(default)]
    gates_ok: bool,
    #[serde(default)]
    stale: bool,
    #[serde(default)]
    active_profile_id: Option<String>,
}

impl CapacityWireBody {
    fn into_node_health(self) -> NodeCapacityHealth {
        NodeCapacityHealth {
            doctor: if self.doctor.is_empty() {
                "unknown".into()
            } else {
                self.doctor
            },
            active_profile_id: self.active_profile_id,
            gates_ok: self.gates_ok,
            stale: self.stale,
        }
    }
}

#[cfg(test)]
#[path = "legacy_tests.rs"]
mod tests;

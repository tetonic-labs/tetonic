//! Decoupled GPU Inference Fabric Routing (SAE-403).
//!
//! Enforces physical decoupling between stateful agent actor execution and
//! stateless GPU inference servers. Routes token requests across a pool of
//! GPU endpoints using least-concurrency load balancing, reflexive vs. deliberative
//! tiering, and circuit breaking with automatic fallback.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

use crate::{ChatRequest, ChatResponse, InferenceError, InferenceProvider, TokenSink};

/// Latency and capacity tier for inference endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EndpointTier {
    /// High-throughput, low-latency endpoints for reflexive system 1 reactions.
    Reflexive,
    /// High-capacity, deep-reasoning endpoints for deliberative multi-step planning.
    Deliberative,
    /// Universal endpoint capable of serving both reflexive and deliberative tiers.
    Universal,
}

impl EndpointTier {
    pub fn matches(&self, required: EndpointTier) -> bool {
        match (self, required) {
            (Self::Universal, _) => true,
            (_, Self::Universal) => true,
            (Self::Reflexive, Self::Reflexive) => true,
            (Self::Deliberative, Self::Deliberative) => true,
            _ => false,
        }
    }
}

/// Operational state of an endpoint's circuit breaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CircuitState {
    /// Normal operating state; traffic is routed to the endpoint.
    Closed,
    /// Tripped due to consecutive failures; traffic bypasses to backup or fallback.
    Open,
    /// Probing recovery after cooldown period.
    HalfOpen,
}

/// Stateless remote GPU inference endpoint (vLLM, TensorRT-LLM, Ollama cluster).
pub struct InferenceEndpoint {
    pub id: String,
    pub url: String,
    pub tier: EndpointTier,
    pub in_flight: Arc<AtomicUsize>,
    pub consecutive_failures: Arc<AtomicU32>,
    pub failure_threshold: u32,
    pub cooldown_duration: Duration,
    state: Arc<RwLock<CircuitState>>,
    last_failure_at: Arc<Mutex<Option<Instant>>>,
    /// Optional underlying provider representing this physical endpoint.
    provider: Option<Arc<dyn InferenceProvider>>,
}

impl InferenceEndpoint {
    pub fn new(
        id: impl Into<String>,
        url: impl Into<String>,
        tier: EndpointTier,
        provider: Option<Arc<dyn InferenceProvider>>,
    ) -> Self {
        Self {
            id: id.into(),
            url: url.into(),
            tier,
            in_flight: Arc::new(AtomicUsize::new(0)),
            consecutive_failures: Arc::new(AtomicU32::new(0)),
            failure_threshold: 3,
            cooldown_duration: Duration::from_secs(5),
            state: Arc::new(RwLock::new(CircuitState::Closed)),
            last_failure_at: Arc::new(Mutex::new(None)),
            provider,
        }
    }

    pub fn with_thresholds(mut self, threshold: u32, cooldown: Duration) -> Self {
        self.failure_threshold = threshold;
        self.cooldown_duration = cooldown;
        self
    }

    /// Checks if the endpoint is available for routing, handling HalfOpen transitions.
    pub async fn is_available(&self) -> bool {
        let current_state = *self.state.read().await;
        match current_state {
            CircuitState::Closed => true,
            CircuitState::HalfOpen => true,
            CircuitState::Open => {
                let last = *self.last_failure_at.lock().await;
                if let Some(t) = last {
                    if t.elapsed() >= self.cooldown_duration {
                        let mut state_guard = self.state.write().await;
                        *state_guard = CircuitState::HalfOpen;
                        return true;
                    }
                }
                false
            }
        }
    }

    /// Records a successful completion, resetting failure counters and closing the circuit.
    pub async fn record_success(&self) {
        self.consecutive_failures.store(0, Ordering::Relaxed);
        let mut state_guard = self.state.write().await;
        *state_guard = CircuitState::Closed;
    }

    /// Records a failure or timeout, tripping the circuit breaker if the threshold is reached.
    pub async fn record_failure(&self) {
        let fails = self.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
        let mut last = self.last_failure_at.lock().await;
        *last = Some(Instant::now());

        if fails >= self.failure_threshold {
            let mut state_guard = self.state.write().await;
            *state_guard = CircuitState::Open;
        }
    }

    /// Acquires an in-flight execution lease, decrementing automatically on drop.
    pub fn acquire_lease(self: &Arc<Self>) -> EndpointLease {
        self.in_flight.fetch_add(1, Ordering::SeqCst);
        EndpointLease {
            endpoint: Arc::clone(self),
        }
    }

    /// Current circuit state.
    pub async fn circuit_state(&self) -> CircuitState {
        *self.state.read().await
    }

    /// Force circuit state (useful in testing).
    pub async fn set_circuit_state(&self, new_state: CircuitState) {
        let mut s = self.state.write().await;
        *s = new_state;
    }
}

/// RAII lease tracking active in-flight request count on an endpoint.
pub struct EndpointLease {
    endpoint: Arc<InferenceEndpoint>,
}

impl Drop for EndpointLease {
    fn drop(&mut self) {
        self.endpoint.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Decoupled GPU inference router managing pool of stateless endpoints.
pub struct DecoupledInferenceRouter {
    endpoints: Arc<RwLock<Vec<Arc<InferenceEndpoint>>>>,
    fallback_provider: Option<Arc<dyn InferenceProvider>>,
}

impl DecoupledInferenceRouter {
    pub fn new(fallback_provider: Option<Arc<dyn InferenceProvider>>) -> Self {
        Self {
            endpoints: Arc::new(RwLock::new(Vec::new())),
            fallback_provider,
        }
    }

    /// Registers a new stateless GPU endpoint with the router.
    pub async fn add_endpoint(&self, endpoint: Arc<InferenceEndpoint>) {
        let mut list = self.endpoints.write().await;
        list.push(endpoint);
    }

    /// Removes an endpoint by ID.
    pub async fn remove_endpoint(&self, id: &str) {
        let mut list = self.endpoints.write().await;
        list.retain(|ep| ep.id != id);
    }

    /// Selects the healthiest endpoint matching the requested tier using least-concurrency balancing.
    pub async fn select_endpoint(&self, tier: EndpointTier) -> Option<Arc<InferenceEndpoint>> {
        let list = self.endpoints.read().await;

        let mut best: Option<Arc<InferenceEndpoint>> = None;
        let mut min_queue = usize::MAX;

        for ep in list.iter() {
            if !ep.tier.matches(tier) {
                continue;
            }
            if !ep.is_available().await {
                continue;
            }

            let q = ep.in_flight.load(Ordering::Relaxed);
            if q < min_queue {
                min_queue = q;
                best = Some(Arc::clone(ep));
            }
        }

        best
    }

    /// Resolves whether a request is reflexive (fast system 1) or deliberative (system 2).
    pub fn determine_tier(&self, req: &ChatRequest) -> EndpointTier {
        let m = req.model.to_lowercase();
        if m.contains("reflex") || m.contains("fast") || m.contains("small") {
            EndpointTier::Reflexive
        } else {
            EndpointTier::Deliberative
        }
    }
}

#[async_trait]
impl InferenceProvider for DecoupledInferenceRouter {
    async fn chat(
        &self,
        req: ChatRequest,
        on_token: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        let tier = self.determine_tier(&req);

        // Attempt primary routing through decoupled GPU pool
        if let Some(endpoint) = self.select_endpoint(tier).await {
            let _lease = endpoint.acquire_lease();

            if let Some(ref provider) = endpoint.provider {
                match provider.chat(req.clone(), on_token).await {
                    Ok(resp) => {
                        endpoint.record_success().await;
                        return Ok(resp);
                    }
                    Err(_e) => {
                        endpoint.record_failure().await;
                        // Log failure and attempt failover to fallback
                    }
                }
            } else {
                // Endpoint has no local adapter, mark success and fall back
                endpoint.record_success().await;
            }
        }

        // Secondary fallback to healthy alternative or local fallback provider
        if let Some(ref fallback) = self.fallback_provider {
            return fallback.chat(req, on_token).await;
        }

        Err(InferenceError::Provider(
            "No healthy inference endpoints available in decoupled fabric and no fallback configured".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Message;

    struct MockProvider {
        should_fail: Arc<tokio::sync::Mutex<bool>>,
        reply_text: String,
    }

    #[async_trait]
    impl InferenceProvider for MockProvider {
        async fn chat(
            &self,
            _req: ChatRequest,
            _on_token: &mut TokenSink<'_>,
        ) -> Result<ChatResponse, InferenceError> {
            if *self.should_fail.lock().await {
                Err(InferenceError::Provider("GPU node timeout / OOM".into()))
            } else {
                Ok(ChatResponse {
                    message: Message::assistant(self.reply_text.clone()),
                    usage: Default::default(),
                    provenance: Default::default(),
                })
            }
        }
    }

    fn sample_chat_request(model: &str) -> ChatRequest {
        ChatRequest {
            model: model.into(),
            model_digest: None,
            messages: vec![Message::user("hello")],
            tools: Vec::new(),
            temperature: 0.2,
            num_ctx: None,
            draft_model: None,
            draft_count: None,
            keep_alive: None,
            fabric: None,
            response_format: None,
            outbound_scan: crate::OutboundScan::default(),
        }
    }

    #[tokio::test]
    async fn test_router_least_concurrency_load_balancing() {
        let ep1 = Arc::new(InferenceEndpoint::new(
            "gpu-1",
            "http://gpu-1:8000",
            EndpointTier::Universal,
            None,
        ));
        let ep2 = Arc::new(InferenceEndpoint::new(
            "gpu-2",
            "http://gpu-2:8000",
            EndpointTier::Universal,
            None,
        ));

        let router = DecoupledInferenceRouter::new(None);
        router.add_endpoint(Arc::clone(&ep1)).await;
        router.add_endpoint(Arc::clone(&ep2)).await;

        // Simulate ep1 having an active job
        let _lease1 = ep1.acquire_lease();
        assert_eq!(ep1.in_flight.load(Ordering::Relaxed), 1);
        assert_eq!(ep2.in_flight.load(Ordering::Relaxed), 0);

        // Next request must choose ep2 (least concurrency)
        let chosen = router
            .select_endpoint(EndpointTier::Deliberative)
            .await
            .expect("should select endpoint");
        assert_eq!(chosen.id, "gpu-2");
    }

    #[tokio::test]
    async fn test_router_tier_routing() {
        let ep_reflex = Arc::new(InferenceEndpoint::new(
            "gpu-fast",
            "http://gpu-fast:8000",
            EndpointTier::Reflexive,
            None,
        ));
        let ep_delib = Arc::new(InferenceEndpoint::new(
            "gpu-deep",
            "http://gpu-deep:8000",
            EndpointTier::Deliberative,
            None,
        ));

        let router = DecoupledInferenceRouter::new(None);
        router.add_endpoint(Arc::clone(&ep_reflex)).await;
        router.add_endpoint(Arc::clone(&ep_delib)).await;

        let selected_reflex = router
            .select_endpoint(EndpointTier::Reflexive)
            .await
            .expect("reflexive endpoint");
        assert_eq!(selected_reflex.id, "gpu-fast");

        let selected_delib = router
            .select_endpoint(EndpointTier::Deliberative)
            .await
            .expect("deliberative endpoint");
        assert_eq!(selected_delib.id, "gpu-deep");
    }

    #[tokio::test]
    async fn test_circuit_breaker_trips_and_falls_back() {
        let fail_flag = Arc::new(tokio::sync::Mutex::new(true));
        let mock_gpu = Arc::new(MockProvider {
            should_fail: Arc::clone(&fail_flag),
            reply_text: "gpu reply".into(),
        });

        let fallback_gpu = Arc::new(MockProvider {
            should_fail: Arc::new(tokio::sync::Mutex::new(false)),
            reply_text: "local fallback reply".into(),
        });

        let ep = Arc::new(
            InferenceEndpoint::new(
                "flaky-gpu",
                "http://flaky:8000",
                EndpointTier::Universal,
                Some(mock_gpu),
            )
            .with_thresholds(2, Duration::from_millis(50)),
        );

        let router = DecoupledInferenceRouter::new(Some(fallback_gpu));
        router.add_endpoint(Arc::clone(&ep)).await;

        let mut sink = |_token: &str| {};
        let req = sample_chat_request("deliberative-model");

        // Request 1 fails on primary endpoint -> falls back to local fallback
        let resp1 = router.chat(req.clone(), &mut sink).await.expect("fallback success");
        assert_eq!(resp1.message.content, "local fallback reply");
        assert_eq!(ep.circuit_state().await, CircuitState::Closed);

        // Request 2 fails -> trips circuit to Open!
        let resp2 = router.chat(req.clone(), &mut sink).await.expect("fallback success");
        assert_eq!(resp2.message.content, "local fallback reply");
        assert_eq!(ep.circuit_state().await, CircuitState::Open);

        // While Open, endpoint is bypassed entirely
        assert!(!ep.is_available().await);

        // Wait for cooldown to test self-healing
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert!(ep.is_available().await); // transitions to HalfOpen
        assert_eq!(ep.circuit_state().await, CircuitState::HalfOpen);

        // Recover endpoint
        *fail_flag.lock().await = false;
        let resp3 = router.chat(req, &mut sink).await.expect("recovered success");
        assert_eq!(resp3.message.content, "gpu reply");
        assert_eq!(ep.circuit_state().await, CircuitState::Closed);
    }
}

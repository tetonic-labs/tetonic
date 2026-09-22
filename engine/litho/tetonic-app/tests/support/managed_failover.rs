//! A real managed loop crosses the production broker and fails over between workers.
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tetonic_domain::{AttemptId, AttemptState, CandidateOutcome, RunId, WorkerId};
use tetonic_inference::*;
use tetonic_run::RunSupervisor;

struct Worker {
    id: String,
    calls: Arc<AtomicUsize>,
    hops: Arc<Mutex<Vec<(String, String)>>>,
    supervisor: Arc<dyn RunSupervisor>,
}
#[async_trait]
impl InferenceProvider for Worker {
    async fn chat(
        &self,
        _: ChatRequest,
        _: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        panic!("worker must receive the fabric hop path")
    }
}
#[async_trait]
impl FabricNodeProvider for Worker {
    fn node_id(&self) -> &str {
        &self.id
    }
    fn label(&self) -> &str {
        &self.id
    }
    fn fabric_capabilities(&self) -> tetonic_fabric_protocol::WorkerCapabilityAdvertisement {
        tetonic_fabric_protocol::WorkerCapabilityAdvertisement::fabric_v1_full(vec![
            tetonic_fabric_protocol::JobKind::Infer,
        ])
    }
    async fn probe_node(&self) -> Option<NodeInfo> {
        Some(NodeInfo {
            id: self.id.clone(),
            label: self.id.clone(),
            vram_total_mb: 24000,
            vram_free_mb: 24000,
            resident_models: vec!["proof-model".into()],
            queue_depth: 0,
            healthy: true,
            models_verified: true,
            capacity: None,
            legacy_v1_chat_only: false,
            negotiated_protocol_version: Some(1),
        })
    }
    async fn chat_on_fabric(
        &self,
        _: ChatRequest,
        _: &str,
        _: &str,
        fabric: Option<&FabricCallMeta>,
        _: Option<&str>,
        _: Option<&ActiveJobRegistry>,
        _: &mut TokenSink<'_>,
    ) -> Result<ChatResponse, InferenceError> {
        let meta = fabric.expect("managed inference correlation");
        let agent_run = RunId::new(meta.run_id.as_ref().expect("agent run"));
        let agent_attempt = AttemptId::new(meta.attempt_id.as_ref().expect("agent attempt"));
        let snapshot = self.supervisor.snapshot(agent_run.clone()).await.unwrap();
        assert_eq!(
            snapshot.attempts[&agent_attempt].state,
            AttemptState::Running
        );
        assert!(snapshot.attempts[&agent_attempt].execution_claimed);
        let hop_run = meta.hop_run_id.clone().expect("hop run");
        let hop_attempt = meta.hop_attempt_id.clone().expect("hop attempt");
        assert_ne!(hop_run, agent_run.0);
        assert_ne!(hop_attempt, agent_attempt.0);
        self.hops.lock().unwrap().push((hop_run, hop_attempt));
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(InferenceError::Provider(
                "injected worker connection lost".into(),
            ));
        }
        Ok(ChatResponse {
            message: Message::assistant("").with_tool_calls(vec![ToolCall {
                function: FunctionCall {
                    name: "finish".into(),
                    arguments: serde_json::json!({"summary": "managed failover completed"}),
                },
            }]),
            usage: GenUsage::default(),
            provenance: InferenceProvenance::default(),
        })
    }
}
struct Redactions;
impl tetonic_domain::secrets::OutboundRedactionSink for Redactions {
    fn record(&self, _: &tetonic_domain::secrets::OutboundRedaction) -> Result<(), String> {
        Ok(())
    }
}

#[tokio::test]
async fn managed_loop_survives_real_broker_failover() {
    let tmp = tempfile::tempdir().unwrap();
    let store = tetonic_memory::SharedStore::open(tmp.path().join("managed.db"), 1).unwrap();
    let supervisor: Arc<dyn RunSupervisor> =
        Arc::new(tetonic_run::DurableRunSupervisor::new(Some(store.clone())));
    let artifacts = Arc::new(
        tetonic_artifact::LocalArtifactStore::new(
            tmp.path().join("artifacts"),
            tetonic_app::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );
    let policy = Arc::new(tetonic_policy::PolicyEngine::default());
    let manager =
        tetonic_run::ManagedRunService::new(supervisor.clone(), Some(store), artifacts, policy);
    let calls = Arc::new(AtomicUsize::new(0));
    let hops = Arc::new(Mutex::new(Vec::new()));
    let mut registry = tetonic_fabric_protocol::CapabilityRegistry::new();
    let mut workers: Vec<Arc<dyn FabricNodeProvider>> = Vec::new();
    for id in ["proof-worker-a", "proof-worker-b"] {
        let worker_id = WorkerId::new(id);
        let caps = tetonic_fabric_protocol::WorkerCapabilities::legacy_infer_profile(
            worker_id.clone(),
            "boot",
            1,
            0,
            &["proof-model".into()],
            &["proof-model".into()],
            32768,
            8192,
            0,
            0,
            2,
        );
        registry
            .upsert_validated(caps, &worker_id, 0, chrono::Utc::now())
            .unwrap();
        workers.push(Arc::new(Worker {
            id: id.into(),
            calls: calls.clone(),
            hops: hops.clone(),
            supervisor: supervisor.clone(),
        }));
    }
    let registry = Arc::new(RwLock::new(registry));
    // An unavailable loopback endpoint is an additional eligible fallback;
    // both remote transports are deterministic fixtures.
    let local = Arc::new(OllamaProvider::new(
        "http://127.0.0.1:1",
        Arc::new(tetonic_egress::EgressGuard::new()),
    ));
    let pooled = Arc::new(
        PooledProvider::new(local, workers)
            .with_capability_registry(registry.clone())
            .with_dispatch_guard(Arc::new(tetonic_policy::PolicyDispatchGuard::new(
                Arc::new(tetonic_policy::PolicyEngine::default()),
            )))
            .with_worker_trust_resolver(Arc::new(|_| {
                Some((tetonic_domain::WorkerTrust::OwnerControlledEstate, 0))
            })),
    );
    let budgets = Arc::new(tetonic_broker::HierarchicalBudgetLedger::new(
        tetonic_broker::BudgetLimits::default(),
    ));
    let queue = Arc::new(tetonic_broker::QueueManager::new(
        Default::default(),
        Default::default(),
    ));
    let admission = Arc::new(tetonic_broker::HierarchicalAdmissionController::new(
        budgets, queue,
    ));
    let broker = Arc::new(tetonic_broker::DefaultComputeBroker::new(
        admission,
        Arc::new(tetonic_broker::InMemoryReservationStore::default()),
        Some(Arc::new(tetonic_broker::InferenceTargetAdapter::new(
            pooled,
        ))),
        Some(supervisor.clone()),
    ));
    broker.set_capability_registry(registry);
    let inference = Arc::new(
        tetonic_broker::BrokerInferenceProvider::new(broker).with_outbound_scanner(
            Arc::new(tetonic_secrets::ScannerEngine::default_engine()),
            Arc::new(Redactions),
        ),
    );
    let mut agent = tetonic_core::Agent::new(
        inference,
        tetonic_tools::Tools::new(tetonic_tools::Workspace::new(tmp.path()).unwrap(), false),
        tetonic_core::AgentConfig {
            model: "proof-model".into(),
            max_steps: 2,
            ..Default::default()
        },
    );
    let identity =
        tetonic_app::definition::CodingAgentDefinition::production().coding_identity_record();
    let command = tetonic_run::StartIdentityJobCommand {
        job_spec: tetonic_domain::AgentJobSpec {
            identity_id: identity.id.clone(),
            definition_digest: identity.bound_definition_digest.clone(),
            input_digest: tetonic_run::job_input_digest("complete the proof"),
            capability_bindings: vec![],
            artifact_bindings: vec![],
            recovery_id: identity.recovery_id.clone(),
        },
        identity,
        invocation: tetonic_domain::AgentInvocation {
            instructions: "finish after inference".into(),
            user_input: "complete the proof".into(),
            max_steps: 2,
            explain_turn: false,
            empty_tool_nudge: false,
            completion_tool: "finish".into(),
            discipline: Default::default(),
        },
    };
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        manager.start_identity_job(command, &mut agent),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        matches!(result.outcome, CandidateOutcome::Completed { .. }),
        "{:?}",
        result.outcome
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "first worker fails, another worker succeeds"
    );
    let recorded = hops.lock().unwrap().clone();
    assert_ne!(recorded[0].1, recorded[1].1);
    let failed = supervisor
        .snapshot(RunId::new(&recorded[0].0))
        .await
        .unwrap();
    assert_eq!(
        failed.attempts[&AttemptId::new(&recorded[0].1)].state,
        AttemptState::Failed
    );
    let agent_snapshot = supervisor.snapshot(result.run_id).await.unwrap();
    assert_eq!(agent_snapshot.state, tetonic_domain::RunState::Succeeded);
    assert!(!agent_snapshot.cancellation.run_canceled);
}

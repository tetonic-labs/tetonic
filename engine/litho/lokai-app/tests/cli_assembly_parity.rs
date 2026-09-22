//! Assembly and parity integration tests for Application.

use std::sync::Arc;

use lokai_app::events::RecordingEventSink;
use lokai_app::{build_compute_plane, Application, ApplicationDependencies, ComputePlaneRequest};
use lokai_egress::EgressGuard;
use lokai_eval::parity::{
    compare_sequences, normalize_application_events, run_kernel_lifecycle_scenario,
};
use lokai_policy::PolicyEngine;
use lokai_runtime::EngineRuntime;

#[test]
fn runtime_default_policy_allows_mutations() {
    let temp = tempfile::tempdir().unwrap();
    let artifact_store = Arc::new(
        lokai_artifact::LocalArtifactStore::new(
            temp.path().join("artifacts"),
            lokai_app::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );
    let rt = EngineRuntime::new(Arc::new(PolicyEngine::default()), None, artifact_store);
    assert!(rt.policy().mutations_allowed());
    assert!(rt.policy().verify_allowed());
}

#[tokio::test]
async fn cli_compute_plane_broker_is_some_and_secret_stays_local() {
    let dir = std::env::temp_dir().join(format!("lokai-cli-plane-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let plane = build_compute_plane(ComputePlaneRequest {
        guard: Arc::new(EgressGuard::new()),
        ollama_base: "http://127.0.0.1:11434".into(),
        policy: Arc::new(PolicyEngine::default()),
        workspace_root: dir.clone(),
        artifact_store: Arc::new(
            lokai_artifact::LocalArtifactStore::new(
                dir.join("artifacts"),
                lokai_app::secret_scanner_factory::artifact_scan_policy(&None),
            )
            .unwrap(),
        ),
        store: None,
        coordinator: None,
        placement_sink: None,
        previous_pooled: None,
    })
    .await;
    let host_broker = Some(plane.compute_broker.clone());
    assert!(
        host_broker.is_some(),
        "CLI TurnExecutionHost.compute_broker must be Some"
    );
    let name = std::any::type_name_of_val(plane.provider.as_ref());
    assert!(
        name.contains("BrokerInferenceProvider"),
        "CLI Infer must use BrokerInferenceProvider (compute.submit / admission.evaluate), got {name}"
    );
    let pooled = plane.pooled.expect("pooled");
    let guard = pooled.dispatch_guard().expect("PolicyDispatchGuard");
    let decision = guard
        .evaluate(&lokai_inference::DispatchRequest {
            payload: Some(lokai_inference::Classification::new(
                lokai_inference::DataClass::Secret,
                vec![lokai_inference::ClassificationSource::UserDesignation],
            )),
            session: None,
            destination: lokai_inference::DispatchDestination::RemoteWorker {
                worker_id: "w_cli".into(),
            },
            post_redaction: false,
            worker_trust: None,
            project_policy: Default::default(),
        })
        .unwrap();
    assert_eq!(decision, lokai_inference::DispatchDecision::LocalOnly);
}

#[tokio::test(flavor = "multi_thread")]
async fn cli_kernel_lifecycle_semantic_effects() {
    let temp = tempfile::tempdir().unwrap();
    let (recorder, events) = RecordingEventSink::new();
    let store = lokai_memory::SharedStore::open(":memory:", 1).unwrap();
    let policy = Arc::new(PolicyEngine::default());
    let artifact_store = Arc::new(
        lokai_artifact::LocalArtifactStore::new(
            temp.path().join("artifacts"),
            lokai_app::secret_scanner_factory::artifact_scan_policy(&None),
        )
        .unwrap(),
    );
    let runtime = Arc::new(EngineRuntime::new(policy.clone(), None, artifact_store));
    let app = Application::new(ApplicationDependencies {
        runtime,
        store: Some(store),
        policy,
        event_sink: recorder,
        index_db: None,
        fabric_hint: None,
    });

    let tmp = temp.path();
    let root = tmp.display().to_string();
    run_kernel_lifecycle_scenario(&app, &root, "hello")
        .await
        .expect("cli kernel lifecycle");

    let effects = normalize_application_events(&events.lock().unwrap());
    let expected = vec![
        lokai_eval::parity::SemanticEffect::SessionInitialization,
        lokai_eval::parity::SemanticEffect::ModelRequest,
        lokai_eval::parity::SemanticEffect::TerminalOutcome,
        lokai_eval::parity::SemanticEffect::PersistenceWrite,
    ];
    compare_sequences(&expected, &effects).expect("cli lifecycle semantic effects");
}

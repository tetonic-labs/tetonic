#![allow(clippy::field_reassign_with_default)]

use super::*;
use crate::legacy_result::{emit_accepted_stream_tokens, validate_result_identity};
use tetonic_inference::{FabricJobResult, GenUsageSerde, JobStatus, Message};

/// These tests seal artifacts to exercise dispatch revalidation, so they must
/// state a scan policy (M6). A scanner that finds nothing keeps the subject of
/// the test the digest check rather than secret detection; the real engine is
/// wired in `lokai-app` and exercised there.
fn clean_scan() -> tetonic_artifact::ScanPolicy {
    tetonic_artifact::ScanPolicy::Scan(std::sync::Arc::new(|_| false))
}

#[test]
fn validate_result_identity_rejects_job_mismatch() {
    let result = FabricJobResult {
        job_id: "job_a".into(),
        attempt_id: Some("att_a".into()),
        message: Message::assistant(""),
        usage: GenUsageSerde::default(),
        status: JobStatus::Ok,
        error: None,
        result_envelope: None,
    };
    assert!(validate_result_identity("job_b", Some("att_a"), &result).is_err());
    assert!(validate_result_identity("job_a", Some("att_a"), &result).is_ok());
}

#[test]
fn validate_result_identity_rejects_attempt_mismatch() {
    let result = FabricJobResult {
        job_id: "job_a".into(),
        attempt_id: Some("att_b".into()),
        message: Message::assistant(""),
        usage: GenUsageSerde::default(),
        status: JobStatus::Ok,
        error: None,
        result_envelope: None,
    };
    assert!(validate_result_identity("job_a", Some("att_a"), &result).is_err());
    assert!(validate_result_identity("job_a", Some("att_b"), &result).is_ok());
}

#[tokio::test]
async fn worker_revoked_before_dispatch_rejects_legacy_chat() {
    use tetonic_inference::FabricNodeProvider;

    let provider = RemoteNodeProvider::new(
        "w1".into(),
        "box".into(),
        "127.0.0.1".parse().unwrap(),
        9443,
        vec![],
        Arc::new(KeyPair::generate()),
        Arc::new(EgressGuard::new()),
        "estate".into(),
        Arc::new(AtomicU64::new(0)),
    );
    provider.mark_worker_revoked();

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
    let mut on_token = |_: &str| {};
    let err = provider
        .chat_on_fabric(
            req,
            "job_revoked",
            "att_revoked",
            None,
            None,
            None,
            &mut on_token,
        )
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("revoked"),
        "expected revocation error, got: {err}"
    );
}

#[tokio::test]
async fn chat_without_negotiate_fails_closed() {
    use tetonic_inference::FabricNodeProvider;

    let provider = RemoteNodeProvider::new(
        "w1".into(),
        "box".into(),
        "127.0.0.1".parse().unwrap(),
        9443,
        vec![],
        Arc::new(KeyPair::generate()),
        Arc::new(EgressGuard::new()),
        "estate".into(),
        Arc::new(AtomicU64::new(0)),
    );
    assert!(provider.negotiated_protocol_version().is_none());

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
    let mut on_token = |_: &str| {};
    let err = provider
        .chat_on_fabric(req, "job_nego", "att_nego", None, None, None, &mut on_token)
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("UnsupportedProtocolVersion"),
        "expected negotiate gate, got: {err}"
    );
}

#[test]
fn matching_negotiate_response_records_version_on_connection() {
    use tetonic_fabric_protocol::{
        negotiate_versions, IMMUTABLE_SECURITY_FEATURES, MAX_SUPPORTED_VERSION,
        MIN_SUPPORTED_VERSION,
    };

    let provider = RemoteNodeProvider::new(
        "w1".into(),
        "box".into(),
        "127.0.0.1".parse().unwrap(),
        9443,
        vec![],
        Arc::new(KeyPair::generate()),
        Arc::new(EgressGuard::new()),
        "estate".into(),
        Arc::new(AtomicU64::new(0)),
    );
    let req = RemoteNodeProvider::version_negotiation_request();
    assert_eq!(req.min_supported_version, MIN_SUPPORTED_VERSION);
    assert_eq!(req.max_supported_version, MAX_SUPPORTED_VERSION);
    for feat in IMMUTABLE_SECURITY_FEATURES {
        assert!(req.required_features.iter().any(|f| f == feat));
    }

    let resp = negotiate_versions(&req, MIN_SUPPORTED_VERSION, MAX_SUPPORTED_VERSION).unwrap();
    provider.record_negotiation(resp.clone());
    assert_eq!(
        provider.negotiated_protocol_version(),
        Some(resp.negotiated_version)
    );
    let session = provider.negotiated_session().expect("session");
    assert_eq!(session.negotiated_version, resp.negotiated_version);
    assert_eq!(session.active_features, resp.active_features);
}

#[test]
fn mismatched_stream_tokens_use_verified_content() {
    let buffered = vec!["hel".to_string(), "lo".to_string()];
    let mut out = String::new();
    let mut sink = |t: &str| out.push_str(t);
    emit_accepted_stream_tokens(&buffered, "HELLO_FULL", &mut sink);
    assert_eq!(out, "HELLO_FULL");
}

#[test]
fn stream_tokens_use_full_content_when_buffer_empty() {
    let mut out = String::new();
    let mut sink = |t: &str| out.push_str(t);
    emit_accepted_stream_tokens(&[], "full", &mut sink);
    assert_eq!(out, "full");
}

fn test_provider(root: PathBuf, store: Arc<dyn ArtifactStore>) -> RemoteNodeProvider {
    RemoteNodeProvider::new(
        "w1".into(),
        "box".into(),
        "127.0.0.1".parse().unwrap(),
        9443,
        vec![],
        Arc::new(KeyPair::generate()),
        Arc::new(EgressGuard::new()),
        "estate".into(),
        Arc::new(AtomicU64::new(0)),
    )
    .with_dispatch_state(root, store)
}

#[tokio::test]
async fn dispatch_revalidation_rejects_changed_workspace() {
    let dir = tempfile::tempdir().unwrap();
    let store: Arc<dyn ArtifactStore> = Arc::new(
        tetonic_artifact::LocalArtifactStore::new(dir.path().join("artifacts"), clean_scan())
            .unwrap(),
    );
    std::fs::write(dir.path().join("source.txt"), "before").unwrap();
    let expected =
        tetonic_transaction::version::capture_workspace_version(dir.path(), &[]).unwrap();
    let mut job = crate::build_infer_job_envelope(
        &crate::LegacyInferParams {
            job_id: "job".into(),
            attempt_id: "att".into(),
            data_class: DataClass::RepositorySource,
            run_id: Some("run".into()),
            task_id: Some("task".into()),
        },
        &[Message::user("hello")],
        "qwen:7b",
    )
    .unwrap();
    job.workspace_version = Some(expected);
    std::fs::write(dir.path().join("source.txt"), "after").unwrap();

    let provider = test_provider(dir.path().to_path_buf(), store);
    assert!(provider.revalidate_dispatch_state(&job).await.is_err());
}

#[tokio::test]
async fn dispatch_revalidation_rejects_changed_artifact_digest() {
    use tetonic_domain::artifact::{ArtifactDeclaration, ArtifactKind, RetentionPolicy};
    use tetonic_domain::{AttemptId, RunId, TaskId};

    let dir = tempfile::tempdir().unwrap();
    let store: Arc<dyn ArtifactStore> = Arc::new(
        tetonic_artifact::LocalArtifactStore::new(dir.path().join("artifacts"), clean_scan())
            .unwrap(),
    );
    let mut writer = store
        .begin_write(ArtifactDeclaration {
            kind: ArtifactKind::ContextPack,
            producer_run_id: RunId::new("run"),
            producer_task_id: TaskId::new("task"),
            producer_attempt_id: AttemptId::new("att"),
            worker_id: None,
            workspace_version: None,
            data_class: DataClass::RepositorySource,
            retention_policy: RetentionPolicy::UntilRunCompletes,
        })
        .await
        .unwrap();
    writer.write_chunk(b"context").await.unwrap();
    let metadata = writer.seal().await.unwrap();
    let mut job = crate::build_infer_job_envelope(
        &crate::LegacyInferParams {
            job_id: "job".into(),
            attempt_id: "att".into(),
            data_class: DataClass::RepositorySource,
            run_id: Some("run".into()),
            task_id: Some("task".into()),
        },
        &[Message::user("hello")],
        "qwen:7b",
    )
    .unwrap();
    job.input_artifacts = vec![ArtifactReference {
        artifact_id: metadata.artifact_id.to_string(),
        digest: tetonic_domain::ContentDigest::new("sha256:stale"),
    }];

    let provider = test_provider(dir.path().to_path_buf(), store);
    assert!(provider.revalidate_dispatch_state(&job).await.is_err());
}

#[test]
fn typed_caps_select_jobs_endpoint() {
    assert_eq!(fabric_infer_endpoint(true), "/v1/chat");
    assert_eq!(fabric_infer_endpoint(false), "/v1/jobs");
}

#[test]
fn typed_jobs_501_refuses_without_chat_fallback() {
    assert!(typed_jobs_status_refuses_infer(501));
    assert!(typed_jobs_status_refuses_infer(404));
    assert!(typed_jobs_status_refuses_infer(405));
    assert!(!typed_jobs_status_refuses_infer(200));
    assert!(!typed_jobs_status_refuses_infer(503));
}

#[test]
fn typed_lease_ttl_honors_env_override() {
    std::env::set_var("LOKAI_FABRIC_LEASE_TTL_MS", "900");
    assert_eq!(
        typed_fabric_lease_ttl(),
        std::time::Duration::from_millis(900)
    );
    let interval = typed_fabric_renew_interval(typed_fabric_lease_ttl());
    assert!(interval <= std::time::Duration::from_millis(300));
    std::env::remove_var("LOKAI_FABRIC_LEASE_TTL_MS");
}

#[test]
fn set_fabric_capabilities_updates_cached_advertisement() {
    let provider = RemoteNodeProvider::new(
        "w1".into(),
        "box".into(),
        "127.0.0.1".parse().unwrap(),
        9443,
        vec![],
        Arc::new(KeyPair::generate()),
        Arc::new(EgressGuard::new()),
        "estate".into(),
        Arc::new(AtomicU64::new(0)),
    );
    assert!(provider.fabric_capabilities().legacy_v1_chat_only);
    provider.set_fabric_capabilities(WorkerCapabilityAdvertisement::fabric_v1_full(vec![
        tetonic_fabric_protocol::JobKind::Infer,
    ]));
    let caps = provider.fabric_capabilities();
    assert!(!caps.legacy_v1_chat_only);
    assert!(caps.supports_cancellation);
    assert_eq!(fabric_infer_endpoint(caps.legacy_v1_chat_only), "/v1/jobs");
}

#[test]
fn inventory_mismatch_sets_models_verified_false() {
    let inventory = vec!["fake-model:latest".into(), "real:7b".into()];
    let verified = vec!["real:7b".into()];
    assert!(!inventory_matches_verified(
        &inventory,
        Some(verified.as_slice())
    ));
    assert!(inventory_matches_verified(
        &["real:7b".into()],
        Some(verified.as_slice())
    ));
    assert!(!inventory_matches_verified(&inventory, None));
}

#[tokio::test]
async fn remote_chat_refuses_unstamped_request() {
    use tetonic_inference::{ChatRequest, FabricCallMeta, InferenceError, InferenceProvider};

    let provider = RemoteNodeProvider::new(
        "w1".into(),
        "box".into(),
        "127.0.0.1".parse().unwrap(),
        9443,
        vec![],
        Arc::new(KeyPair::generate()),
        Arc::new(EgressGuard::new()),
        "estate".into(),
        Arc::new(AtomicU64::new(0)),
    );
    let req = ChatRequest {
            max_tokens: None,
        model: "qwen:7b".into(),
        messages: vec![Message::user("hello")],
        fabric: Some(FabricCallMeta {
            data_class: DataClass::RepositorySource,
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut on_token = |_: &str| {};
    let err = provider.chat(req, &mut on_token).await.unwrap_err();
    assert!(
        matches!(err, InferenceError::SecretScanFailed { ref reason } if reason.contains("stamp")),
        "got {err}"
    );
}

use super::tests::{base_request, expansion_request, make_evidence, MockProvider};
use crate::pipeline::ContextCompiler;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tetonic_domain::{classify::DataClass, ContextExpansionRequest};

async fn fixture() -> (
    Arc<ContextCompiler>,
    ContextExpansionRequest,
    Arc<AtomicUsize>,
) {
    let reads = Arc::new(AtomicUsize::new(0));
    let mut provider = MockProvider {
        file_reads: Some(reads.clone()),
        ..Default::default()
    };
    provider.text_results.push(make_evidence(
        "safe excerpt",
        Some("src/file.rs"),
        DataClass::RepositorySource,
    ));
    provider
        .file_content
        .insert("src/file.rs".into(), "owned content".into());
    let compiler = Arc::new(ContextCompiler::new(Arc::new(provider)));
    let req = base_request();
    let fp = req.workspace_version.state_fingerprint();
    let pack = compiler.compile(req).await.unwrap();
    let request = expansion_request(&pack.expansion_handles[0].handle_id.0, &fp);
    reads.store(0, Ordering::SeqCst);
    (compiler, request, reads)
}

#[tokio::test]
async fn expansion_limits_reject_oversized_output_and_handle_growth() {
    let (compiler, request, _) = fixture().await;
    let compiler = Arc::try_unwrap(compiler)
        .ok()
        .unwrap()
        .with_expansion_limits(crate::pipeline::ExpansionLimits {
            max_live_handles: 1,
            max_text_bytes: 2,
            ..Default::default()
        });
    assert!(compiler
        .expand_pack(&request)
        .await
        .unwrap_err()
        .contains("output exceeds"));
    assert!(compiler
        .compile(base_request())
        .await
        .unwrap_err()
        .to_string()
        .contains("handle limit"));
    assert_eq!(compiler.handles.lock().unwrap().len(), 1);
}

struct StalledScanner;

struct PausedScanner {
    entered: Arc<tokio::sync::Notify>,
    resume: Arc<tokio::sync::Notify>,
}
#[async_trait::async_trait]
impl crate::interfaces::SecretScanner for PausedScanner {
    async fn scan_and_redact(
        &self,
        _: &str,
        _: Option<&str>,
    ) -> Result<tetonic_domain::secrets::ScanOutcome, String> {
        self.entered.notify_one();
        self.resume.notified().await;
        Ok(None)
    }
}

#[tokio::test]
async fn session_revocation_rejects_inflight_and_future_expansion() {
    use tetonic_domain::ContextCompiler as _;
    let (compiler, request, _) = fixture().await;
    let entered = Arc::new(tokio::sync::Notify::new());
    let resume = Arc::new(tokio::sync::Notify::new());
    let compiler = Arc::new(
        Arc::try_unwrap(compiler)
            .ok()
            .unwrap()
            .with_secret_scanner(Arc::new(PausedScanner {
                entered: entered.clone(),
                resume: resume.clone(),
            })),
    );
    let worker = compiler.clone();
    let worker_request = request.clone();
    let task = tokio::spawn(async move { worker.expand_pack(&worker_request).await });
    tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
        .await
        .unwrap();
    compiler.invalidate_session(&request.session_id).unwrap();
    resume.notify_one();
    assert!(task.await.unwrap().unwrap_err().contains("revoked"));
    assert!(compiler
        .expand_pack(&request)
        .await
        .unwrap_err()
        .contains("unknown"));
    assert!(compiler.handles.lock().unwrap().is_empty());
}

#[tokio::test]
async fn session_revocation_prevents_inflight_compile_from_registering_handles() {
    use tetonic_domain::ContextCompiler as _;
    let (compiler, request, _) = fixture().await;
    let entered = Arc::new(tokio::sync::Notify::new());
    let resume = Arc::new(tokio::sync::Notify::new());
    let compiler = Arc::new(
        Arc::try_unwrap(compiler)
            .ok()
            .unwrap()
            .with_secret_scanner(Arc::new(PausedScanner {
                entered: entered.clone(),
                resume: resume.clone(),
            })),
    );
    let worker = compiler.clone();
    let task = tokio::spawn(async move { worker.compile(base_request()).await });
    tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
        .await
        .unwrap();
    compiler.invalidate_session(&request.session_id).unwrap();
    resume.notify_one();
    assert!(task
        .await
        .unwrap()
        .unwrap_err()
        .to_string()
        .contains("revoked"));
    assert!(compiler.handles.lock().unwrap().is_empty());
    assert!(compiler
        .compile(base_request())
        .await
        .unwrap_err()
        .to_string()
        .contains("revoked"));
}

struct SignalingScanner(Arc<tokio::sync::Notify>);
#[async_trait::async_trait]
impl crate::interfaces::SecretScanner for SignalingScanner {
    async fn scan_and_redact(
        &self,
        _: &str,
        _: Option<&str>,
    ) -> Result<tetonic_domain::secrets::ScanOutcome, String> {
        self.0.notify_one();
        std::future::pending().await
    }
}

#[tokio::test]
async fn concurrency_rejection_does_not_read_or_consume_and_abort_releases_slot() {
    for global in [true, false] {
        let (compiler, request, reads) = fixture().await;
        let entered = Arc::new(tokio::sync::Notify::new());
        let compiler = Arc::new(
            Arc::try_unwrap(compiler)
                .ok()
                .unwrap()
                .with_secret_scanner(Arc::new(SignalingScanner(entered.clone())))
                .with_expansion_limits(crate::pipeline::ExpansionLimits {
                    max_concurrent: if global { 1 } else { 8 },
                    max_concurrent_per_owner: if global { 8 } else { 1 },
                    ..Default::default()
                }),
        );
        let task_compiler = compiler.clone();
        let task_request = request.clone();
        let task = tokio::spawn(async move { task_compiler.expand_pack(&task_request).await });
        tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
            .await
            .unwrap();
        assert!(compiler
            .expand_pack(&request)
            .await
            .unwrap_err()
            .contains("concurrency limit"));
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert_eq!(compiler.handles.lock().unwrap()[&request.handle_id].uses, 1);
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        let task_compiler = compiler.clone();
        let task = tokio::spawn(async move { task_compiler.expand_pack(&request).await });
        tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
            .await
            .unwrap();
        assert_eq!(reads.load(Ordering::SeqCst), 2);
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }
}
#[async_trait::async_trait]
impl crate::interfaces::SecretScanner for StalledScanner {
    async fn scan_and_redact(
        &self,
        _: &str,
        _: Option<&str>,
    ) -> Result<tetonic_domain::secrets::ScanOutcome, String> {
        std::future::pending().await
    }
}

#[tokio::test]
async fn stalled_expansion_scanner_obeys_deadline_and_consumes_attempt() {
    let (compiler, request, reads) = fixture().await;
    let compiler = Arc::try_unwrap(compiler)
        .ok()
        .unwrap()
        .with_secret_scanner(Arc::new(StalledScanner))
        .with_expansion_limits(crate::pipeline::ExpansionLimits {
            timeout: std::time::Duration::from_millis(10),
            ..Default::default()
        });
    let error = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        compiler.expand_pack(&request),
    )
    .await
    .expect("expansion must not hang")
    .unwrap_err();
    assert!(error.contains("deadline exceeded"));
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(compiler.handles.lock().unwrap()[&request.handle_id].uses, 1);
}

#[tokio::test]
async fn each_owner_dimension_is_required_before_reading_or_consuming_use() {
    let (compiler, request, reads) = fixture().await;
    // Exercise the domain trait used by core, not only the concrete helper.
    let port: &dyn tetonic_domain::ContextCompiler = compiler.as_ref();
    for dimension in 0..3 {
        let mut foreign = request.clone();
        match dimension {
            0 => foreign.session_id = tetonic_domain::SessionId::new("other"),
            1 => foreign.run_id = tetonic_domain::RunId::new("other"),
            _ => foreign.task_id = tetonic_domain::TaskId::new("other"),
        }
        let error = port.expand(&foreign).await.unwrap_err();
        assert!(error.contains("does not belong"));
        assert_eq!(reads.load(Ordering::SeqCst), 0);
        assert_eq!(compiler.handles.lock().unwrap()[&request.handle_id].uses, 0);
    }
    let result = port.expand(&request).await.unwrap();
    assert_eq!(result[0].text, "owned content");
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn simultaneous_valid_and_foreign_callers_cannot_steal_the_last_use() {
    let (compiler, request, reads) = fixture().await;
    compiler
        .handles
        .lock()
        .unwrap()
        .get_mut(&request.handle_id)
        .unwrap()
        .handle
        .max_uses = 1;
    let mut foreign = request.clone();
    foreign.session_id = tetonic_domain::SessionId::new("other");
    let (valid, denied, duplicate) = tokio::join!(
        compiler.expand_pack(&request),
        compiler.expand_pack(&foreign),
        compiler.expand_pack(&request)
    );
    assert!(duplicate.unwrap_err().contains("exhausted"));
    assert_eq!(valid.unwrap()[0].text, "owned content");
    assert!(denied.unwrap_err().contains("does not belong"));
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert!(compiler
        .expand_pack(&request)
        .await
        .unwrap_err()
        .contains("exhausted"));
}

#[tokio::test]
async fn handles_without_session_ownership_are_not_deserialized() {
    let (compiler, request, _) = fixture().await;
    let handle = compiler.handles.lock().unwrap()[&request.handle_id]
        .handle
        .clone();
    let mut serialized = serde_json::to_value(handle).unwrap();
    serialized.as_object_mut().unwrap().remove("session_id");
    assert!(serde_json::from_value::<crate::types::ExpansionHandle>(serialized).is_err());
}

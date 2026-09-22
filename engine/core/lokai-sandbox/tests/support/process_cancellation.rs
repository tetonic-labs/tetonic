//! Async waiter lifetime must not stand in for process-worker lifetime.
use super::*;
use lokai_domain::work_scope::{CancellationSignal, WorkScope};
use lokai_sandbox::{
    SandboxBackend, SandboxCapabilities, SandboxError, SandboxRequest, SandboxedProcess,
};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct ControlledBackend {
    entered: tokio::sync::Notify,
    canceled: tokio::sync::Notify,
    cleanup: tokio::sync::Notify,
    calls: AtomicUsize,
}
#[async_trait::async_trait]
impl SandboxBackend for ControlledBackend {
    fn capabilities(&self) -> SandboxCapabilities {
        lokai_sandbox::platform_backend().capabilities()
    }
    async fn execute(&self, _: SandboxRequest) -> Result<SandboxedProcess, SandboxError> {
        panic!("process port lost the invocation signal")
    }
    async fn execute_cancellable(
        &self,
        _: SandboxRequest,
        signal: CancellationSignal,
    ) -> Result<SandboxedProcess, SandboxError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.notify_one();
        tokio::time::timeout(Duration::from_secs(5), async {
            while !signal.is_canceled() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("process port failed to deliver cancellation");
        self.canceled.notify_one();
        tokio::time::timeout(Duration::from_secs(5), self.cleanup.notified())
            .await
            .expect("fixture cleanup not released");
        Err(SandboxError::Canceled)
    }
}
fn fixture(
    tag: &str,
) -> (
    PathBuf,
    ProcessExecutor,
    AuthorizedAction,
    Arc<ControlledBackend>,
) {
    let path = std::env::temp_dir().join(format!(
        "lokai-process-scope-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&path).unwrap();
    let path = path.canonicalize().unwrap();
    let backend = Arc::new(ControlledBackend::default());
    let executor = ProcessExecutor::new(
        &path,
        EnforcementLevel::Sandboxed,
        Arc::new(NonCodingProcessValidator),
    )
    .with_sandbox_backend(backend.clone());
    let auth = build_authorized_action(
        &path,
        "cancel",
        ActionKind::ExecuteProcess,
        Some("fixture".into()),
        vec![],
        None,
        Some(path.display().to_string()),
    );
    (path, executor, auth, backend)
}
async fn invoke(
    executor: ProcessExecutor,
    auth: AuthorizedAction,
    scope: WorkScope,
    broker: bool,
) -> bool {
    if broker {
        executor
            .execute(AuthorizedProcessRequest {
                authorized_action: auth,
                work_scope: scope,
            })
            .await
            .is_ok()
    } else {
        matches!(
            executor.run_process(&auth, &scope).await,
            ExecutionOutcome::Completed { ok: true, .. }
        )
    }
}
async fn notified(notify: &tokio::sync::Notify) {
    tokio::time::timeout(Duration::from_secs(3), notify.notified())
        .await
        .unwrap();
}

#[tokio::test]
async fn both_ports_keep_leases_until_cancellation_cleanup_finishes() {
    for broker in [false, true] {
        let (path, executor, auth, backend) = fixture("wait");
        let scope = WorkScope::default();
        let task = tokio::spawn(invoke(executor, auth, scope.clone(), broker));
        notified(&backend.entered).await;
        scope.cancel();
        notified(&backend.canceled).await;
        assert!(!scope.is_quiescent());
        assert!(!task.is_finished());
        backend.cleanup.notify_one();
        assert!(!tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap());
        assert!(scope.is_quiescent());
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[tokio::test]
async fn dropped_waiter_cannot_release_the_actual_process_worker() {
    for broker in [false, true] {
        let (path, executor, auth, backend) = fixture("drop");
        let scope = WorkScope::default();
        let task = tokio::spawn(invoke(executor, auth, scope.clone(), broker));
        notified(&backend.entered).await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(!scope.is_quiescent());
        scope.cancel();
        notified(&backend.canceled).await;
        assert!(!scope.is_quiescent());
        backend.cleanup.notify_one();
        tokio::time::timeout(Duration::from_secs(3), async {
            while !scope.is_quiescent() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[tokio::test]
async fn closed_scopes_and_invalid_capabilities_do_not_enter_backend_or_leak_leases() {
    for broker in [false, true] {
        let (path, executor, mut auth, backend) = fixture("deny");
        let closed = WorkScope::default();
        closed.cancel();
        assert!(!invoke(executor.clone(), auth.clone(), closed.clone(), broker).await);
        assert!(closed.is_quiescent());
        auth.capability.revoked = true;
        let live = WorkScope::default();
        assert!(!invoke(executor, auth, live.clone(), broker).await);
        assert!(live.is_quiescent());
        assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
        std::fs::remove_dir_all(path).unwrap();
    }
}

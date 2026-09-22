use super::*;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};
use tetonic_domain::work_scope::WorkScope;
use tetonic_domain::{
    ActionId, ActionKind, AgentId, DataClass, IssuedCapability, ProposedAction, SessionId,
};

fn authorized(dir: &std::path::Path) -> tetonic_domain::AuthorizedAction {
    let wv = tetonic_transaction::version::capture_workspace_version(dir, &[]).unwrap();
    let action = ProposedAction {
        action_id: ActionId::new("mut_auth"),
        session_id: SessionId::new("s"),
        run_id: None,
        task_id: None,
        attempt_id: None,
        agent_id: Some(AgentId::new("a0")),
        workspace_version: Some(wv),
        data_class: DataClass::RepositorySource,
        kind: ActionKind::ExecuteShell,
        parameters: tetonic_domain::execution::CanonicalActionParameters {
            digest: "digest".into(),
            executable_identity: None,
            resolved_path: None,
            arguments: vec![],
            shell_identity: None,
            shell_mode: None,
            script_bytes: Some(b"approved script".to_vec()),
            working_directory: Some(dir.display().to_string()),
            env_vars: None,
            stdin_source_classification: None,
            filesystem_access_scope: None,
            network_policy: None,
            resource_limits: None,
            process_class: None,
            sandbox_profile: None,
            expected_output_limits: None,
            schema_version: 1,
            tool_arguments: Some(serde_json::json!({ "command": "approved script" })),
        },
        requested_capabilities: Default::default(),
        trace_context: Default::default(),
    };
    tetonic_domain::AuthorizedAction {
        capability: IssuedCapability {
            capability_id: tetonic_domain::CapabilityId::new("cap_auth"),
            session_id: action.session_id.clone(),
            run_id: None,
            task_id: None,
            attempt_id: None,
            agent_id: action.agent_id.clone(),
            action_kind: ActionKind::ExecuteShell,
            canonical_parameter_digest: action.parameters.digest.clone(),
            workspace_version: action.workspace_version.clone(),
            data_classification: action.data_class,
            issuance_timestamp: 0,
            expiration: u64::MAX,
            max_use_count: 1,
            current_use_count: 0,
            issuing_policy_version: "v1".into(),
            approval_record_id: None,
            revoked: false,
        },
        action,
    }
}

struct Consumer {
    deny: AtomicBool,
    calls: AtomicUsize,
}
impl tetonic_domain::CapabilityConsumer for Consumer {
    fn authorize(
        &self,
        _: &tetonic_domain::AuthorizedAction,
    ) -> Result<(), tetonic_domain::CapabilityError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.deny.load(Ordering::Acquire) {
            Err(tetonic_domain::CapabilityError::ScopeMismatch)
        } else {
            Ok(())
        }
    }
}
struct BlockingBackend {
    entered: mpsc::Sender<()>,
    calls: AtomicUsize,
}
#[async_trait::async_trait]
impl tetonic_sandbox::SandboxBackend for BlockingBackend {
    fn capabilities(&self) -> tetonic_sandbox::SandboxCapabilities {
        tetonic_sandbox::platform_backend().capabilities()
    }
    async fn execute(
        &self,
        _: tetonic_sandbox::SandboxRequest,
    ) -> Result<tetonic_sandbox::SandboxedProcess, tetonic_sandbox::SandboxError> {
        panic!("signal was dropped before backend")
    }
    async fn execute_cancellable(
        &self,
        req: tetonic_sandbox::SandboxRequest,
        cancel: tetonic_domain::work_scope::CancellationSignal,
    ) -> Result<tetonic_sandbox::SandboxedProcess, tetonic_sandbox::SandboxError> {
        assert_eq!(req.shell_script.as_deref(), Some("approved script"));
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !cancel.is_canceled() {
            assert!(
                std::time::Instant::now() < deadline,
                "backend never received cancellation"
            );
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        Err(tetonic_sandbox::SandboxError::Canceled)
    }
}

#[test]
fn authorized_process_cancellation_is_local_and_keeps_authority_checks() {
    let dir = std::env::temp_dir().join(format!(
        "lokai-tool-cancel-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    let (entered, ready) = mpsc::channel();
    let backend = Arc::new(BlockingBackend {
        entered,
        calls: AtomicUsize::new(0),
    });
    let consumer = Arc::new(Consumer {
        deny: AtomicBool::new(false),
        calls: AtomicUsize::new(0),
    });
    let mut tools =
        Tools::new(Workspace::new(&dir).unwrap(), true).with_capability_consumer(consumer.clone());
    tools.executor = tools.executor.with_sandbox_backend(backend.clone());
    let tools = Arc::new(tools);
    let auth = authorized(&dir);
    let a = WorkScope::default();
    let b = WorkScope::default();
    let launch = |scope: &WorkScope| {
        let tools = tools.clone();
        let auth = auth.clone();
        let signal = scope.cancellation_signal();
        std::thread::spawn(move || {
            tools.execute_authorized_cancellable(
                "run_shell",
                &serde_json::json!({"command":"unapproved caller text"}),
                Some(&auth),
                Some(&signal),
            )
        })
    };
    let a_task = launch(&a);
    let b_task = launch(&b);
    ready
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    ready
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    a.cancel();
    let outcome = a_task.join().unwrap();
    assert!(!outcome.ok && outcome.content.contains("canceled"));
    assert!(!b_task.is_finished());
    b.cancel();
    assert!(!b_task.join().unwrap().ok);
    assert_eq!(consumer.calls.load(Ordering::SeqCst), 2);
    consumer.deny.store(true, Ordering::Release);
    let scope = WorkScope::default();
    assert!(
        !tools
            .execute_authorized_cancellable(
                "run_shell",
                &serde_json::json!({}),
                Some(&auth),
                Some(&scope.cancellation_signal())
            )
            .ok
    );
    assert_eq!(consumer.calls.load(Ordering::SeqCst), 3);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 2);
    scope.cancel();
    assert!(
        !tools
            .execute_authorized_cancellable(
                "run_shell",
                &serde_json::json!({}),
                Some(&auth),
                Some(&scope.cancellation_signal())
            )
            .ok
    );
    assert_eq!(consumer.calls.load(Ordering::SeqCst), 3);
    std::fs::remove_dir_all(dir).unwrap();
}

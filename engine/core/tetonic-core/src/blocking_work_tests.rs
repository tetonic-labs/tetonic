use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use tetonic_domain::work_scope::WorkScope;

#[tokio::test]
async fn dropped_waiter_does_not_release_blocking_worker() {
    let mut agent = Agent::placeholder();
    let scope = WorkScope::default();
    agent.bind_work_scope(scope.clone()).unwrap();
    let (entered, started) = tokio::sync::oneshot::channel();
    let (release, released) = std::sync::mpsc::channel();
    let wrote = Arc::new(AtomicBool::new(false));
    let flag = wrote.clone();
    let task = tokio::spawn(async move {
        agent
            .run_tools_blocking(move |_| {
                entered.send(()).unwrap();
                released
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
                flag.store(true, Ordering::SeqCst);
            })
            .await
    });
    started.await.unwrap();
    scope.cancel();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(
        !scope.is_quiescent(),
        "async Drop must not release the worker lease"
    );
    assert!(!wrote.load(Ordering::SeqCst));
    release.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !scope.is_quiescent() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(wrote.load(Ordering::SeqCst));
}

#[tokio::test]
async fn canceled_scope_refuses_new_work_and_busy_scope_refuses_rebinding() {
    let mut agent = Agent::placeholder();
    let scope = WorkScope::default();
    agent.bind_work_scope(scope.clone()).unwrap();
    let lease = scope.try_enter().unwrap();
    assert!(agent.bind_work_scope(WorkScope::default()).is_err());
    drop(lease);
    scope.cancel();
    assert!(agent
        .run_tools_blocking(|_| panic!("must not execute"))
        .await
        .is_err());
    assert!(scope.is_quiescent());
    agent.bind_work_scope(WorkScope::default()).unwrap();
    assert_eq!(agent.run_tools_blocking(|_| 42).await.unwrap(), 42);
}

#[derive(Clone)]
struct CancelAwareHost {
    entered: Arc<tokio::sync::Notify>,
    release: Arc<AtomicBool>,
}
impl ToolHost for CancelAwareHost {
    fn clone_box(&self) -> Box<dyn ToolHost> {
        Box::new(self.clone())
    }
    fn propose(&self, _: &str, _: &Value) -> Option<tetonic_domain::ToolProposal> {
        None
    }
    fn is_tool_allowed(&self, _: &str) -> bool {
        true
    }
    fn is_read_only(&self, _: &str) -> bool {
        true
    }
    fn advertisements(&self) -> Vec<tetonic_domain::ToolAdvertisement> {
        vec![]
    }
    fn validate_tool_args(&self, _: &str, _: &Value) -> Result<(), String> {
        Ok(())
    }
    fn execute_authorized(
        &self,
        _: &str,
        _: &Value,
        _: Option<&AuthorizedAction>,
        cancel: &tetonic_domain::work_scope::CancellationSignal,
    ) -> ToolOutcome {
        self.entered.notify_one();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !cancel.is_canceled() && !self.release.load(Ordering::Acquire) {
            assert!(
                std::time::Instant::now() < deadline,
                "tool did not receive its cancellation signal"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        ToolOutcome {
            ok: !cancel.is_canceled(),
            summary: "fixture".into(),
            content: String::new(),
            error_kind: None,
            change: None,
        }
    }
}
#[tokio::test]
async fn concurrent_attempts_observe_only_their_own_cancellation() {
    let a_scope = WorkScope::default();
    let b_scope = WorkScope::default();
    let a_entered = Arc::new(tokio::sync::Notify::new());
    let b_entered = Arc::new(tokio::sync::Notify::new());
    let b_release = Arc::new(AtomicBool::new(false));
    let mut a = Agent::placeholder();
    a.tools = Box::new(CancelAwareHost {
        entered: a_entered.clone(),
        release: Arc::new(AtomicBool::new(false)),
    });
    a.bind_work_scope(a_scope.clone()).unwrap();
    let mut b = Agent::placeholder();
    b.tools = Box::new(CancelAwareHost {
        entered: b_entered.clone(),
        release: b_release.clone(),
    });
    b.bind_work_scope(b_scope.clone()).unwrap();
    let a_task = tokio::spawn(async move {
        a.run_tool_execute("fixture", &serde_json::json!({}), None)
            .await
    });
    let b_task = tokio::spawn(async move {
        b.run_tool_execute("fixture", &serde_json::json!({}), None)
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        a_entered.notified().await;
        b_entered.notified().await;
    })
    .await
    .unwrap();
    a_scope.cancel();
    assert!(
        !tokio::time::timeout(std::time::Duration::from_secs(3), a_task)
            .await
            .unwrap()
            .unwrap()
            .ok
    );
    assert!(a_scope.is_quiescent());
    assert!(!b_task.is_finished());
    assert!(!b_scope.is_canceled());
    assert!(!b_scope.is_quiescent());
    b_release.store(true, Ordering::Release);
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(3), b_task)
            .await
            .unwrap()
            .unwrap()
            .ok
    );
    assert!(b_scope.is_quiescent());
}

//! Native Unix lifecycle regressions. All children and files belong to a fixture.
#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tetonic_domain::execution::ProcessClass;
use tetonic_sandbox::sync_service::SyncLongLivedService;
use tetonic_sandbox::{
    platform_backend, profile_for_class, IsolationLevel, NetworkPolicy, ProcessMode, SandboxError,
    SandboxRequest, SandboxedProcess,
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lokai-unix-lifecycle-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
    fn request(&self, mode: ProcessMode, exit: bool) -> SandboxRequest {
        let mut req = profile_for_class(ProcessClass::RepositoryTool, &self.0);
        req.shell_script = Some(format!(
            "/bin/sleep 30 & printf '%s %s\\n' \"$$\" \"$!\" > pids; {}",
            if exit { "printf done; exit 0" } else { "wait" }
        ));
        req.mode = mode;
        req.network = NetworkPolicy::InheritBrokered;
        req.isolation_level = IsolationLevel::Standard;
        req.resources.max_memory_bytes = None;
        req.resources.max_child_processes = None;
        req.runtime_limit = Duration::from_secs(10);
        req.filesystem.denied_paths.clear();
        req.filesystem.temporary_root = Some(self.0.clone());
        req
    }
    fn pids(&self) -> Option<Vec<i32>> {
        let text = std::fs::read_to_string(self.0.join("pids")).ok()?;
        let pids: Vec<i32> = text
            .split_whitespace()
            .filter_map(|p| p.parse().ok())
            .collect();
        (pids.len() == 2 && pids.iter().all(|p| *p > 1)).then_some(pids)
    }
    async fn wait_ready(&self) -> Vec<i32> {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(pids) = self.pids() {
                    return pids;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fixture did not start")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Failure cleanup as well; never target a process outside this fixture.
        if let Some(pids) = self.pids() {
            for pid in pids {
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                }
            }
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn assert_own_group(pids: &[i32]) {
    assert_eq!(unsafe { libc::getpgid(pids[0]) }, pids[0]);
    assert_eq!(unsafe { libc::getpgid(pids[1]) }, pids[0]);
    assert_ne!(pids[0], unsafe { libc::getpgrp() });
}

async fn assert_stopped(pids: &[i32]) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            // Grandchildren are reaped by init, not this test. A zombie is stopped
            // and cannot retain pipes; do not depend on the host's init reaping rate.
            let alive = pids.iter().any(|pid| {
                let out = std::process::Command::new("/bin/ps")
                    .args(["-o", "stat=", "-p", &pid.to_string()])
                    .output()
                    .unwrap();
                let state = String::from_utf8_lossy(&out.stdout);
                let state = state.trim();
                !state.is_empty() && !state.starts_with('Z')
            });
            if !alive {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("owned child survived cleanup");
}

#[tokio::test]
async fn sync_service_drop_stops_group() {
    let fixture = Fixture::new();
    let service =
        SyncLongLivedService::spawn(fixture.request(ProcessMode::LongLived, false)).unwrap();
    let pids = fixture.wait_ready().await;
    assert_own_group(&pids);
    assert!(service.is_alive());
    drop(service);
    assert_stopped(&pids).await;
}

#[tokio::test]
async fn sync_service_termination_is_repeatable() {
    let fixture = Fixture::new();
    let mut service =
        SyncLongLivedService::spawn(fixture.request(ProcessMode::LongLived, false)).unwrap();
    let pids = fixture.wait_ready().await;
    service.force_terminate().unwrap();
    service.force_terminate().unwrap();
    assert!(!service.is_alive());
    assert_stopped(&pids).await;
}

#[tokio::test]
async fn async_service_drop_stops_group() {
    let fixture = Fixture::new();
    let result = platform_backend()
        .execute(fixture.request(ProcessMode::LongLived, false))
        .await
        .unwrap();
    let SandboxedProcess::Service(service) = result else {
        panic!("expected service")
    };
    let pids = fixture.wait_ready().await;
    assert_own_group(&pids);
    assert!(service.is_alive());
    drop(service);
    assert_stopped(&pids).await;
}

#[tokio::test]
async fn async_service_termination_is_repeatable() {
    let fixture = Fixture::new();
    let result = platform_backend()
        .execute(fixture.request(ProcessMode::LongLived, false))
        .await
        .unwrap();
    let SandboxedProcess::Service(mut service) = result else {
        panic!("expected service")
    };
    let pids = fixture.wait_ready().await;
    service.force_terminate().await.unwrap();
    service.force_terminate().await.unwrap();
    assert!(!service.is_alive());
    assert_stopped(&pids).await;
}

#[tokio::test]
async fn successful_leader_exit_cleans_up_descendant_pipe_holders() {
    let fixture = Fixture::new();
    let began = Instant::now();
    let result = platform_backend()
        .execute(fixture.request(ProcessMode::OneShot, true))
        .await
        .unwrap();
    let SandboxedProcess::Completed(result) = result else {
        panic!("expected result")
    };
    assert!(result.success);
    assert_eq!(result.stdout, "done");
    assert!(began.elapsed() < Duration::from_secs(5));
    assert_stopped(&fixture.pids().unwrap()).await;
}

#[tokio::test]
async fn runtime_deadline_stops_group() {
    let fixture = Fixture::new();
    let mut request = fixture.request(ProcessMode::OneShot, false);
    request.runtime_limit = Duration::from_secs(1);
    let result = platform_backend().execute(request).await;
    assert!(matches!(result, Err(SandboxError::Timeout)));
    assert_stopped(&fixture.pids().unwrap()).await;
}

#[tokio::test]
async fn dropping_execution_future_stops_group() {
    let fixture = Fixture::new();
    let request = fixture.request(ProcessMode::OneShot, false);
    let task = tokio::spawn(async move { platform_backend().execute(request).await });
    let pids = fixture.wait_ready().await;
    assert_own_group(&pids);
    task.abort();
    assert!(matches!(task.await, Err(error) if error.is_cancelled()));
    assert_stopped(&pids).await;
}

#[tokio::test]
async fn cancellation_signal_stops_active_group() {
    let fixture = Fixture::new();
    let request = fixture.request(ProcessMode::OneShot, false);
    let scope = tetonic_domain::work_scope::WorkScope::default();
    let signal = scope.cancellation_signal();
    let task = tokio::spawn(async move {
        platform_backend()
            .execute_cancellable(request, signal)
            .await
    });
    let pids = fixture.wait_ready().await;
    scope.cancel();
    let result = tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(result, Err(SandboxError::Canceled)));
    assert_stopped(&pids).await;
}

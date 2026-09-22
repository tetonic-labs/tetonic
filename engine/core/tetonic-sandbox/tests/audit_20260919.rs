//! Windows audit probes; explicit opt-in, bounded owned fixtures only.
#![cfg(windows)]
use std::time::{Duration, Instant};
use tetonic_domain::execution::ProcessClass;
use tetonic_sandbox::{platform_backend, profile_for_class, IsolationLevel, NetworkPolicy};

#[tokio::test]
#[ignore = "requires the bounded native child fixture via LOKAI_AUDIT_CHILD_HELPER"]
async fn descendant_pipe_holder_cannot_bypass_runtime_deadline() {
    let helper = std::env::var("LOKAI_AUDIT_CHILD_HELPER").unwrap();
    let workspace = std::path::Path::new(&helper).parent().unwrap();
    let mut req = profile_for_class(ProcessClass::RepositoryTool, workspace);
    req.executable = helper.clone();
    req.runtime_limit = Duration::from_millis(500);
    req.network = NetworkPolicy::InheritBrokered;
    req.isolation_level = IsolationLevel::Standard;
    let began = Instant::now();
    let result = platform_backend().execute(req).await;
    let elapsed = began.elapsed();
    assert!(result.is_ok(), "{:?}", result.err());
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");
    println!("Regression: 500ms bounded process returned after {elapsed:?}");
}

#[tokio::test]
#[ignore = "native handle accounting; run serially"]
async fn failed_spawn_releases_windows_handles() {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};
    fn handles() -> u32 {
        let mut count = 0;
        assert_ne!(
            unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) },
            0
        );
        count
    }
    let workspace = std::env::current_dir().unwrap();
    let mut req = profile_for_class(ProcessClass::RepositoryTool, &workspace);
    req.executable = workspace
        .join("nonexistent-audit-fixture.exe")
        .display()
        .to_string();
    req.network = NetworkPolicy::InheritBrokered;
    req.isolation_level = IsolationLevel::Standard;
    let backend = platform_backend();
    // Prime the blocking pool and any lazy initialization before counting.
    assert!(backend.execute(req.clone()).await.is_err());
    let before = handles();
    for _ in 0..12 {
        assert!(backend.execute(req.clone()).await.is_err());
    }
    let after = handles();
    assert!(after <= before + 2, "before={before}, after={after}");
    println!("Regression: 12 failed spawns measured OS handles from {before} to {after}");
}

#[test]
#[ignore = "requires bounded native fixture; run serially"]
fn synchronous_service_drop_and_failed_spawn_release_handles() {
    use tetonic_sandbox::sync_service::SyncLongLivedService;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};
    fn handles() -> u32 {
        let mut count = 0;
        assert_ne!(
            unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) },
            0
        );
        count
    }
    let helper = std::env::var("LOKAI_AUDIT_CHILD_HELPER").unwrap();
    let mut request = profile_for_class(
        ProcessClass::RepositoryTool,
        std::path::Path::new(&helper).parent().unwrap(),
    );
    request.executable = helper;
    request.arguments = vec!["child".into()];
    request.network = NetworkPolicy::InheritBrokered;
    request.isolation_level = IsolationLevel::Standard;
    drop(SyncLongLivedService::spawn(request.clone()).unwrap());
    let before = handles();
    for _ in 0..12 {
        let mut service = SyncLongLivedService::spawn(request.clone()).unwrap();
        assert!(service.is_alive());
        service.force_terminate().unwrap();
        service.force_terminate().unwrap();
        assert!(!service.is_alive());
        drop(service);
        drop(SyncLongLivedService::spawn(request.clone()).unwrap());
        let mut invalid = request.clone();
        invalid.executable = "nonexistent-audit-fixture.exe".into();
        assert!(SyncLongLivedService::spawn(invalid).is_err());
    }
    let after = handles();
    assert!(after <= before + 2, "before={before} after={after}");
}

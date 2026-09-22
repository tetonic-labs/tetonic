use super::path_for_create_process_cwd;
use std::path::Path;

#[tokio::test]
#[ignore = "requires bounded native fixture via LOKAI_AUDIT_CHILD_HELPER"]
async fn service_drop_kills_child_and_stop_is_repeatable() {
    use super::*;
    use windows_sys::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let helper = std::env::var("LOKAI_AUDIT_CHILD_HELPER").unwrap();
    for explicit_stop in [false, true] {
        let mut request = crate::profile_for_class(
            tetonic_domain::execution::ProcessClass::RepositoryTool,
            Path::new(&helper).parent().unwrap(),
        );
        request.executable = helper.clone();
        request.arguments = vec!["child".into()];
        request.mode = ProcessMode::LongLived;
        request.network = crate::NetworkPolicy::InheritBrokered;
        request.isolation_level = crate::IsolationLevel::Standard;
        let SandboxedProcess::Service(mut service) =
            WindowsSandboxBackend.execute(request).await.unwrap()
        else {
            panic!("expected service");
        };
        let LongLivedInner::Windows(inner) = &service.inner;
        let child = inner.child.lock().await;
        let mut observed = ptr::null_mut();
        unsafe {
            let process = GetCurrentProcess();
            assert_ne!(
                DuplicateHandle(
                    process,
                    child.process,
                    process,
                    &mut observed,
                    0,
                    0,
                    DUPLICATE_SAME_ACCESS
                ),
                0
            );
        }
        let observed = OwnedNativeHandle(observed);
        drop(child);
        if explicit_stop {
            service.force_terminate().await.unwrap();
            service.force_terminate().await.unwrap();
        }
        drop(service);
        assert_eq!(
            unsafe { WaitForSingleObject(observed.0, 2000) },
            WAIT_OBJECT_0
        );
    }
}

#[test]
fn strips_verbatim_drive_prefix_for_cmd_cwd() {
    assert_eq!(
        path_for_create_process_cwd(Path::new(r"\\?\C:\debug")),
        r"C:\debug"
    );
    assert_eq!(
        path_for_create_process_cwd(Path::new(r"C:\debug")),
        r"C:\debug"
    );
    assert_eq!(
        path_for_create_process_cwd(Path::new(r"\\?\UNC\server\share\dir")),
        r"\\server\share\dir"
    );
}

#[test]
fn cancellation_fixture_child() {
    use std::os::windows::process::CommandExt;
    let Ok(root) = std::env::var("LOKAI_CANCEL_TEST_ROOT") else {
        return;
    };
    let grandchild = std::env::var_os("LOKAI_CANCEL_TEST_GRANDCHILD").is_some();
    if !grandchild {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "backend::windows::tests::cancellation_fixture_child",
                "--nocapture",
            ])
            .env("LOKAI_CANCEL_TEST_GRANDCHILD", "1")
            .creation_flags(0x08000000)
            .spawn()
            .unwrap();
        std::fs::write(
            Path::new(&root).join("parent.pid"),
            std::process::id().to_string(),
        )
        .unwrap();
        let _ = child.wait();
    } else {
        std::fs::write(
            Path::new(&root).join("child.pid"),
            std::process::id().to_string(),
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_secs(15));
        std::fs::write(Path::new(&root).join("late-effect"), "should not happen").unwrap();
    }
}

#[tokio::test]
async fn cancellation_waits_for_entire_windows_job() {
    use super::*;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE};
    let root = std::env::temp_dir().join(format!(
        "lokai-cancel-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let mut request = crate::profile_for_class(
        tetonic_domain::execution::ProcessClass::RepositoryTool,
        &root,
    );
    request.executable = std::env::current_exe().unwrap().display().to_string();
    request.arguments = vec![
        "--exact".into(),
        "backend::windows::tests::cancellation_fixture_child".into(),
        "--nocapture".into(),
    ];
    request
        .environment
        .extra_vars
        .push(("LOKAI_CANCEL_TEST_ROOT".into(), root.display().to_string()));
    request.network = crate::NetworkPolicy::InheritBrokered;
    request.runtime_limit = std::time::Duration::from_secs(20);
    let scope = tetonic_domain::work_scope::WorkScope::default();
    let signal = scope.cancellation_signal();
    let task = tokio::spawn(async move {
        WindowsSandboxBackend
            .execute_cancellable(request, signal)
            .await
            .map(|_| ())
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while !root.join("child.pid").exists() || !root.join("parent.pid").exists() {
        assert!(Instant::now() < deadline, "fixture did not start");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let handles: Vec<_> = ["parent.pid", "child.pid"]
        .iter()
        .map(|name| {
            let pid = std::fs::read_to_string(root.join(name))
                .unwrap()
                .parse::<u32>()
                .unwrap();
            let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
            assert!(!handle.is_null());
            OwnedNativeHandle(handle)
        })
        .collect();
    scope.cancel();
    let result = tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(result, Err(SandboxError::Canceled)));
    for handle in handles {
        assert_eq!(unsafe { WaitForSingleObject(handle.0, 0) }, WAIT_OBJECT_0);
    }
    assert!(!root.join("late-effect").exists());
    std::fs::remove_dir_all(root).unwrap();
}

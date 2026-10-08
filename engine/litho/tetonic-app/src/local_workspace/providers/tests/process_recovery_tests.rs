//! Kill a real executor process: no orderly shutdown or Rust destructors run.
use super::*;
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Duration,
};

const CHILD_ROOT: &str = "TETONIC_RECOVERY_TEST_ROOT";

// Reap even when the parent assertion fails, so tests cannot leave agents behind.
struct Executor(Child);
impl Drop for Executor {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
#[ignore = "subprocess fixture, launched by killed_executor_requires_recovery_without_replaying_work"]
async fn executor_child() {
    let dir =
        PathBuf::from(std::env::var_os(CHILD_ROOT).expect("parent-owned temporary directory"));
    let root = dir.join("work");
    std::fs::create_dir(&root).unwrap();
    let (url, _, _server) = crate::tui_mvp_tests::inference_server_with_tool(
        true,
        "run_shell",
        json!({"command":"echo must-not-run>receipt.txt"}),
    )
    .await;
    tokio::task::LocalSet::new().run_until(async {
        let workspace = LocalWorkspace::open_with_workspace(dir.join("state.db"), "qwen3.5:latest".into(), url, Some(root)).await.unwrap();
        let mut request = parallel_approval_tests::agent_request("Crash operator", "ollama", None);
        request.max_seconds = 30;
        let agent = workspace.create_agent(request).await.unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        workspace.submit_for_agent(id.clone(), "Write the receipt".into(), agent.key.clone()).await.unwrap();
        let approval = parallel_approval_tests::pending(&workspace, 1).await.remove(0);
        let task = workspace.task(&id).await.unwrap();
        assert_eq!(task.state, "running");
        let ready = json!({"id":id,"agent":agent.key,"run_id":task.run_id,"sequence":task.sequence,"approval":approval});
        std::fs::write(dir.join("ready.tmp"), ready.to_string()).unwrap();
        std::fs::rename(dir.join("ready.tmp"), dir.join("ready.json")).unwrap();
        std::future::pending::<()>().await;
    }).await;
}

#[tokio::test]
async fn killed_executor_requires_recovery_without_replaying_work() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("child.log");
    let mut command = Command::new(std::env::current_exe().unwrap());
    let child_test = format!(
        "{}::executor_child",
        module_path!().split_once("::").unwrap().1
    );
    command
        .args(["--exact", &child_test, "--ignored", "--nocapture"])
        .env(CHILD_ROOT, dir.path())
        .stdin(Stdio::null())
        .stdout(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log)
                .unwrap(),
        )
        .stderr(std::fs::OpenOptions::new().append(true).open(&log).unwrap());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = Executor(command.spawn().unwrap());
    tokio::time::timeout(Duration::from_secs(30), async {
        while !dir.path().join("ready.json").exists() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "executor exited before reaching approval: {}",
                std::fs::read_to_string(&log).unwrap()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|e| panic!("{e}: {}", std::fs::read_to_string(&log).unwrap()));
    let ready: Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("ready.json")).unwrap()).unwrap();
    child.0.kill().unwrap();
    assert!(!child.0.wait().unwrap().success());
    let approval_id = ready["approval"]["approval_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let proposal_digest = ready["approval"]["proposal_digest"]
        .as_str()
        .unwrap()
        .to_owned();
    let id = ready["id"].as_str().unwrap();
    let root = dir.path().join("work");

    // An unexpired lease alone cannot prove process loss (another owner may be
    // alive). Cross the real, persisted execution deadline, well before the
    // five-minute lease expires. No projection edits or fake recovery records.
    let deadline = ready["approval"]["expires_at"].as_i64().unwrap();
    while chrono::Utc::now().timestamp() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    tokio::task::LocalSet::new()
        .run_until(async {
            let workspace = LocalWorkspace::open_with_workspace(
                dir.path().join("state.db"),
                "qwen3.5:latest".into(),
                "http://127.0.0.1:1".into(),
                Some(root.clone()),
            )
            .await
            .unwrap();
            let task = workspace.task(id).await.unwrap();
            assert_eq!(task.state, "recovery_required");
            assert_eq!(task.run_id.as_deref(), ready["run_id"].as_str());
            assert_eq!(task.sequence, ready["sequence"].as_u64().unwrap());
            assert_eq!(workspace.snapshot().await.unwrap().tasks.len(), 1);
            assert!(workspace
                .approvals()
                .await
                .unwrap()
                .pending_approvals
                .is_empty());
            assert!(
                workspace
                    .resolve_approval(
                        &approval_id,
                        ResolveApprovalRequest {
                            allow: true,
                            proposal_digest: proposal_digest.clone(),
                        }
                    )
                    .await
                    .is_err(),
                "restart cannot make an orphaned command executable"
            );
            let consume = workspace
                .keys
                .store
                .write(move |db| {
                    db.consume_shell_approval(
                        ORG,
                        TEAM,
                        &approval_id,
                        &proposal_digest,
                        chrono::Utc::now().timestamp(),
                    )
                })
                .await
                .unwrap();
            assert!(consume.is_err());
            // Repeating the submission returns its receipt, not a second executor.
            let retry = workspace
                .submit_for_agent(
                    id.into(),
                    "Write the receipt".into(),
                    ready["agent"].as_str().unwrap().into(),
                )
                .await
                .unwrap();
            assert_eq!(retry.run_id, task.run_id);
            assert_eq!(retry.state, "recovery_required");
            assert!(!root.join("receipt.txt").exists());
            assert_eq!(workspace.snapshot().await.unwrap().tasks.len(), 1);
        })
        .await;
}

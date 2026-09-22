//! Shared mock provider + daemon fixtures for integration tests.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use lokai_rpc::{channel_pair, Notifier};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::daemon::{types::EngineServices, Daemon};

#[allow(unused_imports)]
pub use lokai_app::{test_turn as turn, MockProvider, ScriptTurn, SharedStore};

type DaemonRecordingFixture = (
    Daemon,
    mpsc::UnboundedReceiver<String>,
    PathBuf,
    Arc<lokai_app::events::RecordingEventSink>,
    Arc<std::sync::Mutex<Vec<lokai_app::events::ApplicationEvent>>>,
);

pub(super) fn write_verify_pass_py(dir: &std::path::Path) {
    std::fs::write(dir.join("verify_pass.py"), "import sys\nsys.exit(0)\n").unwrap();
}

pub(super) fn write_verify_once_py(dir: &std::path::Path) {
    let flag_path = dir.join("lokai_flag.txt");
    let _ = std::fs::remove_file(&flag_path);
    let flag_str = flag_path.display().to_string().replace('\\', "/");
    std::fs::write(
        dir.join("verify_once.py"),
        format!(
            "from pathlib import Path\nimport sys\np = Path('{flag_str}')\nif p.exists():\n    sys.exit(0)\np.write_text('x')\nsys.exit(1)\n"
        ),
    )
    .unwrap();
}

pub(super) fn daemon_with_mock(
    turns: Vec<ScriptTurn>,
) -> (Daemon, mpsc::UnboundedReceiver<String>, PathBuf) {
    std::env::remove_var("LOKAI_LLM_ROUTER");
    let unique = format!(
        "lokaid-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let dir = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), "hello").unwrap();

    let (notifier, rx) = channel_pair(512);
    let mut daemon = Daemon::new(notifier.clone(), None, None);
    daemon.rpc_authenticated.store(true, Ordering::Relaxed);

    let root = dir.display().to_string();
    let event_sink = Arc::new(crate::daemon::events::DaemonEventSink::new(notifier));
    let app = lokai_app::Application::bootstrap_mock(&dir, event_sink, turns);

    daemon.services = Some(EngineServices {
        app,
        workspace_root: root,
        model: "mock".into(),
        model_hard: "mock".into(),
        models: vec!["mock".into()],
        tool_capable: true,
        tools: vec!["read_file".into(), "write_file".into()],
        fabric_pooled: false,
        num_ctx: 8192,
        ollama_base: "http://localhost:11434".into(),
        capacity: None,
    });
    (daemon, rx, dir)
}

pub(super) fn daemon_with_mock_recording(turns: Vec<ScriptTurn>) -> DaemonRecordingFixture {
    std::env::remove_var("LOKAI_LLM_ROUTER");
    let unique = format!(
        "lokaid-parity-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let dir = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(&dir).unwrap();

    let (notifier, rx) = channel_pair(512);
    let (recorder, recorded) = lokai_app::events::RecordingEventSink::new();
    let event_sink: Arc<dyn lokai_app::events::ApplicationEventSink> =
        lokai_app::events::FanoutEventSink::new(vec![
            recorder.clone(),
            Arc::new(crate::daemon::events::DaemonEventSink::new(
                notifier.clone(),
            )),
        ]);

    let mut daemon = Daemon::new(notifier, None, None);
    daemon.rpc_authenticated.store(true, Ordering::Relaxed);

    let root = dir.display().to_string();
    let app = lokai_app::Application::bootstrap_mock(&dir, event_sink, turns);

    daemon.services = Some(EngineServices {
        app,
        workspace_root: root,
        model: "mock".into(),
        model_hard: "mock".into(),
        models: vec!["mock".into()],
        tool_capable: true,
        tools: vec!["read_file".into(), "write_file".into()],
        fabric_pooled: false,
        num_ctx: 8192,
        ollama_base: "http://localhost:11434".into(),
        capacity: None,
    });
    (daemon, rx, dir, recorder, recorded)
}

pub(super) fn daemon_with_store_and_mock(
    store: SharedStore,
    workspace_root: String,
    turns: Vec<ScriptTurn>,
) -> (Daemon, mpsc::UnboundedReceiver<String>) {
    std::env::remove_var("LOKAI_LLM_ROUTER");
    let (_tx, rx) = mpsc::unbounded_channel();
    let notifier = Notifier::with_default_queue();
    let mut daemon = Daemon::new(notifier.clone(), None, None);

    let event_sink = Arc::new(crate::daemon::events::DaemonEventSink::new(notifier));
    let dir = std::path::Path::new(&workspace_root);
    let app =
        lokai_app::Application::bootstrap_mock_with_store(dir, Some(store), event_sink, turns);

    daemon.services = Some(EngineServices {
        app,
        workspace_root,
        model: "mock".into(),
        model_hard: "mock".into(),
        models: vec!["mock".into()],
        tool_capable: true,
        tools: vec!["read_file".into(), "write_file".into()],
        fabric_pooled: false,
        num_ctx: 8192,
        ollama_base: "http://localhost:11434".into(),
        capacity: None,
    });
    (daemon, rx)
}

pub(super) async fn drain_until_run_status(rx: &mut mpsc::UnboundedReceiver<String>) -> Vec<Value> {
    let mut out = Vec::new();
    let mut statuses = 0;
    while let Some(s) = rx.recv().await {
        let v: Value = serde_json::from_str(&s).unwrap();
        if v["method"] == "event/run_status" {
            statuses += 1;
            let done = v["params"]["status"] != "started";
            out.push(v);
            if statuses >= 1 && done {
                break;
            }
        } else {
            out.push(v);
        }
    }
    out
}

pub(super) fn methods_of(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .map(|e| e["method"].as_str().unwrap_or("").to_string())
        .collect()
}

pub(super) fn agent_ids_of(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| e["params"]["agent_id"].as_str().map(String::from))
        .collect()
}

pub(super) fn log_messages(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .filter(|e| e["method"] == "event/log")
        .filter_map(|e| e["params"]["message"].as_str().map(String::from))
        .collect()
}

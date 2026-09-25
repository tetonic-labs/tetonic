//! M0-4 / R27 CLI/daemon semantic-effect parity for real tool + mutation turns.

use std::sync::Arc;

use serde_json::json;
use tetonic_app::commands::{
    EndSessionCommand, InitializeCommand, RunTurnCommand, StartSessionCommand,
};
use tetonic_app::events::RecordingEventSink;
use tetonic_app::semantic_effect::{
    compare_sequences, normalize_application_events, normalize_rpc_notifications,
    run_kernel_lifecycle_scenario, SemanticEffect,
};
use tetonic_app::{Application, MockProvider, SharedStore};

use super::harness::{daemon_with_mock_recording, drain_until_run_status, turn};

fn write_mutation_turns() -> Vec<super::harness::ScriptTurn> {
    vec![
        turn(
            "",
            vec![(
                "write_file",
                json!({"path": "note.txt", "content": "parity\n"}),
            )],
        ),
        turn(
            "",
            vec![("finish", json!({"summary": "wrote note for parity"}))],
        ),
    ]
}

/// Effects that must match across CLI-shaped Application::run_turn and daemon chat_send.
fn mutation_core(seq: &[SemanticEffect]) -> Vec<SemanticEffect> {
    seq.iter()
        .filter(|e| {
            matches!(
                e,
                SemanticEffect::ToolInvocation { .. }
                    | SemanticEffect::WorkspaceMutation { .. }
                    | SemanticEffect::TerminalOutcome
            )
        })
        .cloned()
        .collect()
}

async fn run_cli_shaped_write_mutation(
    workspace: &std::path::Path,
    event_sink: Arc<dyn tetonic_app::events::ApplicationEventSink>,
) {
    let db_path = workspace.join(".lokai").join("parity.db");
    std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    let store = SharedStore::open(&db_path, 1).unwrap();
    let (app, _init, bootstrap) = Application::bootstrap(
        InitializeCommand {
            workspace_root: workspace.display().to_string(),
            rpc_token_provided: false,
            rpc_auth_disabled: true,
        },
        Some(store.clone()),
        event_sink,
        None,
        None,
    )
    .await
    .expect("bootstrap");
    let app = Arc::new(app);

    let started = app
        .sessions
        .start_session(StartSessionCommand {
            workspace_root: bootstrap.workspace_root.clone(),
            resume: None,
            model_tier: None,
            session_id: None,
            goal: Some("write note".into()),
            data_class: None,
            verify_cmd: None,
            briefing: Some(false),
            orchestration: Some("single".into()),
            critic: Some(false),
            llm_router: Some(false),
            model_fast: Some("mock".into()),
            model_hard: Some("mock".into()),
            session_max_steps: Some(8),
            ..Default::default()
        })
        .await
        .expect("start");

    // Same scripted turns as the daemon mock provider.
    let provider = Arc::new(MockProvider::new(write_mutation_turns()));
    app.bind_inference(provider, None);
    app.bind_heuristic_tokenizer();
    let join = app.arm_turn_join(&started.session_id);
    app.submit_chat_turn(RunTurnCommand {
        session_id: started.session_id.clone(),
        user_input: "write note.txt".into(),
        verify_cmd: None,
        llm_router: Some(false),
    })
    .expect("submit_chat_turn");
    join.await.expect("turn join");

    app.sessions
        .end_session(EndSessionCommand {
            session_id: started.session_id.clone(),
            workspace_root: bootstrap.workspace_root.clone(),
            status: Some("ok".into()),
            error: None,
        })
        .expect("end");
}

#[test]
fn cli_daemon_kernel_semantic_effect_parity() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let (daemon_kernel_effects, daemon_wire_effects, workspace_root) =
        rt.block_on(local.run_until(async {
            let turns = vec![turn("", vec![("finish", json!({"summary": "done"}))])];
            let (mut daemon, mut rx, dir, _recorder, daemon_events) =
                daemon_with_mock_recording(turns);

            let workspace_root = dir.display().to_string();
            let sid = daemon
                .session_start(json!({ "briefing": false }))
                .await
                .unwrap()["session_id"]
                .as_str()
                .unwrap()
                .to_string();

            daemon
                .chat_send(json!({ "session_id": sid, "text": "hello" }))
                .unwrap();
            let rpc_events = drain_until_run_status(&mut rx).await;

            daemon
                .session_end(json!({ "session_id": sid, "status": "ok" }))
                .unwrap();

            let daemon_kernel_effects =
                normalize_application_events(&daemon_events.lock().unwrap());
            let daemon_wire_effects = normalize_rpc_notifications(&rpc_events);

            (daemon_kernel_effects, daemon_wire_effects, workspace_root)
        }));

    rt.block_on(local.run_until(async {
        let (cli_recorder, cli_events) = RecordingEventSink::new();
        let cli_app = Application::bootstrap_mock(
            std::path::Path::new(&workspace_root),
            cli_recorder,
            vec![],
        );
        run_kernel_lifecycle_scenario(&cli_app, &workspace_root, "hello")
            .await
            .expect("cli kernel lifecycle");

        let cli_effects = normalize_application_events(&cli_events.lock().unwrap());

        compare_sequences(&cli_effects, &daemon_kernel_effects)
            .expect("CLI and daemon must emit identical app-layer semantic effects");

        assert!(
            daemon_wire_effects
                .iter()
                .any(|e| matches!(e, SemanticEffect::SessionInitialization)),
            "daemon wire events missing session init: {daemon_wire_effects:?}"
        );
        assert!(
            daemon_wire_effects
                .iter()
                .any(|e| matches!(e, SemanticEffect::ModelRequest)),
            "daemon wire events missing model request: {daemon_wire_effects:?}"
        );
        assert!(
            daemon_wire_effects
                .iter()
                .any(|e| matches!(e, SemanticEffect::TerminalOutcome)),
            "daemon wire events missing terminal outcome: {daemon_wire_effects:?}"
        );
    }));
}

#[test]
fn cli_daemon_write_file_mutation_parity() {
    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let daemon_core = rt.block_on(local.run_until(async {
        let (mut daemon, mut rx, dir, _recorder, daemon_events) =
            daemon_with_mock_recording(write_mutation_turns());
        let sid = daemon
            .session_start(json!({ "briefing": false }))
            .await
            .unwrap()["session_id"]
            .as_str()
            .unwrap()
            .to_string();
        daemon
            .chat_send(json!({ "session_id": sid, "text": "write note.txt" }))
            .unwrap();
        let _rpc = drain_until_run_status(&mut rx).await;
        daemon
            .session_end(json!({ "session_id": sid, "status": "ok" }))
            .unwrap();

        let note = dir.join("note.txt");
        assert!(
            note.is_file(),
            "daemon path must mutate workspace via execute_turn"
        );
        assert_eq!(std::fs::read_to_string(&note).unwrap(), "parity\n");

        let effects = normalize_application_events(&daemon_events.lock().unwrap());
        mutation_core(&effects)
    }));

    let cli_core = rt.block_on(local.run_until(async {
        let unique = format!(
            "cli-parity-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let dir = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&dir).unwrap();
        let (recorder, events) = RecordingEventSink::new();
        run_cli_shaped_write_mutation(&dir, recorder).await;

        let note = dir.join("note.txt");
        assert!(
            note.is_file(),
            "CLI-shaped Application::run_turn must mutate workspace"
        );
        assert_eq!(std::fs::read_to_string(&note).unwrap(), "parity\n");

        let effects = normalize_application_events(&events.lock().unwrap());
        mutation_core(&effects)
    }));

    assert!(
        daemon_core
            .iter()
            .any(|e| matches!(e, SemanticEffect::ToolInvocation { tool } if tool == "write_file")),
        "daemon missing write_file tool effect: {daemon_core:?}"
    );
    assert!(
        daemon_core.iter().any(|e| matches!(
            e,
            SemanticEffect::WorkspaceMutation { action } if action == "write_file"
        )),
        "daemon missing write_file mutation effect: {daemon_core:?}"
    );
    compare_sequences(&cli_core, &daemon_core)
        .expect("CLI and daemon write+mutation cores must match");
}

#[test]
fn extra_mutation_on_one_side_fails_compare() {
    let a = vec![
        SemanticEffect::ToolInvocation {
            tool: "write_file".into(),
        },
        SemanticEffect::WorkspaceMutation {
            action: "write_file".into(),
        },
        SemanticEffect::TerminalOutcome,
    ];
    let mut b = a.clone();
    b.insert(
        2,
        SemanticEffect::ToolInvocation {
            tool: "edit_file".into(),
        },
    );
    assert!(
        compare_sequences(&a, &b).is_err(),
        "extra tool/mutation must fail parity compare"
    );
}

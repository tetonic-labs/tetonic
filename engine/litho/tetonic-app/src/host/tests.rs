use super::*;
use std::time::Duration;

#[test]
fn workspace_execution_defaults_and_partial_overrides_are_bounded() {
    let default = HostConfiguration::from_json(b"{}")
        .unwrap()
        .workspace_execution;
    assert_eq!(
        (default.worker_steps(false), default.worker_steps(true)),
        (4, 8)
    );
    assert_eq!((default.max_seconds, default.max_tokens), (120, 12_288));
    assert_eq!(
        (
            default.coordination_max_steps,
            default.coordination_tokens()
        ),
        (16, 4096)
    );
    let config = HostConfiguration::from_json(
        br#"{"workspace_execution":{"max_steps":32,"max_tokens":1024}}"#,
    )
    .unwrap();
    assert_eq!(config.workspace_execution.worker_steps(false), 32);
    assert_eq!(config.workspace_execution.worker_steps(true), 32);
    assert_eq!(config.workspace_execution.coordination_tokens(), 1024);
    for (field, values) in [
        ("max_steps", [0, 513]),
        ("max_seconds", [9, 86_401]),
        ("max_tokens", [255, 1_000_001]),
        ("coordination_max_steps", [0, 513]),
        ("coordination_max_tokens", [255, 1_000_001]),
    ] {
        for value in values {
            let config = serde_json::json!({"workspace_execution":{field:value}});
            assert!(
                HostConfiguration::from_json(&serde_json::to_vec(&config).unwrap()).is_err(),
                "{config}"
            );
        }
    }
    assert!(HostConfiguration::from_json(
        br#"{"workspace_execution":{"allowed_tools":["run_shell"]}}"#
    )
    .is_err());
}

#[test]
fn configuration_rejects_unsupported_options_and_authority_fields() {
    for value in [
        serde_json::json!({"storage":{"read_connections":0}}),
        serde_json::json!({"storage":{"read_connections":17}}),
        serde_json::json!({"storage":{"storage_mode":"distributed_db"}}),
        serde_json::json!({"storage":{"artifact_directory":""}}),
        serde_json::json!({"logging":{"level":"anything"}}),
        serde_json::json!({"logging":{"directory":""}}),
        serde_json::json!({"telemetry":{"sink":"otlp"}}),
        serde_json::json!({"telemetry":{"sample_rate":1.1}}),
        serde_json::json!({"telemetry":{"byte_budget":0}}),
        serde_json::json!({"allowed_tools":["run_shell"]}),
        serde_json::json!({"principal":"admin"}),
    ] {
        assert!(
            HostConfiguration::from_json(&serde_json::to_vec(&value).unwrap()).is_err(),
            "{value}"
        );
    }
}

#[test]
fn file_relative_paths_and_explicit_absolute_paths_are_preserved() {
    let directory = tempfile::tempdir().unwrap();
    let absolute = directory.path().join("absolute-artifacts");
    let mut config = HostConfiguration::from_json(
        &serde_json::to_vec(&serde_json::json!({
            "storage":{"artifact_directory":absolute},
            "logging":{"directory":"logs","level":"info","stderr":false},
            "telemetry":{"sample_rate":0.5,"byte_budget":4096}
        }))
        .unwrap(),
    )
    .unwrap();
    config.resolve_relative_paths(directory.path());
    assert_eq!(config.storage.artifact_directory.as_ref(), Some(&absolute));
    assert_eq!(
        config.logging.directory,
        Some(directory.path().join("logs"))
    );
    assert_eq!(config.telemetry.byte_budget, 4096);
}

#[tokio::test]
async fn invalid_configuration_does_not_create_a_database() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("must-not-exist.db");
    let mut config = HostConfiguration::default();
    config.storage.read_connections = 0;
    assert!(ApplicationHost::open(
        database.clone(),
        "test".into(),
        None,
        "http://127.0.0.1:9".into(),
        config
    )
    .await
    .is_err());
    assert!(!database.exists());
}

#[test]
fn database_paths_preserve_workspace_fallback_without_granting_a_folder() {
    // A bare filename has an empty parent, which cannot be canonicalized.
    assert_eq!(
        runtime_workspace(Path::new("workspace.db"), None),
        std::env::temp_dir()
    );
    assert_eq!(
        runtime_workspace(Path::new("data/workspace.db"), None),
        PathBuf::from("data")
    );
    assert_eq!(
        runtime_workspace(Path::new("data/workspace.db"), Some("project".into())),
        PathBuf::from("project")
    );
}

#[tokio::test]
async fn control_and_execution_share_one_writer_and_configured_artifacts() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("control.db");
    let artifacts = directory.path().join("payloads");
    let mut config = HostConfiguration::default();
    config.storage.artifact_directory = Some(artifacts.clone());
    let host = ApplicationHost::open(
        database,
        "test".into(),
        None,
        "http://127.0.0.1:9".into(),
        config,
    )
    .await
    .unwrap();
    assert!(artifacts.is_dir());
    assert!(!directory.path().join(".lokai/artifacts").exists());
    assert!(host.app.compute_broker().is_some());
    assert!(Arc::ptr_eq(
        &host.app.host.policy,
        host.app.host.runtime.policy()
    ));
    let control = host.control.store().clone();
    let execution = host.app.store().unwrap().clone();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    // Hold the writer without a SQL lock: a mistakenly reopened writer would
    // accept the execution-side command immediately, bypassing this queue.
    let blocked = tokio::spawn(async move {
        control
            .write(move |_| {
                let _ = entered_tx.send(());
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            })
            .await
            .unwrap();
    });
    entered_rx.await.unwrap();
    let mut queued = tokio::spawn(async move { execution.write(|_| 42).await.unwrap() });
    let early = tokio::time::timeout(Duration::from_millis(100), &mut queued).await;
    release_tx.send(()).unwrap();
    blocked.await.unwrap();
    assert!(early.is_err(), "execution used a second store writer");
    assert_eq!(queued.await.unwrap(), 42);
}

#[test]
fn diagnostics_filter_flush_and_sanitize_all_file_output() {
    let directory = tempfile::tempdir().unwrap();
    let logging = tetonic_telemetry::host::LoggingConfig {
        level: tetonic_telemetry::host::LogLevel::Warn,
        stderr: false,
        directory: Some(directory.path().to_path_buf()),
    };
    let (subscriber, guard) = tetonic_telemetry::host::host_subscriber(&logging).unwrap();
    tracing::subscriber::with_default(subscriber, || {
        tracing::info!("INFO_CANARY_MUST_NOT_BE_WRITTEN");
        tracing::warn!(
            prompt = "PRIVATE_PROMPT_CANARY",
            process_output = "PRIVATE_PROCESS_CANARY",
            "password=PRIVATE_SECRET_CANARY"
        );
    });
    drop(guard);
    let files: Vec<_> = std::fs::read_dir(directory.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1);
    let text = std::fs::read_to_string(&files[0]).unwrap();
    assert!(text.contains("DROPPED_PAYLOAD"));
    assert!(text.contains("REDACTED_SECRET"));
    assert!(!text.contains("CANARY"));
    for line in text.lines() {
        serde_json::from_str::<serde_json::Value>(line).unwrap();
    }
}

#[test]
fn configured_log_destination_failure_is_reported() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("not-a-directory");
    std::fs::write(&file, "occupied").unwrap();
    let logging = tetonic_telemetry::host::LoggingConfig {
        directory: Some(file),
        ..Default::default()
    };
    assert!(tetonic_telemetry::host::host_subscriber(&logging).is_err());
}

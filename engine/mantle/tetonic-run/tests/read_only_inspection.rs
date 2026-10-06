use tetonic_domain::{CreateRun, RunCommand, RunId, RunState, TaskId, TaskInputBinding, TaskState};
use tetonic_memory::SharedStore;
use tetonic_run::{command_envelope, DurableRunReader, DurableRunSupervisor, RunSupervisor};

#[tokio::test]
async fn observing_an_active_finalization_does_not_run_startup_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let store = SharedStore::open(directory.path().join("runs.db"), 1).unwrap();
    let writer = DurableRunSupervisor::new(Some(store.clone()));
    let run = RunId::new("live-run");
    let created = writer
        .handle(RunCommand::CreateRun(CreateRun {
            envelope: command_envelope("create", None, "test"),
            session_id: None,
            run_id: run.clone(),
            root_task_id: TaskId::new("root"),
            root_binding: TaskInputBinding::default(),
            speculation: None,
            job_spec: None,
        }))
        .await
        .unwrap();
    let mut active = created.snapshot;
    active.state = RunState::Active;
    active.tasks.values_mut().next().unwrap().state = TaskState::Running;
    // This in-progress projection would require recovery after a real writer restart.
    store
        .write(move |db| db.persist_run_projection(&active))
        .await
        .unwrap()
        .unwrap();
    for _ in 0..3 {
        let observer = DurableRunReader::new(store.clone());
        assert_eq!(
            observer.snapshot(run.clone()).await.unwrap().state,
            RunState::Active
        );
        observer
            .resume_from_sequence(run.clone(), 0, 10)
            .await
            .unwrap()
            .unwrap();
    }
    assert_eq!(
        writer.snapshot(run.clone()).await.unwrap().state,
        RunState::Active
    );
    // The actual execution supervisor retains its fail-closed startup behavior.
    let restarted = DurableRunSupervisor::new(Some(store));
    assert_eq!(
        restarted.snapshot(run).await.unwrap().state,
        RunState::RecoveryRequired
    );
}

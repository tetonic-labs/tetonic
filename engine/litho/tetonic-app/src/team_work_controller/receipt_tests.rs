use super::*;
use crate::local_workspace::{LocalWorkspace, ORG, OWNER, TEAM};
use crate::work::{plan_execution::tests as plan_fixture, StartPlan};
use tetonic_memory::HuddleDispatchReceipt;

#[test]
fn aborted_groups_do_not_claim_to_have_returned_collected_contributions() {
    assert!(!response_contains_contributions(&ToolOutcome::fail(
        "A later progress read failed",
        "unavailable"
    )));
    assert!(!response_contains_contributions(&ToolOutcome::ok(
        "Partial", "{}"
    )));
    let mut partial = ToolOutcome::fail("Some assignments need attention", "blocked");
    partial.content = serde_json::json!({"results":[{"assignment_key":"check","result":{"state":"completed"}}],"outstanding_assignments":["compare"]}).to_string();
    assert!(response_contains_contributions(&partial));
}

struct NoDispatch;
#[async_trait::async_trait(?Send)]
impl TeamWorkHost for NoDispatch {
    async fn admit(
        &self,
        _: &HuddleExecution,
        _: &DelegationParent,
        _: &str,
    ) -> Result<(), AppError> {
        panic!("receipt replay must not dispatch a worker")
    }
    async fn contribution(&self, _: &HuddleExecution, _: &str) -> Result<ToolOutcome, AppError> {
        panic!("receipt replay must return the original response")
    }
}

struct WaitingTeam {
    directory: tempfile::TempDir,
    workspace: Rc<LocalWorkspace>,
    source: String,
    receipt: HuddleExecution,
    records: Vec<HuddleDispatchReceipt>,
    calls: Arc<Mutex<Vec<serde_json::Value>>>,
    server: tokio::task::JoinHandle<()>,
}

impl WaitingTeam {
    async fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let (url, calls, server) = plan_fixture::scripted_server(2).await;
        let workspace = Rc::new(
            LocalWorkspace::open(
                directory.path().join("team.db"),
                "qwen3.5:latest".into(),
                url,
            )
            .await
            .unwrap(),
        );
        let source = plan_fixture::seed_options(&workspace, true, false).await;
        let started = workspace
            .start_plan(
                &source,
                StartPlan {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    revision: 1,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let records = tokio::time::timeout(std::time::Duration::from_secs(15), async {
            loop {
                let records = workspace
                    .services
                    .local
                    .store()
                    .read({
                        let source = source.clone();
                        move |db| db.huddle_dispatch_receipts(OWNER, ORG, TEAM, &source)
                    })
                    .await
                    .unwrap()
                    .unwrap();
                // compare waits for a human; check completed independently; the
                // coordinator is now blocked inside its second compare request.
                if records.len() == 3
                    && records[1]
                        .result
                        .as_ref()
                        .is_some_and(|r| r.delivered_keys == ["check"])
                    && records[2].result.is_none()
                {
                    break records;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        Self {
            directory,
            workspace,
            source,
            receipt: started.receipt,
            records,
            calls,
            server,
        }
    }

    fn controller(&self, host: Rc<dyn TeamWorkHost>) -> TeamWorkController {
        let parent = self
            .workspace
            .services
            .host
            .app
            .run_manager
            .managed()
            .delegation_parent(&tetonic_domain::AttemptId::new(&self.records[1].attempt_id))
            .unwrap();
        TeamWorkController {
            reader: ProgressReader {
                store: self.workspace.services.local.store().clone(),
                actor: OWNER.into(),
                org: ORG.into(),
                team: TEAM.into(),
            },
            host,
            receipt: self.receipt.clone(),
            parent,
            remaining: Arc::new(Mutex::new(
                self.receipt
                    .assignments
                    .iter()
                    .map(|a| a.assignment_key.clone())
                    .collect(),
            )),
        }
    }

    fn binding(&self, record: &HuddleDispatchReceipt) -> DispatchBinding {
        DispatchBinding {
            reader: self.controller(Rc::new(NoDispatch)).reader,
            source: self.source.clone(),
            run: record.run_id.clone(),
            attempt: record.attempt_id.clone(),
            lease: record.lease.clone(),
            call: record.call_id.clone(),
            keys: record.keys.clone(),
            grouped: record.grouped,
        }
    }

    async fn stop(self) {
        self.workspace
            .cancel(&self.receipt.root_work_id)
            .await
            .unwrap();
        self.server.abort();
    }
}

fn call(record: &HuddleDispatchReceipt) -> DispatchCall {
    let (reply, _) = tokio::sync::oneshot::channel();
    DispatchCall {
        call_id: record.call_id.clone(),
        keys: record.keys.clone(),
        grouped: record.grouped,
        attempt: record.attempt_id.clone(),
        reply,
    }
}

#[tokio::test]
async fn reopened_dispatch_response_replays_without_work_or_model_calls() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let team = WaitingTeam::start().await;
            let db = tetonic_memory::Store::open(team.directory.path().join("team.db")).unwrap();
            let records = db
                .huddle_dispatch_receipts(OWNER, ORG, TEAM, &team.source)
                .unwrap();
            assert_eq!(records, team.records);
            assert!(db
                .huddle_dispatch_receipts("outsider", ORG, TEAM, &team.source)
                .is_err());
            drop(db);
            let controller = team.controller(Rc::new(NoDispatch));
            let saved = records[1].result.as_ref().unwrap();
            let count = team.calls.lock().unwrap().len();
            for _ in 0..2 {
                let replay = controller.dispatch_recorded(&call(&records[1])).await;
                assert_eq!(
                    (
                        replay.ok,
                        &replay.content,
                        &replay.summary,
                        &replay.error_kind
                    ),
                    (saved.ok, &saved.content, &saved.summary, &saved.error_kind)
                );
                assert_eq!(
                    *controller.remaining.lock().unwrap(),
                    HashSet::from(["compare".into(), "wrap".into()])
                );
            }
            assert_eq!(team.calls.lock().unwrap().len(), count);
            let mut changed = call(&records[1]);
            changed.keys = vec!["compare".into()];
            assert!(!controller.dispatch_recorded(&changed).await.ok);
            assert_eq!(team.calls.lock().unwrap().len(), count);
            team.stop().await;
        })
        .await;
}

#[tokio::test]
async fn dispatch_receipts_reject_changed_requests_stale_fences_and_false_delivery() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let team = WaitingTeam::start().await;
            let db = tetonic_memory::Store::open(team.directory.path().join("team.db")).unwrap();
            let binding = team.binding(&team.records[1]);
            for variant in 0..7 {
                let mut changed = binding.clone();
                match variant {
                    0 => changed.keys = vec!["compare".into()],
                    1 => changed.grouped = !changed.grouped,
                    2 => changed.attempt = "another-attempt".into(),
                    3 => changed.run = "another-run".into(),
                    4 => changed.lease.lease_epoch += 1,
                    5 => {
                        changed.lease.holder =
                            tetonic_domain::ExecutionTargetId::worker("another-host")
                    }
                    _ => changed.reader.actor = "outsider".into(),
                }
                assert!(
                    db.accept_huddle_dispatch(changed.command()).is_err(),
                    "variant {variant}"
                );
            }
            let mut expired = binding.command();
            expired.now += 86400;
            assert!(db.accept_huddle_dispatch(expired).is_err());
            let mut altered = team.records[1].result.clone().unwrap();
            altered.content = "replacement result".into();
            assert!(db
                .complete_huddle_dispatch(binding.command(), altered)
                .is_err());
            let mut pending = binding.clone();
            pending.call = "new-pending-call".into();
            db.accept_huddle_dispatch(pending.command()).unwrap();
            let mut false_delivery = team.records[1].result.clone().unwrap();
            false_delivery.delivered_keys = vec!["compare".into()];
            assert!(db
                .complete_huddle_dispatch(pending.command(), false_delivery)
                .is_err());
            // A newer journal fence must prevent the old owner from writing even
            // when a matching receipt and current sequence remain available.
            let original = db.load_run_snapshot(&binding.run).unwrap().unwrap();
            let mut fenced = original.clone();
            fenced
                .attempts
                .get_mut(&tetonic_domain::AttemptId::new(&binding.attempt))
                .unwrap()
                .lease
                .as_mut()
                .unwrap()
                .lease_epoch += 1;
            db.persist_run_projection(&fenced).unwrap();
            assert!(db.accept_huddle_dispatch(binding.command()).is_err());
            assert!(db
                .complete_huddle_dispatch(
                    binding.command(),
                    team.records[1].result.clone().unwrap()
                )
                .is_err());
            db.persist_run_projection(&original).unwrap();
            drop(db);
            let controller = team.controller(Rc::new(NoDispatch));
            let replay = call(&team.records[1]);
            team.workspace
                .cancel(&team.receipt.root_work_id)
                .await
                .unwrap();
            assert!(!controller.dispatch_recorded(&replay).await.ok);
            team.server.abort();
        })
        .await;
}

#[tokio::test]
async fn failed_response_persistence_cannot_clear_the_coordinator_finish_guard() {
    tokio::task::LocalSet::new().run_until(async {
        let team = WaitingTeam::start().await;
        let controller = team.controller(team.workspace.clone());
        let conn = rusqlite::Connection::open(team.directory.path().join("team.db")).unwrap();
        conn.execute_batch("CREATE TRIGGER fail_dispatch_response BEFORE UPDATE ON huddle_dispatch_receipts WHEN json_type(NEW.payload,'$.result') = 'object' BEGIN SELECT RAISE(ABORT,'injected response write failure'); END;").unwrap();
        let mut request = call(&team.records[1]);
        request.call_id = "response-write-fault".into();
        let before = controller.remaining.lock().unwrap().clone();
        let count = team.calls.lock().unwrap().len();
        assert!(!controller.dispatch_recorded(&request).await.ok);
        assert_eq!(*controller.remaining.lock().unwrap(),before);
        assert_eq!(team.calls.lock().unwrap().len(),count,"completed worker must not run again");
        conn.execute_batch("DROP TRIGGER fail_dispatch_response").unwrap();
        drop(conn);
        let response = controller.dispatch_recorded(&request).await;
        assert!(response.ok);
        assert_eq!(*controller.remaining.lock().unwrap(),HashSet::from(["compare".into(),"wrap".into()]));
        assert_eq!(team.calls.lock().unwrap().len(),count);
        team.stop().await;
    }).await;
}

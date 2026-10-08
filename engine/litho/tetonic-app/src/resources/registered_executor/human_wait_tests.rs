use super::{fixture::Fixture, park, server, *};

#[tokio::test]
async fn stop_after_answer_invalidates_already_built_resume_authority() {
    let f = Fixture::new().await;
    let (url, calls, server) = server::inference().await;
    let app = f.app(&url).await;
    let (receipt, attempt) = tokio::task::LocalSet::new().run_until(park(&f, &app)).await;
    drop(app);
    let app = f.app(&url).await;
    let rebuilt = f.harness(&app, Some(receipt.clone())).await;
    let q = question(&f).await;
    f.store()
        .write(move |db| {
            db.answer_work_human(tetonic_memory::AnswerWorkHuman {
                actor: "admin",
                org: "org",
                team: "team",
                work: "work",
                id: &q.id,
                request: "answer",
                answer: "Beginners",
                now: chrono::Utc::now().timestamp() as u64,
            })
            .unwrap();
            db.request_control_stop("admin", "org", "team", "team", "pause", "Stop before wake")
                .unwrap();
            db.clear_control_stop("admin", "org", "team", "team")
                .unwrap();
        })
        .await
        .unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            assert!(app
                .run_manager
                .execute_prepared_registered_job(rebuilt.prepared, rebuilt.agent, rebuilt.restore)
                .await
                .is_err());
        })
        .await;
    let run = app
        .run_manager
        .managed()
        .inspect_run(&receipt.run_id)
        .await
        .unwrap();
    assert_eq!(run.attempts[&attempt].state, AttemptState::Suspended);
    assert_eq!(calls.lock().unwrap().len(), 2);
    server.abort();
}

#[tokio::test]
async fn schema_65_questions_remain_live_only_after_upgrade() {
    let f = Fixture::new().await;
    let (url, _, server) = server::inference().await;
    let app = f.app(&url).await;
    let _ = tokio::task::LocalSet::new().run_until(park(&f, &app)).await;
    drop(app);
    let q = question(&f).await;
    let mut legacy = serde_json::to_value(&q).unwrap();
    legacy.as_object_mut().unwrap().remove("saved_wait");
    let path = f.dir.path().join("control.db");
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute(
            "UPDATE work_human_questions SET payload=?1",
            [legacy.to_string()],
        )
        .unwrap();
        db.execute("DELETE FROM schema_versions WHERE version>=66", [])
            .unwrap();
    }
    let db = tetonic_memory::Store::open(&path).unwrap();
    let old = db
        .work_human_questions("admin", "org", "team", "work")
        .unwrap()
        .remove(0);
    assert!(old.saved_wait.is_none());
    assert_eq!(old.content, q.content);
    assert_eq!(old.deadline, q.deadline);
    assert_eq!(serde_json::to_value(old).unwrap(), legacy);
    assert!(
        db.answer_work_human(tetonic_memory::AnswerWorkHuman {
            actor: "admin",
            org: "org",
            team: "team",
            work: "work",
            id: &q.id,
            request: "answer",
            answer: "Beginners",
            now: chrono::Utc::now().timestamp() as u64,
        })
        .is_err(),
        "migration does not turn a legacy question into durable authority"
    );
    assert!(tetonic_memory::pre_migrate_backup_directory(&path).is_dir());
    server.abort();
}

#[tokio::test]
async fn crash_before_question_insert_reuses_checkpoint_call_and_original_deadline() {
    let f = Fixture::new().await;
    let (url, calls, server) = server::inference().await;
    let app = f.app(&url).await;
    let (receipt, _) = tokio::task::LocalSet::new().run_until(park(&f, &app)).await;
    let q = question(&f).await;
    let fabricated = tetonic_core::SpawnRequest {
        tool_name: "ask_human".into(),
        attempt_id: Some(q.attempt_id.clone()),
        call_id: q.saved_wait.as_ref().unwrap().call_id.clone(),
        arguments: serde_json::json!({"question":"Replace the saved question", "why":"No"}),
        ..Default::default()
    };
    assert!(app
        .run_manager
        .managed()
        .verify_human_handoff(&fabricated)
        .await
        .is_err());
    drop(app);
    // Fault injection models loss after the checkpoint commit but before insertion.
    rusqlite::Connection::open(f.dir.path().join("control.db"))
        .unwrap()
        .execute("DELETE FROM work_human_questions", [])
        .unwrap();
    let app = f.app(&url).await;
    let rebuilt = f.harness(&app, Some(receipt)).await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let ManagedSubmission::Started { completion, .. } = app
                .run_manager
                .execute_prepared_registered_job(rebuilt.prepared, rebuilt.agent, rebuilt.restore)
                .await
                .unwrap()
            else {
                panic!("restore")
            };
            let saved = tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    let rows = f
                        .store()
                        .read(|db| db.work_human_questions("admin", "org", "team", "work"))
                        .await
                        .unwrap()
                        .unwrap();
                    if let Some(row) = rows.into_iter().next() {
                        break row;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            assert_eq!(saved.id, q.id);
            assert_eq!(saved.deadline, q.deadline);
            assert_eq!(saved.saved_wait, q.saved_wait);
            f.store()
                .write(move |db| {
                    db.answer_work_human(tetonic_memory::AnswerWorkHuman {
                        actor: "admin",
                        org: "org",
                        team: "team",
                        work: "work",
                        id: &saved.id,
                        request: "answer",
                        answer: "Beginners",
                        now: chrono::Utc::now().timestamp() as u64,
                    })
                })
                .await
                .unwrap()
                .unwrap();
            let result = tokio::time::timeout(Duration::from_secs(10), completion)
                .await
                .unwrap()
                .unwrap();
            assert!(matches!(result.outcome, CandidateOutcome::Completed { .. }));
        })
        .await;
    assert_eq!(calls.lock().unwrap().len(), 3);
    server.abort();
}

async fn question(f: &Fixture) -> tetonic_memory::WorkHumanQuestion {
    f.store()
        .read(|db| db.work_human_questions("admin", "org", "team", "work"))
        .await
        .unwrap()
        .unwrap()
        .remove(0)
}

#[tokio::test]
async fn saved_questions_reject_expired_replaced_revoked_and_stopped_work() {
    for fault in [
        "deadline",
        "outsider",
        "wrong_team",
        "checkpoint",
        "task_version",
        "grant",
        "stop",
        "cleared_stop",
    ] {
        let f = Fixture::new().await;
        let (url, calls, server) = server::inference().await;
        let app = f.app(&url).await;
        let (receipt, attempt) = tokio::task::LocalSet::new().run_until(park(&f, &app)).await;
        drop(app);
        let q = question(&f).await;
        let now = chrono::Utc::now().timestamp() as u64;
        let id = q.id.clone();
        let run_id = receipt.run_id.0.clone();
        f.store()
            .write(move |db| {
                match fault {
                    "checkpoint" | "task_version" => {
                        let mut run = db.load_run_snapshot(&run_id).unwrap().unwrap();
                        if fault == "checkpoint" {
                            run.attempts
                                .get_mut(&attempt)
                                .unwrap()
                                .suspension
                                .as_mut()
                                .unwrap()
                                .checkpoint
                                .digest = "sha256:changed".into();
                        } else {
                            run.tasks
                                .get_mut(&receipt.task_id)
                                .unwrap()
                                .binding
                                .task_definition_version += 1;
                        }
                        db.persist_run_projection(&run).unwrap();
                    }
                    "grant" => db
                        .revoke_execution_grant("admin", "org", "grant", now as i64)
                        .unwrap(),
                    "stop" | "cleared_stop" => {
                        db.request_control_stop(
                            "admin",
                            "org",
                            "team",
                            "team",
                            "pause",
                            "Test stop",
                        )
                        .unwrap();
                        if fault == "cleared_stop" {
                            db.clear_control_stop("admin", "org", "team", "team")
                                .unwrap();
                        }
                    }
                    _ => {}
                }
                let at = if fault == "deadline" { q.deadline } else { now };
                assert!(
                    db.answer_work_human(tetonic_memory::AnswerWorkHuman {
                        actor: if fault == "outsider" {
                            "other"
                        } else {
                            "admin"
                        },
                        org: "org",
                        team: if fault == "wrong_team" {
                            "other"
                        } else {
                            "team"
                        },
                        work: "work",
                        id: &id,
                        request: "answer",
                        answer: "Beginners",
                        now: at,
                    })
                    .is_err(),
                    "{fault}"
                );
                assert!(db
                    .work_human_questions("admin", "org", "team", "work")
                    .unwrap()[0]
                    .answer
                    .is_none());
                if !matches!(fault, "outsider" | "wrong_team") {
                    assert!(
                        db.pending_work_human_question("admin", "org", "team", "work", &id, at)
                            .is_err(),
                        "{fault}"
                    );
                }
            })
            .await
            .unwrap();
        assert_eq!(calls.lock().unwrap().len(), 2);
        server.abort();
    }
}

#[tokio::test]
async fn saved_answer_is_idempotent_but_never_authority_after_revocation() {
    let f = Fixture::new().await;
    let (url, _, server) = server::inference().await;
    let app = f.app(&url).await;
    let (_, _) = tokio::task::LocalSet::new().run_until(park(&f, &app)).await;
    drop(app);
    let q = question(&f).await;
    f.store()
        .write(move |db| {
            // The original execution allowance is 30s. Human time does not consume it.
            let now = chrono::Utc::now().timestamp() as u64 + 60;
            let command = || tetonic_memory::AnswerWorkHuman {
                actor: "admin",
                org: "org",
                team: "team",
                work: "work",
                id: &q.id,
                request: "same-response",
                answer: "Beginners",
                now,
            };
            let first = db.answer_work_human(command()).unwrap();
            let retry = db.answer_work_human(command()).unwrap();
            assert_eq!(first.response_id, retry.response_id);
            let mut conflict = command();
            conflict.answer = "Experts";
            assert!(db.answer_work_human(conflict).is_err());
            // Once answered on time, a response deadline is not a new execution deadline.
            assert!(db
                .pending_work_human_question("admin", "org", "team", "work", &q.id, q.deadline + 1)
                .is_ok());
            assert!(db
                .pending_work_human_question("admin", "org", "team", "work", &q.id, now + 4000)
                .is_err());
            db.revoke_execution_grant("admin", "org", "grant", now as i64)
                .unwrap();
            assert!(
                db.answer_work_human(command()).is_ok(),
                "read the same receipt without any effect"
            );
            assert!(db
                .pending_work_human_question("admin", "org", "team", "work", &q.id, now)
                .is_err());
        })
        .await
        .unwrap();
    server.abort();
}

#[tokio::test]
async fn rejected_saved_handoff_never_reacquires_execution_or_calls_model() {
    let f = Fixture::new().await;
    let (url, calls, server) = server::inference().await;
    let app = f.app(&url).await;
    let (receipt, attempt) = tokio::task::LocalSet::new().run_until(park(&f, &app)).await;
    drop(app);
    let mut q = question(&f).await;
    q.content.question = "Corrupted question".into();
    rusqlite::Connection::open(f.dir.path().join("control.db"))
        .unwrap()
        .execute(
            "UPDATE work_human_questions SET payload=?1 WHERE question_id=?2",
            rusqlite::params![serde_json::to_string(&q).unwrap(), q.id],
        )
        .unwrap();
    let app = f.app(&url).await;
    let rebuilt = f.harness(&app, Some(receipt.clone())).await;
    tokio::task::LocalSet::new()
        .run_until(async {
            let ManagedSubmission::Started { completion, .. } = app
                .run_manager
                .execute_prepared_registered_job(rebuilt.prepared, rebuilt.agent, rebuilt.restore)
                .await
                .unwrap()
            else {
                panic!("restore")
            };
            let result = tokio::time::timeout(Duration::from_secs(10), completion)
                .await
                .unwrap()
                .unwrap();
            assert!(matches!(result.outcome, CandidateOutcome::Failed { .. }));
        })
        .await;
    let run = app
        .run_manager
        .managed()
        .inspect_run(&receipt.run_id)
        .await
        .unwrap();
    assert_eq!(run.attempts[&attempt].state, AttemptState::Suspended);
    assert_eq!(calls.lock().unwrap().len(), 2);
    assert_eq!(
        f.store()
            .read(|db| db.team_work_usage("admin", "org", "team"))
            .await
            .unwrap()
            .unwrap()[0]
            .held_tokens,
        35
    );
    server.abort();
}

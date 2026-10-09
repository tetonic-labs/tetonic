use super::*;

#[tokio::test]
async fn saved_boundary_restores_only_received_contributions_and_is_scope_bound() {
    tokio::task::LocalSet::new().run_until(async {
        let team = WaitingTeam::start().await;
        let controller = team.controller(Rc::new(NoDispatch));
        let count = team.calls.lock().unwrap().len();
        let request = call(&team, &team.records[2]).await;
        let received = request.checkpoint.received_host_calls().unwrap();
        assert_eq!(received.len(),2);
        assert!(received[1].matches_response(&team.records[1].result.as_ref().unwrap().content));
        let reference = team.records[2].checkpoint.as_ref().unwrap();
        let meta = controller.manager.artifacts().metadata(&tetonic_domain::ArtifactId::new(&reference.artifact_id)).await.unwrap();
        assert_eq!(meta.data_class, tetonic_domain::DataClass::Secret);
        assert_eq!(meta.producer_attempt_id.0, request.attempt);
        assert_eq!(meta.producer_run_id.0, team.records[2].run_id);
        assert!(meta.size_bytes > 0);
        // A corrupt/missing cache cannot hide or invent delivery on reconstruction.
        controller.remaining.lock().unwrap().clear();
        controller.bind_checkpoint(&request, &team.binding(&team.records[2]), &team.records[2]).await.unwrap();
        assert_eq!(*controller.remaining.lock().unwrap(),HashSet::from(["compare".into(),"wrap".into()]));
        // The preceding checkpoint predates delivery of check, even though check
        // is now a successful task and its response is already durable.
        let earlier = call(&team, &team.records[1]).await;
        controller.bind_checkpoint(&earlier, &team.binding(&team.records[1]), &team.records[1]).await.unwrap();
        assert_eq!(controller.remaining.lock().unwrap().len(),3);
        assert_eq!(team.calls.lock().unwrap().len(),count);
        let mut corrupt = reference.clone();
        corrupt.digest = format!("sha256:{}", "0".repeat(64));
        assert!(controller.manager.read_dispatch_checkpoint(&controller.parent, &corrupt).await.is_err());
        let mut replaced = request;
        replaced.checkpoint = earlier.checkpoint;
        assert!(!controller.dispatch_recorded(&replaced).await.ok);
        team.workspace.cancel(&team.receipt.root_work_id).await.unwrap();
        assert!(controller.manager.read_dispatch_checkpoint(&controller.parent, reference).await.is_err());
        team.server.abort();
    }).await;
}

#[tokio::test]
async fn conflicting_delivery_history_cannot_change_the_finish_guard() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let team = WaitingTeam::start().await;
            let controller = team.controller(Rc::new(NoDispatch));
            let request = call(&team, &team.records[2]).await;
            let original = serde_json::to_value(&request.checkpoint).unwrap();
            let before = controller.remaining.lock().unwrap().clone();
            for variant in 0..4 {
                let mut value = original.clone();
                match variant {
                    0 => value["received_host_calls"][1]["response_digest"] = "0".repeat(64).into(),
                    1 => {
                        value["received_host_calls"][1]["request"]["arguments"] =
                            serde_json::json!({"assignment_key":"wrap"})
                    }
                    2 => {
                        value["received_host_calls"][1]["request"]["call_id"] =
                            "missing-receipt".into()
                    }
                    _ => {
                        value["received_host_calls"] = serde_json::json!([]);
                    }
                }
                let mut changed = call(&team, &team.records[2]).await;
                *changed.checkpoint = serde_json::from_value(value).unwrap();
                assert!(
                    !controller.dispatch_recorded(&changed).await.ok,
                    "variant {variant}"
                );
                assert_eq!(*controller.remaining.lock().unwrap(), before);
            }
            team.stop().await;
        })
        .await;
}

#[tokio::test]
async fn missing_or_failed_checkpoint_binding_never_admits_a_worker() {
    tokio::task::LocalSet::new().run_until(async {
        let team = WaitingTeam::start().await;
        let controller = team.controller(Rc::new(NoDispatch));
        let request = call(&team, &team.records[1]).await;
        let conn = rusqlite::Connection::open(team.directory.path().join("team.db")).unwrap();
        // A historical completed receipt without a checkpoint cannot be upgraded
        // by manufacturing the state that supposedly preceded its effects.
        conn.execute("UPDATE huddle_dispatch_receipts SET payload=json_set(payload,'$.checkpoint',NULL) WHERE call_id=?1", [&request.call_id]).unwrap();
        assert!(!controller.dispatch_recorded(&request).await.ok);
        conn.execute("UPDATE huddle_dispatch_receipts SET payload=json_set(payload,'$.result',NULL) WHERE call_id=?1", [&request.call_id]).unwrap();
        conn.execute_batch("CREATE TRIGGER fail_dispatch_checkpoint BEFORE UPDATE ON huddle_dispatch_receipts WHEN json_type(NEW.payload,'$.checkpoint') = 'object' AND json_type(OLD.payload,'$.checkpoint') = 'null' BEGIN SELECT RAISE(ABORT,'injected checkpoint binding failure'); END;").unwrap();
        let before = controller.remaining.lock().unwrap().clone();
        let calls = team.calls.lock().unwrap().len();
        assert!(!controller.dispatch_recorded(&request).await.ok);
        assert_eq!(*controller.remaining.lock().unwrap(),before);
        assert_eq!(team.calls.lock().unwrap().len(),calls);
        conn.execute_batch("DROP TRIGGER fail_dispatch_checkpoint").unwrap();
        drop(conn);
        let controller = team.controller(team.workspace.clone());
        assert!(controller.dispatch_recorded(&request).await.ok);
        assert_eq!(team.calls.lock().unwrap().len(),calls);
        team.stop().await;
    }).await;
}

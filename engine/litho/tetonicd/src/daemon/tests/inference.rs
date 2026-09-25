use super::harness::daemon_with_mock;
use serde_json::json;

#[tokio::test]
async fn session_inference_round_trip_preserves_session_and_rejects_stale_change() {
    let (mut daemon, _events, _dir) = daemon_with_mock(vec![]);
    let started = daemon
        .session_start(json!({"orchestration":"off", "critic":false}))
        .await
        .unwrap();
    let sid = started["session_id"].as_str().unwrap();
    let before = daemon
        .session_inference(json!({"session_id":sid}), false)
        .unwrap();
    let command = json!({"session_id":sid, "profile":"default", "model_fast":"replacement",
        "model_hard":"replacement-hard", "expected_revision":before["revision"]});
    let changed = daemon.session_inference(command.clone(), true).unwrap();
    assert_eq!(changed["model_fast"], "replacement");
    assert_eq!(changed["model_hard"], "replacement-hard");
    assert_eq!(changed["revision"], 1);
    assert!(daemon.session_inference(command, true).is_err());
    let after = daemon
        .session_inference(json!({"session_id":sid}), false)
        .unwrap();
    assert_eq!(after, changed);
    assert!(daemon
        .session_inference(
            json!({"session_id":sid, "profile":"unregistered",
        "model_fast":"m", "model_hard":"m", "expected_revision":1}),
            true
        )
        .is_err());
    assert_eq!(
        daemon
            .session_inference(json!({"session_id":sid}), false)
            .unwrap(),
        changed
    );
    let catalog = daemon.session_models(json!({"session_id":sid})).unwrap();
    let choice = catalog["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == "replacement")
        .unwrap();
    let request = json!({"session_id":sid, "selection_id":choice["id"], "expected_revision":catalog["revision"]});
    let selected = daemon.session_select_model(request.clone()).unwrap();
    assert_eq!(selected["model_fast"], "replacement");
    assert_eq!(selected["model_hard"], "replacement");
    assert_eq!(selected["revision"], 2);
    assert!(daemon.session_select_model(request).is_err());
    assert!(daemon
        .session_select_model(
            json!({"session_id":sid, "selection_id":"unregistered", "expected_revision":2})
        )
        .is_err());
}

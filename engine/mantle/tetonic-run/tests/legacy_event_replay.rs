//! M1: run event logs written before the `ExecutionTargetId` seam still replay.
//!
//! `service.rs` persists each `RunCommand` as `payload_json` and `replay.rs` reads it back
//! with `serde_json::from_value::<RunCommand>`. Before M1, `LeaseAttempt.holder` was an
//! opaque newtype that serialised as a bare string. These tests take a real event log,
//! rewrite the holder into that legacy shape, and replay it through the production
//! function — an unreadable holder would fail the whole run projection.

mod harness;

use harness::{create_started_run, mem_supervisor, ready_task_with_attempt};
use tetonic_domain::{ExecutionTargetId, RunEventEnvelope};
use tetonic_run::{empty_snapshot, replay_from_events, RunSupervisor};

/// Rewrite every `LeaseAttempt.holder` into the pre-M1 bare-string encoding.
fn downgrade_holders_to_legacy_strings(events: &mut [RunEventEnvelope]) -> usize {
    let mut rewritten = 0;
    for event in events.iter_mut() {
        // `RunCommand` is internally tagged: {"type":"lease_attempt","holder":…,…}.
        if event.payload.get("type").and_then(|t| t.as_str()) != Some("lease_attempt") {
            continue;
        }
        // Tagged holder is {"worker":{"worker_id":"worker"}}; legacy form was "worker".
        let legacy = event
            .payload
            .get("holder")
            .and_then(|h| h.get("worker"))
            .and_then(|w| w.get("worker_id"))
            .and_then(|id| id.as_str())
            .map(|id| id.to_string())
            .expect("leased attempt should carry a Worker holder");
        event.payload["holder"] = serde_json::Value::String(legacy);
        rewritten += 1;
    }
    rewritten
}

#[tokio::test]
async fn legacy_string_holder_events_replay() {
    let sup = mem_supervisor();
    let (run, task, session) = create_started_run(&sup).await;
    let attempt = ready_task_with_attempt(&sup, &run, &task).await;

    let live = sup.snapshot(run.clone()).await.unwrap();
    let expected_holder = live
        .attempts
        .get(&attempt)
        .and_then(|a| a.lease.as_ref())
        .map(|l| l.holder.clone())
        .expect("attempt is leased");
    assert_eq!(expected_holder, ExecutionTargetId::worker("worker"));

    let mut events = live.events.clone();
    assert!(
        downgrade_holders_to_legacy_strings(&mut events) > 0,
        "test must actually rewrite a holder or it proves nothing"
    );

    let replayed = replay_from_events(&empty_snapshot(run, Some(session)), &events)
        .expect("a pre-M1 event log must still replay");

    let holder = replayed
        .attempts
        .get(&attempt)
        .and_then(|a| a.lease.as_ref())
        .map(|l| l.holder.clone())
        .expect("attempt is leased after replay");
    assert_eq!(holder, expected_holder);
    assert_eq!(replayed.sequence, live.sequence);
}

#[test]
fn legacy_local_holder_deserializes_as_local_variant() {
    let lease = harness::default_lease_cmd(
        tetonic_run::command_envelope("c1", None, "test"),
        tetonic_domain::RunId::new("run_1"),
        tetonic_domain::AttemptId::new("att_1"),
    );
    let mut payload =
        serde_json::to_value(tetonic_domain::RunCommand::LeaseAttempt(lease)).unwrap();
    payload["holder"] = serde_json::Value::String("local".into());

    let command = serde_json::from_value::<tetonic_domain::RunCommand>(payload)
        .expect("legacy local holder must deserialize");
    let tetonic_domain::RunCommand::LeaseAttempt(lease) = command else {
        panic!("expected LeaseAttempt");
    };
    assert_eq!(lease.holder, ExecutionTargetId::Local);
}

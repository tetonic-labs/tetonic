//! Rebuild run projection from authoritative events.

use lokai_domain::{
    RunCommand, RunEventEnvelope, RunId, RunSnapshot, RunSupervisorError, SessionId,
};

use crate::transition::apply_command;

pub fn empty_snapshot(run_id: RunId, session_id: Option<SessionId>) -> RunSnapshot {
    RunSnapshot {
        run_id,
        session_id,
        state: lokai_domain::RunState::Created,
        sequence: 0,
        workspace_version: None,
        tasks: Default::default(),
        attempts: Default::default(),
        dependencies: Default::default(),
        events: Vec::new(),
        delivery_index: Default::default(),
        side_effect_commits: Default::default(),
        deadlines: Default::default(),
        cancellation: Default::default(),
        speculation: Default::default(),
        next_lease_epoch: 0,
        job_spec: None,
    }
}

pub fn replay_from_events(
    base: &RunSnapshot,
    events: &[RunEventEnvelope],
) -> Result<RunSnapshot, RunSupervisorError> {
    let mut snap = base.clone();
    snap.events.clear();
    for event in events {
        let command = serde_json::from_value::<RunCommand>(event.payload.clone())
            .map_err(|e| RunSupervisorError::Persistence(e.to_string()))?;
        if event.sequence != snap.sequence + 1 {
            return Err(RunSupervisorError::Corruption(format!(
                "Sequence gap in replay: expected {}, got {}",
                snap.sequence + 1,
                event.sequence
            )));
        }
        snap = apply_command(&snap, &command)?;
        snap.sequence = event.sequence;
    }
    snap.events = events.to_vec();
    Ok(snap)
}

//! Failure metrics and trace events (M3-2).

use lokai_domain::{FailureClass, RunCommandResult, RunEventEnvelope};
use tracing::{debug, info};

pub fn emit_failure_trace(event: &RunEventEnvelope, class: Option<&FailureClass>) {
    if let Some(c) = class {
        info!(
            sequence = event.sequence,
            event_type = ?event.event_type,
            command_id = ?event.command_id,
            failure_class = ?c,
            "run failure event"
        );
    } else {
        debug!(
            sequence = event.sequence,
            event_type = ?event.event_type,
            command_id = ?event.command_id,
            "run event committed"
        );
    }
}

pub fn metric_command_replay(result: &RunCommandResult) {
    if result.idempotent_replay {
        debug!(run_id = %result.run_id, sequence = result.sequence, "command idempotent replay");
    }
}

pub fn metric_stale_rejection(run_id: &str, reason: &str) {
    info!(run_id, reason, "stale result rejected");
}

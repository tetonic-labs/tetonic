//! Map execution-contract sink outcomes to agent-facing tool results.

use lokai_domain::ExecutionOutcome;

use crate::types::{ToolError, ToolOutcome};

pub fn tool_outcome_from_execution(outcome: ExecutionOutcome) -> ToolOutcome {
    match outcome {
        ExecutionOutcome::Completed {
            ok,
            summary,
            execution_id,
            ..
        } => ToolOutcome {
            ok,
            summary: summary.clone(),
            content: if summary.is_empty() {
                format!("execution_id={execution_id}")
            } else {
                format!("{summary}\nexecution_id={execution_id}")
            },
            error_kind: if ok {
                None
            } else {
                Some("nonzero_exit".into())
            },
            change: None,
        },
        ExecutionOutcome::Failed {
            reason,
            execution_id,
        } => ToolOutcome {
            ok: false,
            summary: "execution failed".into(),
            content: format!("ERROR: {reason}\nexecution_id={execution_id}"),
            error_kind: Some("error".into()),
            change: None,
        },
        ExecutionOutcome::Cancelled { execution_id } => ToolOutcome {
            ok: false,
            summary: "execution cancelled".into(),
            content: format!("ERROR: cancelled\nexecution_id={execution_id}"),
            error_kind: Some("cancelled".into()),
            change: None,
        },
        ExecutionOutcome::Started { execution_id } => crate::types::outcome_err(ToolError::Other(
            format!("unexpected started status (execution_id={execution_id})"),
        )),
    }
}

pub fn verify_result_from_execution(outcome: ExecutionOutcome) -> (bool, String) {
    match outcome {
        ExecutionOutcome::Completed { ok, summary, .. } => (ok, summary),
        ExecutionOutcome::Failed { reason, .. } => (false, reason),
        ExecutionOutcome::Cancelled { .. } => (false, "cancelled".into()),
        ExecutionOutcome::Started { .. } => (false, "unexpected started status".into()),
    }
}

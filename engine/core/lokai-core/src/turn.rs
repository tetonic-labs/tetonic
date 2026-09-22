//! Turn workflow state machine (AC2-6).
//!
//! Operational persistence is driven by [`TurnOpsHook`] callbacks wired from
//! `lokaid` into [`crate::Agent`].

use std::sync::Arc;

/// In-flight turn phases persisted for crash recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnState {
    Idle,
    Generating,
    AwaitingApproval,
    Executing,
    Verifying,
    Complete,
}

impl TurnState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Generating => "generating",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Executing => "executing",
            Self::Verifying => "verifying",
            Self::Complete => "complete",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "idle" => Some(Self::Idle),
            "generating" => Some(Self::Generating),
            "awaiting_approval" => Some(Self::AwaitingApproval),
            "executing" => Some(Self::Executing),
            "verifying" => Some(Self::Verifying),
            "complete" => Some(Self::Complete),
            _ => None,
        }
    }

    /// True when a daemon restart should surface `recovery_required` on resume.
    pub fn requires_recovery(self) -> bool {
        matches!(
            self,
            Self::AwaitingApproval | Self::Executing | Self::Verifying
        )
    }
}

/// Validate an operational transition (incremental FSM — not every edge is used yet).
pub fn validate_transition(from: TurnState, to: TurnState) -> Result<(), String> {
    if from == to {
        return Ok(());
    }
    let ok = matches!(
        (from, to),
        (TurnState::Idle, TurnState::Generating)
            | (TurnState::Generating, TurnState::AwaitingApproval)
            | (TurnState::Generating, TurnState::Executing)
            | (TurnState::Generating, TurnState::Verifying)
            | (TurnState::Generating, TurnState::Complete)
            | (TurnState::AwaitingApproval, TurnState::Executing)
            | (TurnState::AwaitingApproval, TurnState::Generating)
            | (TurnState::Executing, TurnState::Generating)
            | (TurnState::Verifying, TurnState::Generating)
            | (_, TurnState::Complete)
            | (_, TurnState::Idle)
    );
    if ok {
        Ok(())
    } else {
        Err(format!(
            "invalid turn transition: {} -> {}",
            from.as_str(),
            to.as_str()
        ))
    }
}

#[derive(Debug, Clone)]
pub enum TurnOpsEvent {
    Persist {
        session_id: String,
        turn_id: String,
        state: TurnState,
        payload: Option<String>,
    },
    Clear {
        session_id: String,
        turn_id: String,
    },
}

pub type TurnOpsHook = Arc<dyn Fn(TurnOpsEvent) + Send + Sync>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_states() {
        assert!(TurnState::Executing.requires_recovery());
        assert!(!TurnState::Generating.requires_recovery());
        assert!(!TurnState::Complete.requires_recovery());
    }

    #[test]
    fn transitions_allow_operational_path() {
        assert!(validate_transition(TurnState::Generating, TurnState::Executing).is_ok());
        assert!(validate_transition(TurnState::Complete, TurnState::Generating).is_err());
    }
}

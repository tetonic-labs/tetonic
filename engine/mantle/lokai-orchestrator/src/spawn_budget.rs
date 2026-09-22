//! Spawn fan-out admission (H2-3) — orchestrator-facing gate, no broker types.

/// Context for a spawn reservation request (before child `Agent` is built).
#[derive(Debug, Clone)]
pub struct SpawnAdmitCtx {
    pub session_id: String,
    pub parent_agent_id: String,
    pub child_agent_id: String,
    pub depth: u32,
}

/// Opaque reservation handle returned by [`SpawnBudgetGate::admit`].
#[derive(Debug, Clone)]
pub struct SpawnReservationToken {
    pub id: String,
}

/// Typed spawn refusal (ledger full / concurrency). Not an inference error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpawnAdmitError {
    Refused { reason: String },
}

impl SpawnAdmitError {
    pub fn tool_message(&self) -> String {
        match self {
            Self::Refused { reason } => format!("spawn_refused: {reason}"),
        }
    }
}

/// Pays for a child agent context before it is created (H2-3).
///
/// Implemented in `lokai-app` against `HierarchicalBudgetLedger`. Orchestrator
/// keeps only this trait so it does not depend on the broker crate.
pub trait SpawnBudgetGate: Send + Sync {
    fn admit(&self, ctx: &SpawnAdmitCtx) -> Result<SpawnReservationToken, SpawnAdmitError>;
    fn release(&self, token: &SpawnReservationToken);
    /// Release every active spawn reservation for this gate's session (CancelRun).
    fn release_session(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct CountingGate {
        admits: AtomicUsize,
        releases: AtomicUsize,
        refuse: AtomicBool,
    }

    impl SpawnBudgetGate for CountingGate {
        fn admit(&self, ctx: &SpawnAdmitCtx) -> Result<SpawnReservationToken, SpawnAdmitError> {
            if self.refuse.load(Ordering::SeqCst) {
                return Err(SpawnAdmitError::Refused {
                    reason: "ledger at capacity".into(),
                });
            }
            self.admits.fetch_add(1, Ordering::SeqCst);
            Ok(SpawnReservationToken {
                id: format!("tok_{}", ctx.child_agent_id),
            })
        }

        fn release(&self, _token: &SpawnReservationToken) {
            self.releases.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn refused_message_is_typed_prefix() {
        let e = SpawnAdmitError::Refused {
            reason: "full".into(),
        };
        assert!(e.tool_message().starts_with("spawn_refused:"));
    }

    #[test]
    fn counting_gate_tracks_admit_release() {
        let g = CountingGate {
            admits: AtomicUsize::new(0),
            releases: AtomicUsize::new(0),
            refuse: AtomicBool::new(false),
        };
        let tok = g
            .admit(&SpawnAdmitCtx {
                session_id: "s".into(),
                parent_agent_id: "a0".into(),
                child_agent_id: "a0_s0".into(),
                depth: 1,
            })
            .unwrap();
        g.release(&tok);
        assert_eq!(g.admits.load(Ordering::SeqCst), 1);
        assert_eq!(g.releases.load(Ordering::SeqCst), 1);
    }
}

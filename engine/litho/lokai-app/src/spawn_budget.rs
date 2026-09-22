//! H2-3: spawn admission backed by `HierarchicalBudgetLedger`.

use std::sync::Arc;

use chrono::Utc;
use lokai_broker::{
    BudgetReject, HierarchicalBudgetLedger, ReservationState, ReservationTarget, ResourceRequest,
};
use lokai_domain::ids::{AttemptId, ReservationId, RunId, TaskId};
use lokai_orchestrator::{SpawnAdmitCtx, SpawnAdmitError, SpawnBudgetGate, SpawnReservationToken};

/// Pays for each spawned specialist with one Infer-concurrency slot before the
/// child `Agent` is constructed.
pub struct LedgerSpawnBudgetGate {
    budgets: Arc<HierarchicalBudgetLedger>,
    run_id: RunId,
}

impl LedgerSpawnBudgetGate {
    pub fn new(budgets: Arc<HierarchicalBudgetLedger>, run_id: RunId) -> Self {
        Self { budgets, run_id }
    }

    pub fn into_arc(self) -> Arc<dyn SpawnBudgetGate> {
        Arc::new(self)
    }

    pub fn into_rc(self) -> std::rc::Rc<dyn SpawnBudgetGate> {
        std::rc::Rc::new(self)
    }
}

impl SpawnBudgetGate for LedgerSpawnBudgetGate {
    fn admit(&self, ctx: &SpawnAdmitCtx) -> Result<SpawnReservationToken, SpawnAdmitError> {
        let task_id = TaskId::new(format!("task_spawn_{}", ctx.child_agent_id));
        let attempt_id = AttemptId::new(format!("att_spawn_{}", ctx.child_agent_id));
        let request = ResourceRequest::spawned_agent_default();
        match self.budgets.reserve(
            &self.run_id,
            &task_id,
            &attempt_id,
            None,
            ReservationTarget::Local,
            &request,
            false,
            Utc::now(),
        ) {
            Ok(res) => Ok(SpawnReservationToken {
                id: res.reservation_id.0,
            }),
            Err(e) => Err(SpawnAdmitError::Refused {
                reason: budget_reject_reason(e),
            }),
        }
    }

    fn release(&self, token: &SpawnReservationToken) {
        self.budgets.release(
            &ReservationId::new(token.id.clone()),
            ReservationState::Released,
        );
    }

    fn release_session(&self) {
        let n = self
            .budgets
            .release_for_run(&self.run_id, ReservationState::Canceled);
        if n > 0 {
            tracing::info!(
                run_id = %self.run_id,
                released = n,
                "spawn budget: released reservations on session cancel"
            );
        }
    }
}

fn budget_reject_reason(e: BudgetReject) -> String {
    format!("budget rejected: {e:?}")
}

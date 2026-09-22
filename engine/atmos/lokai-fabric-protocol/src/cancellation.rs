//! Idempotent cancellation tracking (M5-1).

use std::collections::HashMap;

use lokai_domain::ids::{AttemptId, LeaseId, RunId, TaskId};

use crate::{CancellationAcknowledged, CancellationRequest, FabricError, FabricErrorCode};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CancellationKey {
    pub run_id: RunId,
    pub task_id: TaskId,
    pub attempt_id: AttemptId,
    pub lease_id: LeaseId,
    pub lease_epoch: u64,
}

impl CancellationKey {
    pub fn from_request(req: &CancellationRequest) -> Self {
        Self {
            run_id: req.run_id.clone(),
            task_id: req.task_id.clone(),
            attempt_id: req.attempt_id.clone(),
            lease_id: req.lease_id.clone(),
            lease_epoch: req.lease_epoch,
        }
    }

    pub fn digest(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}",
            self.run_id, self.task_id, self.attempt_id, self.lease_id, self.lease_epoch
        )
    }
}

#[derive(Clone, Debug, Default)]
pub struct CancellationState {
    acknowledged: HashMap<String, CancellationAcknowledged>,
    /// Attempt + lease epoch pairs that have been canceled.
    canceled: HashMap<String, u64>,
}

impl CancellationState {
    pub fn acknowledge(
        &mut self,
        req: &CancellationRequest,
        ack: CancellationAcknowledged,
    ) -> Result<bool, FabricError> {
        crate::validate::validate_cancellation(req)?;
        let key = CancellationKey::from_request(req).digest();
        if self.acknowledged.contains_key(&key) {
            return Ok(false);
        }
        self.acknowledged.insert(key.clone(), ack);
        self.canceled.insert(
            cancel_key(&req.attempt_id, req.lease_epoch),
            req.lease_epoch,
        );
        Ok(true)
    }

    pub fn is_canceled(&self, attempt_id: &AttemptId, lease_epoch: u64) -> bool {
        self.canceled
            .contains_key(&cancel_key(attempt_id, lease_epoch))
    }

    pub fn can_renew_lease(
        &self,
        attempt_id: &AttemptId,
        lease_epoch: u64,
    ) -> Result<(), FabricError> {
        if self.is_canceled(attempt_id, lease_epoch) {
            return Err(FabricError {
                code: FabricErrorCode::InvalidLease,
                message: "lease renewal after cancellation".into(),
                details: None,
            });
        }
        Ok(())
    }
}

fn cancel_key(attempt_id: &AttemptId, lease_epoch: u64) -> String {
    format!("{}:{}", attempt_id.0, lease_epoch)
}

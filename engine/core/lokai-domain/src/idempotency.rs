//! Idempotency key models (M3-2).

use serde::{Deserialize, Serialize};

use crate::ids::{AttemptId, RunId, TaskId, WorkspaceVersion};
use crate::run::TaskInputBinding;

/// Logical work identity: run + task version + workspace + inputs + policy.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskIdempotencyKey {
    pub run_id: RunId,
    pub task_id: TaskId,
    pub task_definition_version: u64,
    pub workspace_version: Option<WorkspaceVersion>,
    pub input_digests: Vec<String>,
    pub policy_version: u64,
}

impl TaskIdempotencyKey {
    pub fn from_binding(run_id: &RunId, task_id: &TaskId, binding: &TaskInputBinding) -> Self {
        Self {
            run_id: run_id.clone(),
            task_id: task_id.clone(),
            task_definition_version: binding.task_definition_version,
            workspace_version: binding.workspace_version.clone(),
            input_digests: binding
                .input_artifacts
                .iter()
                .map(|a| a.digest.clone())
                .collect(),
            policy_version: 1,
        }
    }

    pub fn digest(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        self.hash(&mut h);
        format!("task_idem:{:x}", h.finish())
    }
}

/// One dispatch of one attempt to one execution target.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AttemptDeliveryKey {
    pub attempt_id: AttemptId,
    pub target: String,
    pub dispatch_sequence: u64,
}

impl AttemptDeliveryKey {
    pub fn new(attempt_id: &AttemptId, target: impl Into<String>, dispatch_sequence: u64) -> Self {
        Self {
            attempt_id: attempt_id.clone(),
            target: target.into(),
            dispatch_sequence,
        }
    }

    pub fn digest(&self) -> String {
        format!(
            "delivery:{}:{}:{}",
            self.attempt_id, self.target, self.dispatch_sequence
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_idempotency_key_stable() {
        let k1 = TaskIdempotencyKey::from_binding(
            &RunId::new("run_1"),
            &TaskId::new("task_1"),
            &TaskInputBinding::default(),
        );
        let k2 = TaskIdempotencyKey::from_binding(
            &RunId::new("run_1"),
            &TaskId::new("task_1"),
            &TaskInputBinding::default(),
        );
        assert_eq!(k1.digest(), k2.digest());
    }
}

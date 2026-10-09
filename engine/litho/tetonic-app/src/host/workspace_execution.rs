//! Operator ceilings for local workspace agents and managed plan coordination.
use serde::{Deserialize, Serialize};

use crate::errors::AppError;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkspaceExecutionConfiguration {
    /// None uses a finite host ceiling of 32 steps. Saved agent limits remain independent.
    pub max_steps: Option<usize>,
    pub max_seconds: u64,
    /// Reported input plus output tokens per worker run, not a spend allowance.
    pub max_tokens: u64,
    pub coordination_max_steps: usize,
    /// Coordination is charged inside the plan total, separately from workers.
    pub coordination_max_tokens: u64,
}

impl Default for WorkspaceExecutionConfiguration {
    fn default() -> Self {
        Self {
            max_steps: None,
            max_seconds: 600,
            max_tokens: 12_288,
            coordination_max_steps: 16,
            coordination_max_tokens: 4096,
        }
    }
}

impl WorkspaceExecutionConfiguration {
    pub(crate) fn worker_steps(&self, _has_workspace: bool) -> usize {
        self.max_steps.unwrap_or(32)
    }

    pub(crate) fn coordination_tokens(&self) -> u64 {
        self.coordination_max_tokens.min(self.max_tokens)
    }

    pub(crate) fn validate(&self) -> Result<(), AppError> {
        for (field, value, minimum, maximum) in [
            ("max_steps", self.max_steps.unwrap_or(4) as u64, 1, 512),
            ("max_seconds", self.max_seconds, 10, 86_400),
            ("max_tokens", self.max_tokens, 256, 1_000_000),
            (
                "coordination_max_steps",
                self.coordination_max_steps as u64,
                1,
                512,
            ),
            (
                "coordination_max_tokens",
                self.coordination_max_tokens,
                256,
                1_000_000,
            ),
        ] {
            if !(minimum..=maximum).contains(&value) {
                return Err(AppError::InvalidRequest(format!(
                    "workspace_execution.{field} must be between {minimum} and {maximum}"
                )));
            }
        }
        Ok(())
    }
}

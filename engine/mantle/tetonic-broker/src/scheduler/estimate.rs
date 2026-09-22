//! Completion-time estimates and uncertainty margins (M6-2).

use crate::scheduler::types::ExecutionTargetId;

#[derive(Debug, Clone)]
pub struct UncertaintyModel {
    /// Absolute error floor for workers with little history (ms).
    pub cold_floor_ms: u64,
    /// Extra margin when model is cold (ms).
    pub cold_model_extra_ms: u64,
    /// Rolling mean absolute error when history exists (ms).
    pub rolling_mae_ms: u64,
    /// Sample count for this target.
    pub samples: u32,
}

impl UncertaintyModel {
    pub fn margin_ms(&self, cold_start: bool) -> u64 {
        let mut m = if self.samples < 5 {
            self.cold_floor_ms.max(self.rolling_mae_ms)
        } else {
            (self.rolling_mae_ms * 120) / 100 // ~p60-ish inflate
        };
        if cold_start {
            m = m.saturating_add(self.cold_model_extra_ms);
        }
        m.max(25)
    }
}

impl Default for UncertaintyModel {
    fn default() -> Self {
        Self {
            cold_floor_ms: 250,
            cold_model_extra_ms: 500,
            rolling_mae_ms: 0,
            samples: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CandidateInputs {
    pub target: ExecutionTargetId,
    pub admission_delay_ms: u64,
    pub queue_delay_ms: u64,
    pub connection_setup_ms: u64,
    pub input_transfer_ms: u64,
    pub cold_start_ms: u64,
    pub execution_ms: u64,
    pub result_transfer_ms: u64,
    pub verification_ms: u64,
    pub queue_depth: u32,
    pub transfer_bytes: u64,
    pub cold_start: bool,
    pub uncertainty: UncertaintyModel,
}

#[derive(Debug, Clone)]
pub struct CompletionEstimate {
    pub finish_ms: u64,
    pub uncertainty_margin_ms: u64,
    pub scored_ms: u64,
}

pub fn estimate_finish_ms(input: &CandidateInputs) -> CompletionEstimate {
    let finish = input
        .admission_delay_ms
        .saturating_add(input.queue_delay_ms)
        .saturating_add(input.connection_setup_ms)
        .saturating_add(input.input_transfer_ms)
        .saturating_add(input.cold_start_ms)
        .saturating_add(input.execution_ms)
        .saturating_add(input.result_transfer_ms)
        .saturating_add(input.verification_ms);
    let margin = input.uncertainty.margin_ms(input.cold_start);
    CompletionEstimate {
        finish_ms: finish,
        uncertainty_margin_ms: margin,
        scored_ms: finish.saturating_add(margin),
    }
}

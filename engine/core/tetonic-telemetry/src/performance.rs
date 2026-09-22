//! Developer-only monotonic timing. No prompts, arguments, or model outputs.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

static NEXT_TIMING_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy)]
pub enum PerfStage {
    Startup,
    TaskTurn,
    Context,
    Inference,
    InferenceScan,
    InferenceSchedule,
    InferenceDiscovery,
    InferenceSchedulePersist,
    InferenceHeaders,
    InferenceFirstChunk,
    InferenceFirstContent,
    InferenceStream,
    Tool,
    Finalization,
}

impl PerfStage {
    fn label(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::TaskTurn => "task_turn",
            Self::Context => "context",
            Self::Inference => "inference",
            Self::InferenceScan => "inference_scan",
            Self::InferenceSchedule => "inference_schedule",
            Self::InferenceDiscovery => "inference_discovery",
            Self::InferenceSchedulePersist => "inference_schedule_persist",
            Self::InferenceHeaders => "inference_headers",
            Self::InferenceFirstChunk => "inference_first_chunk",
            Self::InferenceFirstContent => "inference_first_content",
            Self::InferenceStream => "inference_stream",
            Self::Tool => "tool",
            Self::Finalization => "finalization",
        }
    }
}

pub struct StageTimer {
    stage: PerfStage,
    started: Instant,
    outcome: &'static str,
    timing_id: u64,
}

impl StageTimer {
    pub fn start(stage: PerfStage) -> Self {
        Self {
            stage,
            started: Instant::now(),
            outcome: "interrupted",
            timing_id: NEXT_TIMING_ID.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// Announce entry before awaiting external work. A hard process kill cannot
    /// run Drop; the unmatched start remains evidence of an unfinished scope.
    pub fn start_visible(stage: PerfStage) -> Self {
        let timer = Self::start(stage);
        tracing::debug!(target: "lokai_performance", stage = timer.stage.label(),
            timing_id = timer.timing_id, outcome = "started", duration_ms = 0.0,
            "performance stage entered");
        timer
    }

    pub fn finish(mut self, success: bool) {
        self.outcome = if success { "succeeded" } else { "unsuccessful" };
    }
}

impl Drop for StageTimer {
    fn drop(&mut self) {
        tracing::debug!(target: "lokai_performance",
            stage = self.stage.label(), outcome = self.outcome,
            timing_id = self.timing_id,
            duration_ms = self.started.elapsed().as_secs_f64() * 1000.0,
            "performance stage");
    }
}

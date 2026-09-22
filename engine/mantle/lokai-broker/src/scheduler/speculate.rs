//! Speculation eligibility (M6-2). Does not bypass admission budgets.

use lokai_domain::SpeculationConfig;
use lokai_fabric_protocol::JobKind;

use crate::job_profile::profile_for;
use crate::scheduler::types::SchedulerDecision;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeculationDenyReason {
    ConfigDisabled,
    JobProfileDisallows,
    SideEffectful,
    NonIdempotentKind,
}

impl SpeculationDenyReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ConfigDisabled => "config_disabled",
            Self::JobProfileDisallows => "job_profile_disallows",
            Self::SideEffectful => "side_effectful",
            Self::NonIdempotentKind => "non_idempotent_kind",
        }
    }
}

pub fn speculation_allowed(
    kind: &JobKind,
    config: &SpeculationConfig,
) -> Result<(), SpeculationDenyReason> {
    if !config.allowed {
        return Err(SpeculationDenyReason::ConfigDisabled);
    }
    if matches!(kind, JobKind::IndexShard | JobKind::TestShard) {
        return Err(SpeculationDenyReason::NonIdempotentKind);
    }
    let profile = profile_for(kind);
    if !profile.supports_speculation {
        return Err(SpeculationDenyReason::JobProfileDisallows);
    }
    if !profile.side_effect_free {
        return Err(SpeculationDenyReason::SideEffectful);
    }
    Ok(())
}

/// Session IDs for a speculative race. Must match `race_speculative_infer` so
/// `ActiveJobRegistry::cancel_session` rejects the late loser at fabric accept.
pub fn speculative_race_sessions(base_session: &str) -> (String, String) {
    (
        format!("{base_session}:primary"),
        format!("{base_session}:spec"),
    )
}

/// Tail-latency trigger: speculate when the selected remote estimate has a large
/// uncertainty margin relative to the local candidate (or expected speedup is thin).
pub fn should_speculate_for_tail(decision: &SchedulerDecision) -> bool {
    let Some(selected) = decision.selected_target.as_ref() else {
        return false;
    };
    if matches!(selected, crate::scheduler::types::ExecutionTargetId::Local) {
        return false;
    }
    let local_finish = decision
        .candidates
        .iter()
        .find(|c| matches!(c.target, crate::scheduler::types::ExecutionTargetId::Local))
        .map(|c| c.estimated_finish_ms)
        .unwrap_or(0);
    let remote = decision.candidates.iter().find(|c| &c.target == selected);
    let Some(remote) = remote else {
        return false;
    };
    // High uncertainty on the selected remote ⇒ race a second target.
    if remote.uncertainty_margin_ms >= 200
        && remote.uncertainty_margin_ms * 2 >= remote.estimated_finish_ms
    {
        return true;
    }
    // Thin speedup vs local ⇒ speculation can cut tail.
    if local_finish > 0 {
        let speedup = local_finish as f32 / remote.estimated_finish_ms.max(1) as f32;
        if speedup < 1.5 {
            return true;
        }
    }
    decision.uncertainty_margin_ms >= 150
}

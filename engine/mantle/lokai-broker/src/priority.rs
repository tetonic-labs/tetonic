//! Priority and fairness (M6-1).

use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum ComputePriority {
    Interactive,
    VerificationCritical,
    #[default]
    Normal,
    Background,
    Maintenance,
}

impl ComputePriority {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::VerificationCritical => "verification_critical",
            Self::Normal => "normal",
            Self::Background => "background",
            Self::Maintenance => "maintenance",
        }
    }

    /// Lower number = higher scheduling preference.
    pub fn rank(self) -> u8 {
        match self {
            Self::Interactive => 0,
            Self::VerificationCritical => 1,
            Self::Normal => 2,
            Self::Background => 3,
            Self::Maintenance => 4,
        }
    }
}

/// Aging / fairness knobs for the admission queue.
#[derive(Debug, Clone)]
pub struct FairnessPolicy {
    pub max_consecutive_background: u32,
    pub interactive_reserved_slots: u32,
    pub age_boost_after_ms: u64,
}

impl Default for FairnessPolicy {
    fn default() -> Self {
        Self {
            max_consecutive_background: 4,
            interactive_reserved_slots: 2,
            age_boost_after_ms: 2_000,
        }
    }
}

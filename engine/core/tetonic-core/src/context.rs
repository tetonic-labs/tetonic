//! Context budget breakdown for the Context Inspector.

use tetonic_domain::DataClass;

/// A token breakdown of what we actually send the model this turn — the seed of
/// the user-visible Context Inspector. `system` + `tools` are the **stable
/// prefix** (KV-cache reusable); `conversation` is the volatile suffix.
#[derive(Debug, Clone)]
pub struct ContextReport {
    pub system_tokens: usize,
    pub tools_tokens: usize,
    pub conversation_tokens: usize,
    pub total_tokens: usize,
    pub budget: usize,
    /// Older messages trimmed to fit the budget this turn.
    pub dropped_messages: usize,
    /// True if counts are from a heuristic estimator, not an exact tokenizer.
    pub estimated: bool,
    /// M2-2: effective session data class for this context snapshot.
    pub data_class: Option<DataClass>,
}

impl ContextReport {
    pub fn utilization(&self) -> f32 {
        if self.budget == 0 {
            0.0
        } else {
            self.total_tokens as f32 / self.budget as f32
        }
    }
}

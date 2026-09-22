use crate::types::{ContextEvidence, ContextOmission, TokenBudget, TokenUsage};

/// Approximate token count for a string — 4 chars ≈ 1 token (GPT-4 heuristic).
/// Used as the fallback when no exact tokenizer is configured.
pub fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

/// Stage 6: Token budgeting.
///
/// Allocates the available budget across evidence items in ranked order.
/// A configurable safety reserve is held back for agent reasoning, tool calls,
/// and follow-up context expansion, so the budget used here is:
///
///   effective_budget = max_tokens − safety_reserve
///
/// Items that do not fit within the effective budget are recorded as omissions.
pub fn apply_budget(
    deduped: Vec<ContextEvidence>,
    budget: &TokenBudget,
) -> Result<(Vec<ContextEvidence>, Vec<ContextOmission>, TokenUsage), String> {
    if budget.safety_reserve >= budget.max_tokens {
        return Err(format!(
            "safety_reserve ({}) must be less than max_tokens ({})",
            budget.safety_reserve, budget.max_tokens
        ));
    }

    let effective = budget.max_tokens - budget.safety_reserve;
    let mut used = 0usize;
    let mut budgeted = Vec::new();
    let mut omissions = Vec::new();

    let mut token_by_evidence = 0usize;

    for ev in deduped {
        let tokens = estimate_tokens(&ev.text);
        if used + tokens <= effective {
            used += tokens;
            token_by_evidence += tokens;
            budgeted.push(ev);
        } else {
            omissions.push(ContextOmission {
                reason: format!(
                    "budget exhausted: needed {} tokens, {} remaining",
                    tokens,
                    effective.saturating_sub(used)
                ),
                path: ev.repository_path.clone(),
            });
        }
    }

    let usage = TokenUsage {
        objective: 0,      // filled in by seal stage
        repository_map: 0, // filled in by seal stage
        evidence: token_by_evidence,
        relationships: 0,
        tests: 0,
        diff: 0,
        prior_decisions: 0,
        total: used,
    };

    Ok((budgeted, omissions, usage))
}

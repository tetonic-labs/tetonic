//! Preflight estimate, not a reservation or a guarantee of provider token use.
//! Actual reported usage remains enforced by the existing managed budget ledger.
use super::*;
use tetonic_core::{HeuristicTokenizer, Tokenizer};

pub(in crate::work) fn input(
    brief: &str,
    brief_revision: i64,
    content: &PlanContent,
    revision: i64,
) -> String {
    format!("Shared brief (revision {brief_revision}):\n{brief}\n\nAgreed plan (revision {revision}):\n{}",
        serde_json::json!({"title":content.title,"summary":content.summary,
            "assignments":content.assignments.iter().map(|a|serde_json::json!({"key":a.key,"title":a.title,"depends_on":a.depends_on,"deliverable":a.deliverable})).collect::<Vec<_>>() }))
}

pub(in crate::work) fn minimum(brief: &str, content: &PlanContent) -> u64 {
    let keys: Vec<_> = content.assignments.iter().map(|a| a.key.clone()).collect();
    let mut tools = crate::resources::plan_dispatch::advertisements::coordinator(&keys);
    tools.push(crate::resources::plan_dispatch::advertisements::human());
    let schemas = serde_json::json!(tools.iter().map(|ad| serde_json::json!({"type":"function", "function":{"name":ad.name,"description":ad.description,"parameters":ad.parameters}})).collect::<Vec<_>>()).to_string();
    let prefix = HeuristicTokenizer.count(INSTRUCTIONS)
        + HeuristicTokenizer.count(&input(brief, 1, content, 1))
        + HeuristicTokenizer.count(&schemas);
    // One dispatch and one synthesis call repeat the prefix. Reserve space for
    // concise contributions, message framing, reasoning and the final answer.
    // Verbose outputs, extra turns and provider tokenization may cost more.
    let estimate = 2 * prefix as u64 + 384 * keys.len() as u64 + 1024;
    estimate.div_ceil(128) * 128
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_and_contributions_increase_overhead_without_changing_allocations() {
        let content: PlanContent = serde_json::from_value(serde_json::json!({"title":"Review","summary":"Independent review","token_budget":8000,"open_questions":[],"assignments":[{"key":"a","agent_key":"reviewer","title":"Read","instructions":"Read supplied evidence","tools":[],"depends_on":[],"deliverable":"Concise finding","token_budget":2000}]})).unwrap();
        let small = minimum("Shared facts", &content);
        assert!(small > 1000 && small <= 4096);
        assert!(minimum(&"Longer evidence. ".repeat(200), &content) > small);
        let mut pair = content.clone();
        let mut second = pair.assignments[0].clone();
        second.key = "b".into();
        pair.assignments.push(second);
        assert!(minimum("Shared facts", &pair) > small);
        assert_eq!(content.token_budget, 8000);
        assert_eq!(content.assignments[0].token_budget, 2000);
    }
}

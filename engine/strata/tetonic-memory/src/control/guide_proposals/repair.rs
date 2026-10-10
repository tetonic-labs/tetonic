//! Repairs may change representation and allocation, never the promised work.
use super::PlanContent;
use serde_json::Value;

pub(super) fn preserves_intent(
    first: &Value,
    direction: &str,
    candidate: Option<&PlanContent>,
) -> bool {
    let Some(candidate) = candidate else {
        return false;
    };
    let (original_direction, original) = if first.get("rejected_arguments").is_some() {
        let arguments = &first["rejected_arguments"];
        let plan = match arguments["plan"].as_str() {
            Some(text) => serde_json::from_str(text).unwrap_or(Value::Null),
            None => arguments["plan"].clone(),
        };
        (&arguments["direction"], plan)
    } else {
        (&first["direction"], first["content"].clone())
    };
    if original_direction != direction {
        return false;
    }
    let Ok(original) = serde_json::from_value::<PlanContent>(original) else {
        return false;
    };
    let mut comparison = candidate.clone();
    if original.assignments.len() != comparison.assignments.len() {
        return false;
    }
    // Compare dependency graphs by assignment position, allowing invalid keys to
    // be repaired without adding/removing dependencies. Old agent references are
    // understood only when there is one unambiguous assignment for that agent.
    let Some(old_edges) = edges(&original, true) else {
        return false;
    };
    let Some(new_edges) = edges(candidate, false) else {
        return false;
    };
    if old_edges != new_edges {
        return false;
    }
    for (before, after) in original.assignments.iter().zip(&mut comparison.assignments) {
        after.token_budget = before.token_budget;
        after.key = before.key.clone();
        after.depends_on = before.depends_on.clone();
    }
    comparison == original
}

fn edges(plan: &PlanContent, accept_agent_reference: bool) -> Option<Vec<Vec<usize>>> {
    plan.assignments
        .iter()
        .map(|assignment| {
            let mut result: Vec<_> = assignment
                .depends_on
                .iter()
                .map(|key| {
                    let matching: Vec<_> = plan
                        .assignments
                        .iter()
                        .enumerate()
                        .filter(|(_, a)| a.key == *key)
                        .map(|(i, _)| i)
                        .collect();
                    match matching.as_slice() {
                        [index] => Some(*index),
                        [] if accept_agent_reference => {
                            let matching: Vec<_> = plan
                                .assignments
                                .iter()
                                .enumerate()
                                .filter(|(_, a)| a.agent_key == *key)
                                .map(|(i, _)| i)
                                .collect();
                            match matching.as_slice() {
                                [index] => Some(*index),
                                _ => None,
                            }
                        }
                        _ => None,
                    }
                })
                .collect::<Option<_>>()?;
            result.sort_unstable();
            Some(result)
        })
        .collect()
}

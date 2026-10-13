//! Fit the automatic overview without turning workspace growth into a chat error.
//! Counts, focus, authority and usage stay intact; optional rows are explicitly partial.
use super::*;

const MAX_BYTES: usize = 16_000;

pub(super) fn bound(mut value: serde_json::Value) -> Result<String, AppError> {
    if let Some(notes) = value.pointer_mut("/discussion_context/subject/understanding") {
        if let Some(text) = notes.as_str() {
            if text.len() > 1800 {
                *notes=format!("{}… [excerpt; read this subject through work_plan context for its original understanding]",tetonic_context::conversation::excerpt(text,1800)).into();
                value["discussion_context"]["subject_excerpt"] = true.into();
            }
        }
    }
    let mut encoded = value.to_string();
    if encoded.len() <= MAX_BYTES {
        return Ok(encoded);
    }
    value["partial"] = true.into();
    value["omitted_from_snapshot"] = serde_json::json!({});
    value["detail_lookup"] = "Use work_plan work for current work, work with a recorded work_id for its detail, resources for saved agents and limits, and inspect for this conversation's full proposal. Omitted rows are not absent resources or completed work.".into();
    // Work is already ordered active-first. Drop low-priority tail entries first,
    // then proposal detail and inventory; never cut an ID or tool grant in half.
    for (path, label) in [
        ("/discussion_context/subjects", "subjects"),
        ("/work", "work"),
        ("/saved_plan/assignments", "proposal_assignments"),
        ("/agents", "agents"),
        ("/local_access/available_folders", "available_folders"),
    ] {
        let mut omitted = 0;
        while value.to_string().len() > MAX_BYTES {
            let removed = value
                .pointer_mut(path)
                .and_then(serde_json::Value::as_array_mut)
                .and_then(Vec::pop);
            if removed.is_none() {
                break;
            }
            omitted += 1;
            value["omitted_from_snapshot"][label] = omitted.into();
        }
    }
    encoded = value.to_string();
    if encoded.len() > MAX_BYTES {
        return Err(AppError::InvalidRequest("The workspace metadata exceeds the conversation limit. Review the configured team and folder metadata.".into()));
    }
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_overview_keeps_active_work_totals_scope_and_precise_ids() {
        let tasks: Vec<_> = (0..24)
            .map(|i| {
                serde_json::json!({
                    "id":format!("work-{i}"),"state":if i==0 { "running" } else { "completed" },
                    "title":"資料と証拠".repeat(120),"depends_on":["prerequisite"]
                })
            })
            .collect();
        let input = serde_json::json!({
            "scope":{"organization":"org","team":"team"},
            "focus":{"work_count":80,"completed_worker_assignments":70},
            "workspace_totals":{"reported_tokens_including_discussions":12345,"unconfirmed_calls":3},
            "work":tasks,"agents":[{"key":"saved-agent","tools":["read_file"]}],
            "limits":"Inventory is not a grant.","partial":true
        });
        let encoded = bound(input.clone()).unwrap();
        assert!(encoded.len() <= MAX_BYTES);
        let result: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        for key in ["scope", "focus", "workspace_totals", "limits", "agents"] {
            assert_eq!(result[key], input[key]);
        }
        let retained = result["work"].as_array().unwrap();
        assert!(!retained.is_empty());
        assert_eq!(retained[0]["id"], "work-0");
        assert_eq!(result["omitted_from_snapshot"]["work"], 24 - retained.len());
    }

    #[test]
    fn large_proposal_stays_identifiable_and_small_overviews_are_unchanged() {
        let small = serde_json::json!({"work":[],"partial":false});
        assert_eq!(bound(small.clone()).unwrap(), small.to_string());
        let proposal = serde_json::json!({
            "saved_plan":{"revision":9,"status":"proposed","token_budget":300000,
                "assignments":(0..32).map(|i|serde_json::json!({"key":format!("task-{i}"),"instructions_excerpt":"detail ".repeat(80)})).collect::<Vec<_>>()},
            "agents":[{"key":"available-agent"}],"partial":false
        });
        let result: serde_json::Value =
            serde_json::from_str(&bound(proposal.clone()).unwrap()).unwrap();
        assert_eq!(result["saved_plan"]["revision"], 9);
        assert_eq!(result["saved_plan"]["token_budget"], 300000);
        assert_eq!(result["agents"], proposal["agents"]);
        assert_eq!(result["partial"], true);
        assert!(
            result["omitted_from_snapshot"]["proposal_assignments"]
                .as_u64()
                .unwrap()
                > 0
        );
    }
}

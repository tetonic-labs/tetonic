//! In-loop `spawn_agent` orchestration tool (A13).

use serde_json::json;

/// JSON-schema args for `spawn_agent`.
pub fn spawn_agent_parameters_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "role": {
                "type": "string",
                "description": "Specialist role: planner, coder, debugger, reviewer, or critic",
                "enum": ["planner", "coder", "debugger", "reviewer", "critic"]
            },
            "task": {
                "type": "string",
                "description": "Task for the spawned specialist (goal + constraints)"
            }
        },
        "required": ["role", "task"]
    })
}

pub fn spawn_agent_tool_name() -> &'static str {
    "spawn_agent"
}

pub fn spawn_agent_description() -> &'static str {
    "Spawn a specialist sub-agent to handle a focused sub-task in-process. \
Returns a summary when the specialist finishes. Roles: planner (read-only plan), \
coder (implement), debugger (fix failures), reviewer/critic (read-only review)."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_schema_requires_role_and_task() {
        let s = spawn_agent_parameters_schema();
        let req = s["required"].as_array().unwrap();
        assert!(req.iter().any(|v| v == "role"));
        assert!(req.iter().any(|v| v == "task"));
    }
}

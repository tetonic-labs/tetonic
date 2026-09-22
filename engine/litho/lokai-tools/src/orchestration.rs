//! Orchestration-only tools (A13) — advertised on root agent when swarm mode is on.

use serde_json::json;

use crate::ToolDef;

pub fn orchestration_tool_defs() -> Vec<ToolDef> {
    vec![ToolDef {
        name: "spawn_agent",
        description: "Spawn a specialist sub-agent to handle a focused sub-task in-process. \
Returns a summary when the specialist finishes. Roles: planner (read-only plan), \
coder (implement), debugger (fix failures), reviewer/critic (read-only review).",
        parameters: json!({
            "type": "object",
            "properties": {
                "role": {
                    "type": "string",
                    "description": "Specialist role",
                    "enum": ["planner", "coder", "debugger", "reviewer", "critic"]
                },
                "task": {
                    "type": "string",
                    "description": "Task for the spawned specialist"
                }
            },
            "required": ["role", "task"]
        }),
        mutating: false,
    }]
}

#[derive(serde::Deserialize)]
pub struct SpawnAgentArgs {
    pub role: String,
    pub task: String,
}

pub fn parse_spawn_agent_args(args: serde_json::Value) -> Result<SpawnAgentArgs, crate::ToolError> {
    serde_json::from_value(args)
        .map_err(|e| crate::ToolError::Other(format!("spawn_agent args: {e}")))
}

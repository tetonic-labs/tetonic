//! Conversation planning through the existing managed asynchronous tool boundary.
//! This capability can inspect and propose work; it cannot agree, dispatch or grant.
use serde::Deserialize;
use tetonic_domain::{ToolAdvertisement, ToolOutcome};

pub(crate) const CONTROL: &str = "work_plan";

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Command {
    Inspect {},
    Resources {},
    Work {
        #[serde(default)]
        work_id: Option<String>,
    },
    Propose {
        direction: String,
        plan: Box<tetonic_memory::PlanContent>,
    },
}

pub(crate) fn command(mut value: serde_json::Value) -> Result<Command, String> {
    if value.to_string().len() > 60_000 {
        return Err("Keep the proposal concise.".into());
    }
    if value["operation"] == "propose" && value["plan"].is_string() {
        return Err("work_plan.plan must be a JSON object, not a quoted JSON string. Send its fields directly inside plan; do not stringify the proposal.".into());
    }
    // A fixed, fully required outer shape is easier for model tool callers.
    // Only null placeholders are omitted for inspection, never unknown fields.
    if value["operation"] != "propose" {
        if let Some(fields) = value.as_object_mut() {
            for name in ["direction", "plan"] {
                if fields.get(name).is_some_and(serde_json::Value::is_null) {
                    fields.remove(name);
                }
            }
        }
    }
    if value["operation"] != "work" && value["work_id"].is_null() {
        if let Some(fields) = value.as_object_mut() {
            fields.remove("work_id");
        }
    }
    let parsed: Command = serde_json::from_value(value).map_err(|error| {
        let detail: String = error.to_string().chars().take(240).collect();
        format!("Invalid work_plan arguments: {detail}. Propose requires direction and plan; reads use null direction and plan. Only work accepts a non-null work_id.")
    })?;
    if let Command::Propose { direction, plan } = &parsed {
        if direction.trim().is_empty() || direction.len() > 12_000 || direction.contains('\0') {
            return Err("Provide a concise shared direction of at most 12000 bytes.".into());
        }
        plan.validate().map_err(|e| e.to_string())?;
    }
    if let Command::Work { work_id: Some(id) } = &parsed {
        uuid::Uuid::parse_str(id)
            .map_err(|_| "Use a recorded work_id from the workspace listing".to_string())?;
    }
    Ok(parsed)
}

pub(crate) struct Call {
    pub attempt: String,
    pub call_id: String,
    pub command: Command,
    pub reply: tokio::sync::oneshot::Sender<ToolOutcome>,
}

#[derive(Clone)]
pub(crate) struct DirectorBinding {
    pub source: String,
    pub turn: String,
    pub revision: i64,
    pub brief_revision: i64,
    pub plan_schema: serde_json::Value,
    pub sender: tokio::sync::mpsc::Sender<Call>,
}

impl DirectorBinding {
    pub(crate) fn advertisement(&self) -> ToolAdvertisement {
        ToolAdvertisement {
            name: CONTROL.into(),
            description: "Guide-only workspace control. resources: inspect current saved agents, teams, skills, connectors, usage and execution ceilings before allocating work. work: list current workspace efforts, or inspect one recorded work_id and its result. inspect: this conversation's saved plan and readiness. propose: save a real delegation proposal with shared direction and a complete plan using saved agent keys. One saved proposal per reply. If the engine returns repair_needed, one correction of assignment token allocations is allowed within this reply's existing budget. Preserve total allowance, shared direction, contributors, dependencies, tools and deliverables. On repair_failed, explain what needs the owner's input and stop proposing; never ask the owner to calculate coordination tokens. Does not dispatch or grant access. The owner starts the reviewed proposal inline.".into(),
            parameters: serde_json::json!({"type":"object","additionalProperties":false,
                "required":["operation","direction","plan","work_id"],"properties":{
                    "operation":{"type":"string","enum":["resources","work","inspect","propose"]},
                    "work_id":{"type":["string","null"],"description":"For work: a recorded work ID to inspect, or null to list efforts. Null for all other operations."},
                    "direction":{"type":["string","null"],"description":"Required shared direction for propose: include the task context, constraints and decisions. Omit unrelated private discussion. Set null for read operations."},
                    "plan":{"anyOf":[self.plan_schema,{"type":"null"}],"description":"Complete JSON object for propose, never quoted or stringified JSON; null for read operations."}}}),
        }
    }

    pub(crate) async fn call(&self, request: tetonic_core::SpawnRequest) -> ToolOutcome {
        let Some(attempt) = request.attempt_id else {
            return ToolOutcome::fail("Managed conversation required", "denied");
        };
        if request.tool_name != CONTROL {
            return ToolOutcome::fail("Invalid work plan request", "bad_args");
        }
        let command = match command(request.arguments) {
            Ok(command) => command,
            Err(detail) => return ToolOutcome::fail(detail, "bad_args"),
        };
        let (reply, receive) = tokio::sync::oneshot::channel();
        if self
            .sender
            .send(Call {
                attempt,
                call_id: request.call_id,
                command,
                reply,
            })
            .await
            .is_err()
        {
            return ToolOutcome::fail("The conversation is no longer active", "unavailable");
        }
        receive
            .await
            .unwrap_or_else(|_| ToolOutcome::fail("The conversation stopped", "unavailable"))
    }
}

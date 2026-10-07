//! Conversation planning through the existing managed asynchronous tool boundary.
//! This capability can inspect and propose work; it cannot agree, dispatch or grant.
use serde::Deserialize;
use tetonic_domain::{ToolAdvertisement, ToolOutcome};

pub(crate) const CONTROL: &str = "work_plan";

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Command {
    Inspect {},
    Propose {
        direction: String,
        plan: Box<tetonic_memory::PlanContent>,
    },
}

pub(crate) fn command(mut value: serde_json::Value) -> Result<Command, String> {
    if value.to_string().len() > 60_000 {
        return Err("Keep the proposal concise.".into());
    }
    // A fixed, fully required outer shape is easier for model tool callers.
    // Only null placeholders are omitted for inspection, never unknown fields.
    if value["operation"] == "inspect" {
        if let Some(fields) = value.as_object_mut() {
            for name in ["direction", "plan"] {
                if fields.get(name).is_some_and(serde_json::Value::is_null) {
                    fields.remove(name);
                }
            }
        }
    }
    let parsed: Command = serde_json::from_value(value).map_err(|error| {
        let detail: String = error.to_string().chars().take(240).collect();
        format!("Invalid work_plan arguments: {detail}. Propose requires top-level direction and plan; inspect uses null direction and plan.")
    })?;
    if let Command::Propose { direction, plan } = &parsed {
        if direction.trim().is_empty() || direction.len() > 12_000 || direction.contains('\0') {
            return Err("Provide a concise shared direction of at most 12000 bytes.".into());
        }
        plan.validate().map_err(|e| e.to_string())?;
    }
    Ok(parsed)
}

pub(crate) struct Call {
    pub attempt: String,
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
            description: "Inspect this conversation's saved plan, work and bounded results. To propose, supply BOTH direction and the complete plan. Use each saved agent's exact key from ENGINE OBSERVATION, never its display name. One proposal per reply. Saves a draft only; never dispatches or changes permissions. The owner starts it inline.".into(),
            parameters: serde_json::json!({"type":"object","additionalProperties":false,
                "required":["operation","direction","plan"],"properties":{
                    "operation":{"type":"string","enum":["inspect","propose"]},
                    "direction":{"type":["string","null"],"description":"Required shared direction for propose: include the task context, constraints and decisions. Omit unrelated private discussion. Set null for inspect."},
                    "plan":{"anyOf":[self.plan_schema,{"type":"null"}],"description":"Complete proposal for propose; null for inspect."}}}),
        }
    }

    pub(crate) async fn call(&self, request: tetonic_core::SpawnRequest) -> ToolOutcome {
        let Some(attempt) = request.attempt_id else {
            return ToolOutcome::fail("Managed conversation required", "denied");
        };
        let command = match command(request.arguments) {
            Ok(command) if request.tool_name == CONTROL => command,
            _ => return ToolOutcome::fail("Invalid work plan request", "bad_args"),
        };
        let (reply, receive) = tokio::sync::oneshot::channel();
        if self
            .sender
            .send(Call {
                attempt,
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

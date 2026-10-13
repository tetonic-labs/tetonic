//! Conversation planning through the existing managed asynchronous tool boundary.
//! This capability can inspect, retain discussion notes and propose work; it cannot agree, dispatch or grant.
use serde::Deserialize;
use tetonic_domain::{ToolAdvertisement, ToolOutcome};

pub(crate) const CONTROL: &str = "work_plan";

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Command {
    Context {
        context: Box<crate::work::GuideContextCommand>,
    },
    History {
        #[serde(default)]
        work_id: Option<String>,
        #[serde(default)]
        query: Option<String>,
        #[serde(default)]
        offset: Option<usize>,
    },
    Remember {
        direction: String,
    },
    Rebalance {
        allocations: Vec<Allocation>,
    },
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
    #[serde(skip)]
    RejectedProposal {
        arguments: serde_json::Value,
        reason: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Allocation {
    pub assignment_key: String,
    pub token_budget: u64,
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
    if !matches!(value["operation"].as_str(), Some("work" | "history"))
        && value["work_id"].is_null()
    {
        if let Some(fields) = value.as_object_mut() {
            fields.remove("work_id");
        }
    }
    if value["operation"] != "history" {
        if let Some(fields) = value.as_object_mut() {
            for name in ["query", "offset"] {
                if fields.get(name).is_some_and(serde_json::Value::is_null) {
                    fields.remove(name);
                }
            }
        }
    }
    if value["operation"] == "context" {
        if let Some(fields) = value["context"].as_object_mut() {
            let focus = fields.get("action").and_then(|v| v.as_str()) == Some("focus");
            fields.retain(|key, v| !v.is_null() || focus && key == "subject_id");
        }
    } else if value["context"].is_null() {
        if let Some(fields) = value.as_object_mut() {
            fields.remove("context");
        }
    }
    if value["operation"] != "rebalance" && value["allocations"].is_null() {
        if let Some(fields) = value.as_object_mut() {
            fields.remove("allocations");
        }
    }
    let parsed: Command = serde_json::from_value(value).map_err(|error| {
        let detail: String = error.to_string().chars().take(240).collect();
        format!("Invalid work_plan arguments: {detail}. Remember requires direction and null plan; propose requires direction and plan; reads use null direction and plan. Only work and history accept a non-null work_id.")
    })?;
    if let Command::Remember { direction } = &parsed {
        if direction.trim().is_empty() || direction.len() > 6000 || direction.contains('\0') {
            return Err(
                "Keep the current understanding concise: 1–6000 bytes, without null characters."
                    .into(),
            );
        }
    }
    if let Command::Propose { direction, plan } = &parsed {
        if direction.trim().is_empty() || direction.len() > 12_000 || direction.contains('\0') {
            return Err("Provide a concise shared direction of at most 12000 bytes.".into());
        }
        plan.validate().map_err(|e| e.to_string())?;
    }
    if let Command::Work { work_id: Some(id) }
    | Command::History {
        work_id: Some(id), ..
    } = &parsed
    {
        uuid::Uuid::parse_str(id)
            .map_err(|_| "Use a recorded work_id from the workspace listing".to_string())?;
    }
    if let Command::History { query, offset, .. } = &parsed {
        if query
            .as_ref()
            .is_some_and(|q| q.len() > 500 || q.contains('\0'))
            || offset.is_some_and(|n| n > 1_000_000)
        {
            return Err(
                "Use a search query of at most 500 bytes and a valid returned offset.".into(),
            );
        }
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
    pub read_only: bool,
    pub source: String,
    pub turn: String,
    pub revision: i64,
    pub brief_revision: i64,
    pub plan_schema: serde_json::Value,
    pub sender: tokio::sync::mpsc::Sender<Call>,
}

impl DirectorBinding {
    pub(crate) fn advertisement(&self) -> ToolAdvertisement {
        let mut plan_schema = self.plan_schema.clone();
        plan_schema["type"] = serde_json::json!(["object", "null"]);
        plan_schema["description"] =
            "Complete plan object for propose; null for reads. Never stringify.".into();
        let mut tool = ToolAdvertisement {
            name: CONTROL.into(),
            description: "Guide-only workspace control. resources: inspect current saved agents, teams, skills, connectors, usage and execution ceilings before allocating work. work: list current workspace efforts, or inspect one recorded work_id and its result. inspect: this conversation's current understanding, saved plan and readiness. remember: retain a concise current understanding in direction, with null plan, work_id and allocations. Preserve relevant decisions, corrections and open questions; distinguish tentative ideas from settled choices. This replaces the conversation's notes, so preserve still-relevant context. Save at most once per reply when the understanding materially changes, not for every answer. Saving creates no plan and starts no work; changing direction makes an existing proposal need review. Started plans cannot be changed here. propose: save a real delegation proposal with shared direction and a complete plan using saved agent keys. One saved proposal per reply. rebalance: repair only assignment token budgets on the first rejected parsed proposal; provide allocations for every original assignment key, with null direction, plan and work_id. This retains every other field and the original total automatically. If the engine returns repair_needed, one correction is allowed: use rebalance for token allocations, or propose only for structural repair within this reply's existing budget. Preserve total allowance, shared direction, contributors, dependencies, tools and deliverables. On repair_failed, explain what needs the owner's input and stop proposing; never ask the owner to calculate coordination tokens. Does not dispatch or grant access. The owner starts the reviewed proposal inline.".into(),
            parameters: serde_json::json!({"type":"object","additionalProperties":false,
                "required":["operation","direction","plan","work_id","allocations","query","offset","context"],"properties":{
                    "operation":{"type":"string","enum":["resources","work","inspect","remember","propose","rebalance","history","context"]},
                    "context": context_schema(),
                    "query":{"type":["string","null"],"description":"For history: search original messages in this authorized discussion. Null lists recent message excerpts. Use returned IDs to read evidence, not guesses. Null for other operations."},
                    "offset":{"type":["integer","null"],"description":"For history: returned next_offset for another page of matches or characters of a specific message. Null starts at zero. Null for other operations."},
                    "work_id":{"type":["string","null"],"description":"For work: a recorded work ID to inspect, or null to list efforts. For history: an original message ID in this conversation or a saved subject's source_turns; null searches/lists this conversation. Null otherwise."},
                    "direction":{"type":["string","null"],"description":"For remember: the complete concise working understanding, retaining relevant decisions and open questions. For propose: shared task context, constraints and decisions, omitting unrelated private discussion. Null for reads."},
                    "allocations":{"type":["array","null"],"description":"Only for rebalance: one allocation per original assignment key. Null otherwise.","items":{"type":"object","additionalProperties":false,"required":["assignment_key","token_budget"],"properties":{"assignment_key":{"type":"string"},"token_budget":{"type":"integer"}}}},
                    "plan":plan_schema}}),
        };
        tool.description.push_str(" history retrieves original discussion evidence on demand. context maintains optional source-backed subjects across discussions; it never changes execution targets or grants.");
        if self.read_only {
            for field in ["plan", "direction", "allocations"] {
                tool.parameters["properties"][field] = serde_json::json!({"type":"null"});
            }
            tool.description="Background Guide inspection only. work reads current efforts or a specific recorded work_id and its evidence. resources reads current resource availability. inspect reads the current understanding. history reads authorized source messages. context list/read retrieves saved subjects. Report implications concisely; do not change work or discussion.".into();
            tool.parameters["properties"]["operation"]["enum"] =
                serde_json::json!(["resources", "work", "inspect", "history", "context"]);
            tool.parameters["properties"]["context"]["properties"]["action"]["enum"] =
                serde_json::json!(["list", "read"]);
        }
        tool.description.push_str(" This managed host capability accepts one call per model response, never parallel calls in the same batch. Inspect only the evidence needed to answer; work with null work_id returns an overview, and an effort's work_id includes its recorded result and usage.");
        tool
    }

    pub(crate) async fn call(&self, request: tetonic_core::SpawnRequest) -> ToolOutcome {
        let Some(attempt) = request.attempt_id else {
            return ToolOutcome::fail("Managed conversation required", "denied");
        };
        if request.tool_name != CONTROL {
            return ToolOutcome::fail("Invalid work plan request", "bad_args");
        }
        let command = match command(request.arguments.clone()) {
            Ok(command) => command,
            Err(reason)
                if matches!(
                    request.arguments["operation"].as_str(),
                    Some("propose" | "rebalance")
                ) =>
            {
                // Malformed proposals consume the same durable repair allowance
                // as allocation failures; successful reads cannot reset it.
                let arguments = if request.arguments.to_string().len() <= 60_000 {
                    request.arguments
                } else {
                    use sha2::Digest;
                    serde_json::json!({"oversized_digest":format!("{:x}", sha2::Sha256::digest(request.arguments.to_string()))})
                };
                Command::RejectedProposal { arguments, reason }
            }
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

fn context_schema() -> serde_json::Value {
    serde_json::json!({"type":["object","null"],"additionalProperties":false,
    "description":"Only for context: follow distinct subjects across discussions. list finds saved subjects; read retrieves one; save creates/revises notes with source references; focus changes relevance in this conversation only. No work or permission changes. Unused fields null. Creating a subject is optional, not needed for every message.",
    "required":["action","subject_id","expected_revision","subject","offset"],
    "properties":{
        "action":{"type":"string","enum":["list","read","save","focus"]},
        "subject_id":{"type":["string","null"],"description":"Read/focus: existing subject ID. Null focus clears association."},
        "expected_revision":{"type":["integer","null"],"description":"Focus: current focus revision from discussion_context; zero initially."},
        "offset":{"type":["integer","null"],"description":"List: next_offset from previous page."},
        "subject":{"type":["object","null"],"additionalProperties":false,"required":["id","expected_revision","title","understanding","source_turns","work_ids"],"properties":{
            "id":{"type":"string","description":"Existing subject ID to revise or new UUID to create."},"expected_revision":{"type":"integer","description":"Current subject revision or zero for a new subject."},
            "title":{"type":"string"},"understanding":{"type":"string","description":"Concise current understanding, preserving corrections, uncertainty and rejected alternatives. At most 6000 bytes. Never approval."},
            "source_turns":{"type":"array","items":{"type":"string"},"description":"1–64 original message IDs providing evidence; include this turn when updating. Do not copy entire conversations."},
            "work_ids":{"type":"array","items":{"type":"string"},"description":"Existing related effort IDs, not execution targets or new work."}
        }}
    }})
}

//! Host-bound plan tool on the existing asynchronous spawn boundary. The model
//! selects an assignment key; it cannot supply agents, grants, budgets or input.
use std::sync::{Arc, Mutex};
use tetonic_domain::work_scope::CancellationSignal;
use tetonic_domain::{AuthorizedAction, ToolAdvertisement, ToolHost, ToolOutcome, ToolProposal};

pub(crate) const DISPATCH: &str = "dispatch_assignment";
pub(crate) const ASK_HUMAN: &str = "ask_human";

#[derive(Clone)]
pub(crate) struct HumanHandoff {
    pub store: tetonic_memory::SharedStore,
    pub actor: String,
    pub org: String,
    pub team: String,
    pub work: String,
}
impl HumanHandoff {
    async fn ask(&self, request: tetonic_core::SpawnRequest) -> ToolOutcome {
        use sha2::Digest;
        let Some(attempt) = request.attempt_id else {
            return ToolOutcome::fail("Managed attempt required", "denied");
        };
        let Ok(content) =
            serde_json::from_value::<tetonic_memory::HumanQuestionContent>(request.arguments)
        else {
            return ToolOutcome::fail(
                "Provide a question, why it matters, and optional choices",
                "bad_args",
            );
        };
        let id = format!(
            "{:x}",
            sha2::Sha256::digest(format!("{attempt}:{}", request.call_id).as_bytes())
        );
        let h = self.clone();
        let question_id = id.clone();
        let saved = self
            .store
            .write(move |db| {
                db.ask_work_human(tetonic_memory::AskWorkHuman {
                    actor: &h.actor,
                    org: &h.org,
                    team: &h.team,
                    work: &h.work,
                    attempt: &attempt,
                    id: &question_id,
                    content,
                    now: chrono::Utc::now().timestamp() as u64,
                })
            })
            .await;
        let Ok(Ok(saved)) = saved else {
            return ToolOutcome::fail("A human request could not be recorded. The attempt must be active; at most two questions are allowed for this assignment.","denied");
        };
        loop {
            let h = self.clone();
            let id = id.clone();
            let row = self
                .store
                .read(move |db| {
                    db.work_human_questions(&h.actor, &h.org, &h.team, &h.work)
                        .map(|rows| rows.into_iter().find(|r| r.id == id))
                })
                .await;
            if let Ok(Ok(Some(row))) = row {
                if let Some(answer) = row.answer {
                    return ToolOutcome::ok("The owner answered",serde_json::json!({"question_id":row.id,"answer":answer,"authority":"Task guidance only. Existing permissions and limits still apply."}).to_string());
                }
            } else {
                return ToolOutcome::fail("Human request could not be read", "unavailable");
            }
            if chrono::Utc::now().timestamp() as u64 >= saved.deadline {
                return ToolOutcome::fail(
                    "The time limit expired while waiting for the owner",
                    "expired",
                );
            }
            // No inference, model mutex, database transaction or blocking thread is held.
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }
}
pub(crate) struct DispatchCall {
    pub key: String,
    pub attempt: String,
    pub reply: tokio::sync::oneshot::Sender<ToolOutcome>,
}

#[derive(Clone)]
pub struct PlanDispatch {
    pub(crate) human: Option<HumanHandoff>,
    pub(crate) binding: String,
    pub(crate) sender: tokio::sync::mpsc::Sender<DispatchCall>,
    pub(crate) assignment_keys: Vec<String>,
    pub(crate) remaining: Arc<Mutex<std::collections::HashSet<String>>>,
}

impl PlanDispatch {
    pub(crate) fn hook(&self) -> tetonic_core::SpawnHook {
        let sender = self.sender.clone();
        let human = self.human.clone();
        Box::new(move |request, _| {
            let sender = sender.clone();
            let human = human.clone();
            Box::pin(async move {
                if request.tool_name == ASK_HUMAN {
                    return match human {
                        Some(h) => h.ask(request).await,
                        None => ToolOutcome::fail("Human handoff unavailable", "denied"),
                    };
                }
                let Some(attempt) = request.attempt_id else {
                    return ToolOutcome::fail("Managed parent required", "denied");
                };
                let Ok(args) = serde_json::from_value::<DispatchArgs>(request.arguments) else {
                    return ToolOutcome::fail(
                        "Use an assignment key from the agreed plan",
                        "bad_args",
                    );
                };
                let (reply, receiver) = tokio::sync::oneshot::channel();
                if sender
                    .send(DispatchCall {
                        key: args.assignment_key,
                        attempt,
                        reply,
                    })
                    .await
                    .is_err()
                {
                    return ToolOutcome::fail("The plan dispatcher is unavailable", "unavailable");
                }
                receiver.await.unwrap_or_else(|_| {
                    ToolOutcome::fail("The plan dispatcher stopped", "unavailable")
                })
            })
        })
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DispatchArgs {
    assignment_key: String,
}

#[derive(Clone)]
pub(crate) struct RegisteredToolHost {
    pub tools: tetonic_tools::Tools,
    pub dispatch: Option<PlanDispatch>,
}

impl ToolHost for RegisteredToolHost {
    fn clone_box(&self) -> Box<dyn ToolHost> {
        Box::new(self.clone())
    }
    fn propose(&self, name: &str, args: &serde_json::Value) -> Option<ToolProposal> {
        self.tools.propose(name, args)
    }
    fn is_tool_allowed(&self, name: &str) -> bool {
        if name == DISPATCH {
            self.dispatch
                .as_ref()
                .is_some_and(|d| !d.assignment_keys.is_empty())
        } else if name == ASK_HUMAN {
            self.dispatch.as_ref().is_some_and(|d| d.human.is_some())
        } else {
            self.tools.is_tool_allowed(name)
        }
    }
    fn is_read_only(&self, name: &str) -> bool {
        name != DISPATCH && name != ASK_HUMAN && self.tools.is_read_only(name)
    }
    fn requires_action_broker(&self, name: &str) -> bool {
        self.tools.requires_action_broker(name)
    }
    fn requires_user_approval(&self, name: &str) -> bool {
        self.tools.requires_user_approval(name)
    }
    fn advertisements(&self) -> Vec<ToolAdvertisement> {
        let mut ads = self.tools.advertisements();
        if self
            .dispatch
            .as_ref()
            .is_some_and(|d| !d.assignment_keys.is_empty())
        {
            // Advertisements repeat on every inference call. Keep the plan
            // completion instruction concise; validation still owns readiness.
            if let Some(finish) = ads.iter_mut().find(|ad| ad.name == "finish") {
                finish.description = "Return the final synthesis in summary after all contributions arrive. Preserve source citations and the requested length.".into();
            }
            ads.push(ToolAdvertisement {
            name:DISPATCH.into(),
            description:"Dispatch a ready key; retries reuse work. Returns its result or waiting_human, also_completed, and outstanding_assignments.".into(),
            parameters:serde_json::json!({"type":"object","additionalProperties":false,"required":["assignment_key"],"properties":{"assignment_key":{"type":"string","description":"Use the key, not an assignment object.","enum":self.dispatch.as_ref().unwrap().assignment_keys}}}),
        });
        }
        if self.dispatch.as_ref().is_some_and(|d| d.human.is_some()) {
            ads.push(ToolAdvertisement {name:ASK_HUMAN.into(),description:"Ask a blocking question; waits within this attempt's deadline. Answers do not change permissions or budget.".into(),parameters:serde_json::json!({"type":"object","additionalProperties":false,"required":["question","why"],"properties":{"question":{"type":"string","maxLength":800},"why":{"type":"string","maxLength":1200},"options":{"type":"array","maxItems":4,"items":{"type":"string","maxLength":300}}}})});
        }
        ads
    }
    fn validate_tool_args(&self, name: &str, args: &serde_json::Value) -> Result<(), String> {
        if name == ASK_HUMAN {
            if !self.is_tool_allowed(name) {
                return Err("Human handoff unavailable".into());
            }
            return serde_json::from_value::<tetonic_memory::HumanQuestionContent>(args.clone())
                .map_err(|_| "Invalid human question".to_string())?
                .validate()
                .map_err(|_| "Keep the question, reason and choices concise".into());
        }
        if name == DISPATCH {
            if self.dispatch.is_none() {
                return Err("Plan dispatch is unavailable".into());
            }
            let input: DispatchArgs =
                serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
            if !self
                .dispatch
                .as_ref()
                .unwrap()
                .assignment_keys
                .contains(&input.assignment_key)
            {
                return Err(format!("Use only an assignment_key from: {}. Pass the short key, not an object or instructions.", self.dispatch.as_ref().unwrap().assignment_keys.join(", ")));
            }
            return Ok(());
        }
        if name == "finish" {
            if let Some(dispatch) = &self.dispatch {
                let remaining = dispatch
                    .remaining
                    .lock()
                    .map_err(|_| "Cannot confirm plan completion")?;
                if !remaining.is_empty() {
                    let mut keys: Vec<_> = remaining.iter().cloned().collect();
                    keys.sort();
                    return Err(format!("The plan still needs contributions from: {}. Dispatch ready assignments before finishing. Never claim uncompleted work is done.",keys.join(", ")));
                }
            }
        }
        self.tools.validate_tool_args(name, args)
    }
    fn execute_authorized(
        &self,
        name: &str,
        args: &serde_json::Value,
        auth: Option<&AuthorizedAction>,
        cancel: &CancellationSignal,
    ) -> ToolOutcome {
        if name == DISPATCH || name == ASK_HUMAN {
            return ToolOutcome::fail("Dispatch requires a managed asynchronous parent", "denied");
        }
        ToolHost::execute_authorized(&self.tools, name, args, auth, cancel)
    }
}

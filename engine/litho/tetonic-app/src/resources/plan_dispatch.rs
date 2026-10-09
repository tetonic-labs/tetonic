//! Host-bound plan tool on the existing asynchronous spawn boundary. The model
//! selects agreed assignment keys; it cannot supply agents, grants, budgets or input.
use std::sync::{Arc, Mutex};
use tetonic_domain::work_scope::CancellationSignal;
use tetonic_domain::{AuthorizedAction, ToolAdvertisement, ToolHost, ToolOutcome, ToolProposal};

mod group;

pub(crate) const DISPATCH: &str = "dispatch_assignment";
pub(crate) const ASK_HUMAN: &str = "ask_human";

#[derive(Clone)]
pub(crate) struct HumanHandoff {
    pub store: tetonic_memory::SharedStore,
    pub actor: String,
    pub org: String,
    pub team: String,
    pub work: String,
    /// Explicit host opt-in; team controllers still use live handoffs.
    pub durable_wait_seconds: Option<u64>,
    pub prepared_stop_binding: Option<String>,
}
impl HumanHandoff {
    async fn ask(
        &self,
        manager: &tetonic_run::managed::ManagedRunService,
        request: tetonic_core::SpawnRequest,
    ) -> ToolOutcome {
        use sha2::Digest;
        let suspended = if let Some(max_wait_seconds) = self.durable_wait_seconds {
            match manager.verify_human_handoff(&request).await {
                Ok(checkpoint) => Some(tetonic_memory::SuspendedHumanQuestion {
                    stop_binding: self.prepared_stop_binding.clone().unwrap_or_default(),
                    checkpoint,
                    call_id: request.call_id.clone(),
                    max_wait_seconds,
                }),
                Err(_) => {
                    return ToolOutcome::fail("Saved handoff could not be verified", "denied")
                }
            }
        } else {
            None
        };
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
                let command = tetonic_memory::AskWorkHuman {
                    actor: &h.actor,
                    org: &h.org,
                    team: &h.team,
                    work: &h.work,
                    attempt: &attempt,
                    id: &question_id,
                    content,
                    now: chrono::Utc::now().timestamp() as u64,
                };
                match suspended {
                    Some(proof) => db.ask_suspended_work_human(command, proof),
                    None => db.ask_work_human(command),
                }
            })
            .await;
        let Ok(Ok(_saved)) = saved else {
            return ToolOutcome::fail("A human request could not be recorded. The work must still be eligible; at most two questions are allowed for this assignment.","denied");
        };
        loop {
            let h = self.clone();
            let id = id.clone();
            let row = self
                .store
                .read(move |db| {
                    db.pending_work_human_question(
                        &h.actor,
                        &h.org,
                        &h.team,
                        &h.work,
                        &id,
                        chrono::Utc::now().timestamp() as u64,
                    )
                })
                .await;
            if let Ok(Ok(row)) = row {
                if let Some(answer) = row.answer {
                    return ToolOutcome::ok("The owner answered",serde_json::json!({"question_id":row.id,"answer":answer,"authority":"Task guidance only. Existing permissions and limits still apply."}).to_string());
                }
            } else {
                return ToolOutcome::fail("Human request could not be read", "unavailable");
            }
            // No inference, model mutex, database transaction or blocking thread is held.
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }
}
pub(crate) struct DispatchCall {
    pub checkpoint: Box<tetonic_core::WaitCheckpoint>,
    pub call_id: String,
    pub keys: Vec<String>,
    pub grouped: bool,
    pub attempt: String,
    pub reply: tokio::sync::oneshot::Sender<ToolOutcome>,
}

#[derive(Clone)]
pub struct PlanDispatch {
    pub(crate) director: Option<super::work_director::DirectorBinding>,
    pub(crate) human: Option<HumanHandoff>,
    pub(crate) binding: String,
    pub(crate) sender: tokio::sync::mpsc::Sender<DispatchCall>,
    pub(crate) assignment_keys: Vec<String>,
    pub(crate) remaining: Arc<Mutex<std::collections::HashSet<String>>>,
}

impl PlanDispatch {
    pub(crate) fn hook(
        &self,
        manager: Arc<tetonic_run::managed::ManagedRunService>,
    ) -> tetonic_core::SpawnHook {
        let dispatch = self.clone();
        Box::new(move |request, conversation| {
            let checkpoint = conversation.host_checkpoint().cloned();
            let dispatch = dispatch.clone();
            let manager = manager.clone();
            Box::pin(async move {
                if request.tool_name == super::work_director::CONTROL {
                    return match &dispatch.director {
                        Some(director) => director.call(request).await,
                        None => ToolOutcome::fail("Conversation planning unavailable", "denied"),
                    };
                }
                if request.tool_name == ASK_HUMAN {
                    return match dispatch.human.clone() {
                        Some(h) => h.ask(&manager, request).await,
                        None => ToolOutcome::fail("Human handoff unavailable", "denied"),
                    };
                }
                group::dispatch(&dispatch, request, checkpoint).await
            })
        })
    }
}

#[derive(Clone)]
pub(crate) struct RegisteredToolHost {
    pub tools: tetonic_tools::Tools,
    pub dispatch: Option<PlanDispatch>,
}

impl ToolHost for RegisteredToolHost {
    fn checkpoint_ready(&self) -> bool {
        // Dispatch checkpoints do not yet quiesce/restore child executors. The
        // director also has its own turn state. Neither can opt into root waits.
        self.dispatch
            .as_ref()
            .is_none_or(|d| d.assignment_keys.is_empty() && d.director.is_none())
            && self.tools.checkpoint_ready()
    }

    fn clone_box(&self) -> Box<dyn ToolHost> {
        Box::new(self.clone())
    }
    fn propose(&self, name: &str, args: &serde_json::Value) -> Option<ToolProposal> {
        self.tools.propose(name, args)
    }
    fn is_tool_allowed(&self, name: &str) -> bool {
        if name == super::work_director::CONTROL {
            self.dispatch.as_ref().is_some_and(|d| d.director.is_some())
        } else if name == DISPATCH {
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
        name != DISPATCH
            && name != ASK_HUMAN
            && name != super::work_director::CONTROL
            && self.tools.is_read_only(name)
    }
    fn requires_action_broker(&self, name: &str) -> bool {
        self.tools.requires_action_broker(name)
    }
    fn requires_user_approval(&self, name: &str) -> bool {
        self.tools.requires_user_approval(name)
    }
    fn advertisements(&self) -> Vec<ToolAdvertisement> {
        let mut ads = self.tools.advertisements();
        if let Some(director) = self.dispatch.as_ref().and_then(|d| d.director.as_ref()) {
            ads.push(director.advertisement());
        }
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
            description:"Dispatch agreed assignment_keys together. Independent agents run concurrently; dependencies and each agent's capacity are respected. Dependents receive completed results. Completed-key retries reuse results.".into(),
            parameters:serde_json::json!({"type":"object","additionalProperties":false,"required":["assignment_keys"],"properties":{"assignment_keys":{"type":"array","minItems":1,"maxItems":12,"uniqueItems":true,"items":{"type":"string","enum":self.dispatch.as_ref().unwrap().assignment_keys}}}}),
        });
        }
        if self.dispatch.as_ref().is_some_and(|d| d.human.is_some()) {
            ads.push(ToolAdvertisement {name:ASK_HUMAN.into(),description:"Ask a blocking question within the host's response limit. Answers do not change permissions or budget.".into(),parameters:serde_json::json!({"type":"object","additionalProperties":false,"required":["question","why"],"properties":{"question":{"type":"string","maxLength":800},"why":{"type":"string","maxLength":1200},"options":{"type":"array","maxItems":4,"items":{"type":"string","maxLength":300}}}})});
        }
        ads
    }
    fn validate_tool_args(&self, name: &str, args: &serde_json::Value) -> Result<(), String> {
        if name == super::work_director::CONTROL {
            if !self.is_tool_allowed(name) {
                return Err("Conversation planning unavailable".into());
            }
            return super::work_director::command(args.clone()).map(|_| ());
        }
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
            let dispatch = self
                .dispatch
                .as_ref()
                .ok_or("Plan dispatch is unavailable")?;
            return group::keys(args.clone(), &dispatch.assignment_keys).map(|_| ());
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
        if name == DISPATCH || name == ASK_HUMAN || name == super::work_director::CONTROL {
            return ToolOutcome::fail("Dispatch requires a managed asynchronous parent", "denied");
        }
        ToolHost::execute_authorized(&self.tools, name, args, auth, cancel)
    }
}

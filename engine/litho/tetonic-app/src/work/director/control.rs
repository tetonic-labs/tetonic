//! Scoped proposal writes reuse the existing brief and huddle services. No launch authority.
use super::*;
use crate::resources::{
    plan_dispatch::PlanDispatch,
    work_director::{Call, Command, DirectorBinding},
    GuideProposalDraft, RegisteredAgentExecution,
};
use std::sync::{Arc, Mutex};
use tetonic_domain::ToolOutcome;
#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;

pub(crate) struct Session {
    binding: DirectorBinding,
    receiver: tokio::sync::mpsc::Receiver<Call>,
}

fn operation_id(turn: &str, kind: &str) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(format!("{turn}/{kind}"));
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    uuid::Uuid::from_bytes(bytes).to_string()
}

fn proposal_receipt(plan: &tetonic_memory::HuddlePlan) -> serde_json::Value {
    // A successful write must not become a reported failure if a subsequent
    // snapshot is unavailable or too large. Inspection is a separate read.
    serde_json::json!({
        "saved": true, "revision": plan.revision, "brief_revision": plan.brief_revision,
        "status": plan.status, "work_id": plan.work_id,
        "title": plan.content.as_ref().map(|p| &p.title),
        "allowance": plan.content.as_ref().map(|p| {
            let workers: u64 = p.assignments.iter().map(|a| a.token_budget).sum();
            serde_json::json!({"total_tokens":p.token_budget,"worker_tokens":workers,"coordination_tokens":p.token_budget.saturating_sub(workers),"reserved":false})
        }),
        "work_launched": false,
        "next": "The proposal is available inline for review. The owner can use Start this plan; use inspect to check readiness."
    })
}

fn resolve_agents(
    content: &mut tetonic_memory::PlanContent,
    agents: &[LocalAgent],
) -> Result<(), AppError> {
    let workers: Vec<_> = agents
        .iter()
        .filter(|a| a.key != shaping::GUIDE && !a.plan_coordinator)
        .collect();
    for assignment in &mut content.assignments {
        if workers.iter().any(|a| a.key == assignment.agent_key) {
            continue;
        }
        let matches: Vec<_> = workers
            .iter()
            .filter(|a| a.name == assignment.agent_key)
            .collect();
        match matches.as_slice() {
            [agent] => assignment.agent_key = agent.key.clone(),
            [] => {
                return Err(AppError::InvalidRequest(format!(
                    "{} is not an available working agent. Use a saved agent key.",
                    assignment.agent_key
                )))
            }
            _ => {
                return Err(AppError::InvalidRequest(format!(
                    "More than one agent is named {}. Use the exact saved key to choose one.",
                    assignment.agent_key
                )))
            }
        }
    }
    Ok(())
}

impl WorkService {
    pub(crate) async fn bind_director(
        &self,
        turn: &str,
    ) -> Result<(PlanDispatch, Session), AppError> {
        let snapshot = self.snapshot().await?;
        let source = conversation_root(&snapshot, turn).to_string();
        let view = self.plan_view(&source).await?;
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        let mut plan_schema = plans::response_schema();
        let agents = self.planning_agents(&source).await?;
        let agent_keys: Vec<_> = agents
            .iter()
            .filter(|a| a.key != shaping::GUIDE && !a.plan_coordinator)
            .map(|a| &a.key)
            .collect();
        if !agent_keys.is_empty() {
            plan_schema["properties"]["assignments"]["items"]["properties"]["agent_key"]["enum"] =
                serde_json::json!(agent_keys);
        }
        let binding = DirectorBinding {
            source,
            turn: turn.into(),
            revision: view.plans.first().map_or(0, |p| p.revision),
            brief_revision: view.brief_revision,
            plan_schema,
            sender,
        };
        let (unused_sender, _) = tokio::sync::mpsc::channel(1);
        Ok((
            PlanDispatch {
                director: Some(binding.clone()),
                human: None,
                binding: turn.into(),
                sender: unused_sender,
                assignment_keys: vec![],
                remaining: Arc::new(Mutex::new(Default::default())),
            },
            Session { binding, receiver },
        ))
    }

    pub(crate) fn serve_director(&self, execution: RegisteredAgentExecution, mut session: Session) {
        // Clones share the same store, runtime and admission lock, not another authority.
        let workspace = self.clone();
        tokio::task::spawn_local(async move {
            let mut completion = execution.completion;
            loop {
                tokio::select! { biased;
                    _ = &mut completion => break,
                    call = session.receiver.recv() => {
                        let Some(call) = call else { break; };
                        if call.attempt != execution.attempt_id.0 || call.reply.is_closed() {
                            let _ = call.reply.send(ToolOutcome::fail("The conversation is no longer active", "denied"));
                            continue;
                        }
                        let outcome = workspace.director_command(&session.binding, &call.call_id, call.command).await;
                        let _ = call.reply.send(match outcome {
                            Ok(value) if value["proposal_state"] == "repair_needed" || value["proposal_state"] == "repair_failed" => {
                                let mut result = ToolOutcome::fail("The proposal needs correction before review", "work_plan_rejected");
                                result.content = value.to_string();
                                result
                            }
                            Ok(value) => ToolOutcome::ok("Recorded work state", value.to_string()),
                            Err(error) => ToolOutcome::fail(error.employee_message(), "work_plan_rejected"),
                        });
                    }
                }
            }
        });
    }

    async fn director_command(
        &self,
        binding: &DirectorBinding,
        call_id: &str,
        command: Command,
    ) -> Result<serde_json::Value, AppError> {
        let _admission = self.services.admission.lock().await;
        let turn = self.task(&binding.turn).await?;
        if turn.agent_key != shaping::GUIDE
            || turn.purpose != WorkPurpose::Explore
            || turn.state != "running"
        {
            return Err(AppError::PolicyDenied(
                "Only this active Guide reply can change its proposal.".into(),
            ));
        }
        match command {
            Command::Inspect {} => self.director_state(&binding.source).await,
            Command::Resources {} => self.director_resources(&binding.source).await,
            Command::Work { work_id } => self.director_work(work_id.as_deref()).await,
            Command::Propose { direction, plan } => {
                self.director_propose_call(binding, call_id, direction, *plan)
                    .await
            }
        }
    }

    async fn director_propose_call(
        &self,
        binding: &DirectorBinding,
        call_id: &str,
        direction: String,
        mut content: tetonic_memory::PlanContent,
    ) -> Result<serde_json::Value, AppError> {
        // Human names are accepted only when they resolve uniquely inside this
        // authorized roster. Persist canonical identities, never fuzzy matches.
        resolve_agents(&mut content, &self.planning_agents(&binding.source).await?)?;
        let view = self.plan_view(&binding.source).await?;
        if view.execution.is_some() {
            return Err(AppError::InvalidRequest("This plan has already started. Inspect its work; use its upcoming-assignment controls to change active work.".into()));
        }
        let request = operation_id(&binding.turn, "proposal");
        if let Some(existing) = view.plans.iter().find(|p| p.request_id == request) {
            let briefs = self.work_briefs(&binding.source).await?;
            if existing.content.as_ref() == Some(&content)
                && briefs
                    .iter()
                    .any(|b| b.revision == existing.brief_revision && b.body == direction)
            {
                return Ok(proposal_receipt(existing));
            }
            return Err(AppError::InvalidRequest("One proposal per reply. The saved proposal is unchanged; finish your response before further edits.".into()));
        }
        if view.plans.first().map_or(0, |p| p.revision) != binding.revision
            || view.brief_revision != binding.brief_revision
        {
            return Err(AppError::InvalidRequest("The owner changed the plan or direction during this reply. Inspect the current version and explain the conflict; do not overwrite their changes.".into()));
        }
        let validation_error = match self.validate_plan_proposal(&binding.source, &content).await {
            Ok(()) => None,
            Err(AppError::InvalidRequest(reason)) => Some(reason),
            Err(error) => return Err(error),
        };
        let scope = self.services.authorized_scope().await?;
        let outcome = self.services.local.resources().submit_guide_proposal(
            &self.services.host.credential, scope.organization().into(), scope.team().into(),
            GuideProposalDraft {
                source: binding.source.clone(), turn: binding.turn.clone(), call: call_id.into(), request,
                expected_plan: binding.revision, expected_brief: binding.brief_revision,
                generation_id: operation_id(&binding.turn, "proposal-origin"),
                generation_input: format!("Guide conversation turn {}. Shared direction is stored in the linked brief revision.", binding.turn),
                direction, content, validation_error,
            },
        ).await.map_err(resource)?;
        Ok(match outcome {
            tetonic_memory::GuideProposalOutcome::Saved { plan } => proposal_receipt(&plan),
            tetonic_memory::GuideProposalOutcome::RepairNeeded { reason } => serde_json::json!({
                "saved":false,"work_launched":false,"proposal_state":"repair_needed","reason":reason,"repair_attempts_remaining":1
            }),
            tetonic_memory::GuideProposalOutcome::RepairFailed { reason } => serde_json::json!({
                "saved":false,"work_launched":false,"proposal_state":"repair_failed","reason":reason,"repair_attempts_remaining":0
            }),
        })
    }

    #[cfg(test)]
    async fn director_propose(
        &self,
        binding: &DirectorBinding,
        direction: String,
        content: tetonic_memory::PlanContent,
    ) -> Result<serde_json::Value, AppError> {
        let result = self
            .director_propose_call(
                binding,
                &uuid::Uuid::new_v4().to_string(),
                direction,
                content,
            )
            .await?;
        if result["saved"] == false {
            return Err(AppError::InvalidRequest(
                result["reason"].as_str().unwrap().into(),
            ));
        }
        Ok(result)
    }

    pub(crate) async fn director_state(&self, source: &str) -> Result<serde_json::Value, AppError> {
        let app_scope = self.services.authorized_scope().await?;
        let view = self.plan_view(source).await?;
        // The turn already has a bounded workspace observation. Inspect adds
        // this conversation's details, not another copy of unrelated work.
        let mut value = serde_json::json!({
            "observed_at": chrono::Utc::now().to_rfc3339(),
            "conversation_id": source,
            "scope": {"organization": app_scope.organization(), "team": app_scope.team()},
        });
        value["plan"] = serde_json::json!(view.plans.first().map(|p| serde_json::json!({
            "revision":p.revision,"brief_revision":p.brief_revision,"status":p.status,"content":p.content
        })));
        value["readiness"] = serde_json::json!(view.readiness);
        value["execution_state"] = serde_json::json!(view.execution.as_ref().map(|e| &e.state));
        value["results"] = serde_json::json!(view.execution.as_ref().map(|e| {
            e.root.iter().chain(e.assignments.iter()).take(13).map(|t| serde_json::json!({
                "work_id": t.id, "agent":t.agent_name, "state":t.state,
                "text":t.messages.iter().rev().find(|m| m.role=="assistant").map(|m| short(&m.content, 1200)),
                "error":t.error,
            })).collect::<Vec<_>>()
        }));
        value["authority"] = "Draft proposals only. No work is launched, no permissions or budgets are changed. The owner starts the reviewed plan using Start this plan. Result excerpts can be truncated; do not infer omitted evidence.".into();
        if value.to_string().len() > 36_000 {
            return Err(AppError::InvalidRequest("This plan is too large for a conversational snapshot. Open the plan to inspect its full details.".into()));
        }
        Ok(value)
    }
}

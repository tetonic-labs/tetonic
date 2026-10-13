//! Scoped proposal writes reuse the existing brief and huddle services. No launch authority.
use super::*;
use crate::resources::{
    plan_dispatch::PlanDispatch,
    work_director::{Call, Command, DirectorBinding},
    GuideProposalDraft, RegisteredAgentExecution,
};
use std::sync::{Arc, Mutex};
use tetonic_domain::ToolOutcome;
mod allocations;
#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;

pub(crate) struct Session {
    binding: DirectorBinding,
    receiver: tokio::sync::mpsc::Receiver<Call>,
}

pub(super) fn operation_id(turn: &str, kind: &str) -> String {
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
        let coordination = self.services.execution.coordination_tokens();
        plan_schema["properties"]["token_budget"]["description"] = format!(
            "Total reported input AND output tokens across every worker call and coordinator call, not answer length. Calculate worker allocations first, then include up to {coordination} tokens for coordination inside this total. Prefer that coordination allowance when the owner's limit permits. Never exceed the owner's limit. A smaller total must still fund coordination and every assignment."
        ).into();
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
            read_only: self.guide_assessment_allowance(turn).await?.is_some(),
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
        let read_only = match &command {
            Command::Inspect {}
            | Command::Resources {}
            | Command::Work { .. }
            | Command::History { .. } => true,
            Command::Context { context } => matches!(
                context.as_ref(),
                super::subjects::GuideContextCommand::List { .. }
                    | super::subjects::GuideContextCommand::Read { .. }
            ),
            _ => false,
        };
        if turn.guide_background && !read_only {
            return Err(AppError::PolicyDenied("Background attention can inspect and explain; it cannot change discussion, proposals or work.".into()));
        }
        match command {
            Command::Context { context } => {
                if let super::subjects::GuideContextCommand::Save { subject } = context.as_ref() {
                    if !subject.source_turns.contains(&binding.turn) {
                        return Err(AppError::InvalidRequest(
                            "Include the current turn ID as evidence for this understanding."
                                .into(),
                        ));
                    }
                }
                self.guide_context_command(
                    &binding.source,
                    super::subjects::GuideContextMutation {
                        request_id: operation_id(&binding.turn, &format!("context/{call_id}")),
                        command: *context,
                    },
                )
                .await
            }
            Command::History {
                work_id,
                query,
                offset,
            } => {
                self.director_history(
                    binding,
                    work_id.as_deref(),
                    query.as_deref(),
                    offset.unwrap_or(0),
                )
                .await
            }
            Command::Remember { direction } => {
                self.remember_understanding(binding, direction).await
            }
            Command::Rebalance { allocations } => {
                self.director_rebalance(binding, call_id, allocations).await
            }
            Command::Inspect {} => self.director_state(&binding.source).await,
            Command::Resources {} => self.director_resources(&binding.source).await,
            Command::Work { work_id } => self.director_work(work_id.as_deref()).await,
            Command::Propose { direction, plan } => {
                self.director_propose_call(binding, call_id, direction, *plan)
                    .await
            }
            Command::RejectedProposal { arguments, reason } => {
                let direction = arguments["direction"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                self.record_proposal(
                    binding,
                    call_id,
                    direction,
                    None,
                    Some(arguments),
                    Some(reason),
                )
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
        let validation_error =
            match resolve_agents(&mut content, &self.planning_agents(&binding.source).await?) {
                Ok(()) => match self
                    .validate_plan_with_direction(&binding.source, &direction, &content)
                    .await
                {
                    Ok(()) => None,
                    Err(AppError::InvalidRequest(reason)) => Some(reason),
                    Err(error) => return Err(error),
                },
                Err(AppError::InvalidRequest(reason)) => Some(reason),
                Err(error) => return Err(error),
            };
        self.record_proposal(
            binding,
            call_id,
            direction,
            Some(content),
            None,
            validation_error,
        )
        .await
    }

    async fn record_proposal(
        &self,
        binding: &DirectorBinding,
        call_id: &str,
        direction: String,
        content: Option<tetonic_memory::PlanContent>,
        rejected_arguments: Option<serde_json::Value>,
        validation_error: Option<String>,
    ) -> Result<serde_json::Value, AppError> {
        let view = self.plan_view(&binding.source).await?;
        if view.execution.is_some() {
            return Err(AppError::InvalidRequest("This plan has already started. Inspect its work; use its upcoming-assignment controls to change active work.".into()));
        }
        let request = operation_id(&binding.turn, "proposal");
        if let Some(existing) = view.plans.iter().find(|p| p.request_id == request) {
            let briefs = self.work_briefs(&binding.source).await?;
            if content.is_some()
                && existing.content.as_ref() == content.as_ref()
                && briefs
                    .iter()
                    .any(|b| b.revision == existing.brief_revision && b.body == direction)
            {
                return Ok(proposal_receipt(existing));
            }
            return Err(AppError::InvalidRequest("One proposal per reply. The saved proposal is unchanged; finish your response before further edits.".into()));
        }
        let expected_brief = self.proposal_brief_revision(binding).await?;
        if view.plans.first().map_or(0, |p| p.revision) != binding.revision
            || view.brief_revision != expected_brief
        {
            return Err(AppError::InvalidRequest("The owner changed the plan or direction during this reply. Inspect the current version and explain the conflict; do not overwrite their changes.".into()));
        }
        let scope = self.services.authorized_scope().await?;
        let outcome = self.services.local.resources().submit_guide_proposal(
            &self.services.host.credential, scope.organization().into(), scope.team().into(),
            GuideProposalDraft {
                source: binding.source.clone(), turn: binding.turn.clone(), call: call_id.into(), request,
                expected_plan: binding.revision, expected_brief,
                generation_id: operation_id(&binding.turn, "proposal-origin"),
                generation_input: format!("Guide conversation turn {}. Shared direction is stored in the linked brief revision.", binding.turn),
                direction, content, rejected_arguments, validation_error,
            },
        ).await.map_err(resource)?;
        Ok(match outcome {
            tetonic_memory::GuideProposalOutcome::Saved { plan } => proposal_receipt(&plan),
            tetonic_memory::GuideProposalOutcome::RepairNeeded { reason } => serde_json::json!({
                "saved":false,"work_launched":false,"proposal_state":"repair_needed","reason":reason,"repair_attempts_remaining":1,"allocation_repair":"For budget-only correction use rebalance with original assignment keys and new token_budget values. Set direction, plan and work_id to null; the saved first proposal supplies all unchanged fields."
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
        value["understanding"] = serde_json::json!(view.understanding);
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

//! Team dispatch policy over the existing work/run journal and managed admission.
//! The host composes agents and reads output; it does not own scheduling truth.
use crate::{errors::AppError, resources::plan_dispatch::DispatchCall};
use futures::{stream::FuturesUnordered, StreamExt};
use std::{
    collections::HashSet,
    rc::Rc,
    sync::{Arc, Mutex},
};
use tetonic_domain::ToolOutcome;
use tetonic_memory::{AssignmentState, HuddleExecution, HuddleProgress, SharedStore};
use tetonic_run::managed::DelegationParent;

mod checkpoint;
mod receipts;

#[async_trait::async_trait(?Send)]
pub(crate) trait TeamWorkHost {
    async fn admit(
        &self,
        receipt: &HuddleExecution,
        parent: &DelegationParent,
        key: &str,
    ) -> Result<(), AppError>;
    async fn contribution(
        &self,
        receipt: &HuddleExecution,
        key: &str,
    ) -> Result<ToolOutcome, AppError>;
}

#[derive(Clone)]
pub(crate) struct ProgressReader {
    pub store: SharedStore,
    pub actor: String,
    pub org: String,
    pub team: String,
}

impl ProgressReader {
    pub async fn read(&self, receipt: &HuddleExecution) -> Result<HuddleProgress, AppError> {
        let scope = self.clone();
        let source = receipt.source_work_id.clone();
        let progress = self
            .store
            .read(move |db| {
                db.huddle_progress(
                    &scope.actor,
                    &scope.org,
                    &scope.team,
                    &source,
                    chrono::Utc::now().timestamp() as u64,
                )
            })
            .await
            .map_err(|_| AppError::InferenceUnavailable)?
            .map_err(|_| {
                AppError::PolicyDenied("Team work is unavailable in this scope.".into())
            })?;
        if &progress.receipt != receipt {
            return Err(AppError::PolicyDenied(
                "The approved execution changed.".into(),
            ));
        }
        Ok(progress)
    }
}

pub(crate) struct TeamWorkController {
    pub reader: ProgressReader,
    pub host: Rc<dyn TeamWorkHost>,
    pub receipt: HuddleExecution,
    pub parent: DelegationParent,
    pub manager: Arc<tetonic_run::managed::ManagedRunService>,
    /// Delivery cache only. Durable task success determines readiness, and the
    /// model's finish guard advances only after receiving a contribution.
    pub remaining: Arc<Mutex<HashSet<String>>>,
}

impl TeamWorkController {
    pub async fn run(
        self,
        mut completion: tokio::sync::oneshot::Receiver<tetonic_run::StartIdentityJobResult>,
        mut receiver: tokio::sync::mpsc::Receiver<DispatchCall>,
    ) {
        loop {
            tokio::select! {
                _ = &mut completion => break,
                call = receiver.recv() => {
                    let Some(call) = call else { break; };
                    if call.attempt != self.parent.binding().attempt_id.0 {
                        let _ = call.reply.send(ToolOutcome::fail("Wrong managed parent", "denied"));
                        continue;
                    }
                    let outcome = tokio::select! {
                        _ = &mut completion => break,
                        result = self.dispatch_recorded(&call) => result,
                    };
                    let _ = call.reply.send(outcome);
                }
            }
        }
    }

    async fn progress(&self) -> Result<HuddleProgress, AppError> {
        let progress = self.reader.read(&self.receipt).await?;
        if progress.run_id.as_deref() != Some(self.parent.binding().run_id.0.as_str()) {
            return Err(AppError::PolicyDenied("The team run changed.".into()));
        }
        Ok(progress)
    }

    async fn dispatch_group(
        &self,
        keys: Vec<String>,
        grouped: bool,
        remaining: &Arc<Mutex<HashSet<String>>>,
    ) -> ToolOutcome {
        let mut pending = keys.clone();
        let mut running = FuturesUnordered::new();
        let mut results = vec![];
        let mut all_ok = true;
        while !pending.is_empty() || !running.is_empty() {
            let progress = match self.progress().await {
                Ok(progress) => progress,
                Err(error) => return ToolOutcome::fail(error.employee_message(), "unavailable"),
            };
            // Persisted attempts own capacity. This per-pass set only keeps a
            // deterministic order for new requests not yet admitted to the journal.
            let mut selected_agents: HashSet<_> = progress
                .assignments
                .iter()
                .filter(|a| a.holds_capacity)
                .map(|a| a.pin.agent_key.clone())
                .collect();
            for key in pending.clone() {
                let Some(assignment) = progress
                    .assignments
                    .iter()
                    .find(|a| a.pin.assignment_key == key)
                else {
                    return ToolOutcome::fail("Choose an agreed assignment", "denied");
                };
                if assignment.state == AssignmentState::NotStarted
                    && selected_agents.contains(&assignment.pin.agent_key)
                {
                    continue;
                }
                if !dependencies_ready(&progress, &key) {
                    continue;
                }
                pending.retain(|p| p != &key);
                selected_agents.insert(assignment.pin.agent_key.clone());
                running.push(async move {
                    let result = self.dispatch_assignment(&key).await;
                    let result = match result {
                        Ok(outcome) => {
                            collect_contributions(
                                &self.reader,
                                self.host.as_ref(),
                                &self.receipt,
                                &key,
                                outcome,
                                remaining,
                            )
                            .await
                        }
                        Err(error) => Err(error),
                    };
                    (
                        key,
                        result
                            .unwrap_or_else(|e| ToolOutcome::fail(e.employee_message(), "blocked")),
                    )
                });
            }
            let Some((key, outcome)) = running.next().await else {
                // A live or finishing peer may still own this agent even though
                // this call has no local future for it. Wait on durable ownership,
                // not another inference turn or a fabricated failure.
                let capacity_pending = pending.iter().any(|key| {
                    dependencies_ready(&progress, key)
                        && progress.assignments.iter().any(|a| {
                            a.pin.assignment_key == *key
                                && a.state == AssignmentState::NotStarted
                                && progress.assignments.iter().any(|other| {
                                    other.pin.agent_key == a.pin.agent_key
                                        && other.holds_capacity
                                        && other.state != AssignmentState::NeedsAttention
                                })
                        })
                });
                if capacity_pending {
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    continue;
                }
                break;
            };
            if !grouped {
                return outcome;
            }
            all_ok &= outcome.ok;
            results.push(serde_json::json!({"assignment_key":key,"ok":outcome.ok,"result":serde_json::from_str::<serde_json::Value>(&outcome.content).ok(),"message":outcome.summary}));
        }
        for key in pending {
            let outcome = ToolOutcome::fail(
                format!("{key} is waiting for its dependencies or agent capacity."),
                "blocked",
            );
            if !grouped {
                return outcome;
            }
            all_ok = false;
            results.push(serde_json::json!({"assignment_key":key,"ok":false,"result":null,"message":outcome.summary}));
        }
        let Ok(outstanding) = remaining.lock() else {
            return ToolOutcome::fail("Cannot confirm outstanding assignments", "unavailable");
        };
        results.sort_by_key(|r| keys.iter().position(|key| r["assignment_key"] == *key));
        let mut outcome = if all_ok {
            ToolOutcome::ok("Requested assignments dispatched", "")
        } else {
            ToolOutcome::fail(
                "Some assignments need attention; independent work continued",
                "blocked",
            )
        };
        outcome.content = serde_json::json!({"message":outcome.summary,"results":results,"outstanding_assignments":self.receipt.assignments.iter().filter(|p| outstanding.contains(&p.assignment_key)).map(|p| &p.assignment_key).collect::<Vec<_>>()}).to_string();
        outcome
    }

    async fn dispatch_assignment(&self, key: &str) -> Result<ToolOutcome, AppError> {
        loop {
            match self.host.admit(&self.receipt, &self.parent, key).await {
                Err(
                    AppError::ExecutionCapacityExceeded
                    | AppError::OrganizationCapacityExceeded
                    | AppError::TeamCapacityExceeded
                    | AppError::PrincipalCapacityExceeded,
                ) => {
                    // Capacity is queueing, not failed work. The parent's completion
                    // select remains responsible for canceling this local driver.
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                }
                result => {
                    result?;
                    break;
                }
            }
        }
        loop {
            let progress = self.progress().await?;
            let assignment = progress
                .assignments
                .iter()
                .find(|a| a.pin.assignment_key == key)
                .ok_or(AppError::InferenceUnavailable)?;
            match assignment.state {
                AssignmentState::Executing => {}
                AssignmentState::WaitingHuman => {
                    let ready = ready_assignments(&progress);
                    if !ready.is_empty() {
                        return Ok(ToolOutcome::ok("Waiting for the owner; other work can proceed", serde_json::json!({"state":"waiting_human","work_id":assignment.pin.work_id,"assignment_key":key,"ready_assignments":ready,"instruction":"Dispatch another ready assignment now. Revisit this key after other ready work; it waits at a tool boundary without polling the model."}).to_string()));
                    }
                }
                _ => return self.host.contribution(&self.receipt, key).await,
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }
}

fn dependencies_ready(progress: &HuddleProgress, key: &str) -> bool {
    progress
        .receipt
        .content
        .assignments
        .iter()
        .find(|a| a.key == key)
        .is_some_and(|assignment| {
            assignment.depends_on.iter().all(|dependency| {
                progress.assignments.iter().any(|a| {
                    a.pin.assignment_key == *dependency && a.state == AssignmentState::Completed
                })
            })
        })
}

pub(crate) fn ready_assignments(progress: &HuddleProgress) -> Vec<String> {
    progress
        .assignments
        .iter()
        .filter(|a| {
            a.state == AssignmentState::NotStarted
                && !progress
                    .assignments
                    .iter()
                    .any(|other| other.holds_capacity && other.pin.agent_key == a.pin.agent_key)
                && dependencies_ready(progress, &a.pin.assignment_key)
        })
        .map(|a| a.pin.assignment_key.clone())
        .collect()
}

pub(crate) async fn collect_contributions(
    reader: &ProgressReader,
    host: &dyn TeamWorkHost,
    receipt: &HuddleExecution,
    requested: &str,
    mut outcome: ToolOutcome,
    remaining: &Arc<Mutex<HashSet<String>>>,
) -> Result<ToolOutcome, AppError> {
    let pending = remaining
        .lock()
        .map_err(|_| AppError::InferenceUnavailable)?
        .clone();
    let progress = reader.read(receipt).await?;
    let mut payload: serde_json::Value =
        serde_json::from_str(&outcome.content).map_err(|_| AppError::InferenceUnavailable)?;
    let mut delivered = vec![];
    let mut additional = vec![];
    for a in progress.assignments.iter().filter(|a| {
        a.state == AssignmentState::Completed && pending.contains(&a.pin.assignment_key)
    }) {
        if a.pin.assignment_key == requested {
            if payload["state"] == "completed" {
                delivered.push(requested.to_owned());
            }
            continue;
        }
        let result = host.contribution(receipt, &a.pin.assignment_key).await?;
        let mut value: serde_json::Value =
            serde_json::from_str(&result.content).map_err(|_| AppError::InferenceUnavailable)?;
        value["assignment_key"] = serde_json::json!(a.pin.assignment_key);
        additional.push(value);
        delivered.push(a.pin.assignment_key.clone());
    }
    let mut pending = remaining
        .lock()
        .map_err(|_| AppError::InferenceUnavailable)?;
    for key in delivered {
        pending.remove(&key);
    }
    payload["also_completed"] = serde_json::json!(additional);
    payload["outstanding_assignments"] = serde_json::json!(receipt
        .assignments
        .iter()
        .filter(|p| pending.contains(&p.assignment_key))
        .map(|p| &p.assignment_key)
        .collect::<Vec<_>>());
    outcome.content = payload.to_string();
    Ok(outcome)
}

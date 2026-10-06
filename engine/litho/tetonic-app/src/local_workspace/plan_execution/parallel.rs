//! Dependency scheduling over existing managed admissions, never a second runtime.
use super::*;
use futures::{stream::FuturesUnordered, StreamExt};
use tetonic_domain::ToolOutcome;

impl LocalWorkspace {
    pub(super) async fn dispatch_plan_group(
        &self,
        receipt: &HuddleExecution,
        parent: &tetonic_run::managed::DelegationParent,
        keys: Vec<String>,
        grouped: bool,
        remaining: &Arc<Mutex<HashSet<String>>>,
    ) -> ToolOutcome {
        let mut pending = keys.clone();
        let mut running = FuturesUnordered::new();
        let mut occupied = HashSet::new();
        let mut results = vec![];
        let mut all_ok = true;
        while !pending.is_empty() || !running.is_empty() {
            for key in pending.clone() {
                let Some(pin) = receipt.assignments.iter().find(|p| p.assignment_key == key) else {
                    return ToolOutcome::fail("Choose an agreed assignment", "denied");
                };
                if occupied.contains(&pin.agent_key) {
                    continue;
                }
                let Some(assignment) = receipt.content.assignments.iter().find(|a| a.key == key)
                else {
                    return ToolOutcome::fail("Assignment unavailable", "unavailable");
                };
                let mut ready = true;
                for dependency in &assignment.depends_on {
                    let Some(other) = receipt
                        .assignments
                        .iter()
                        .find(|p| &p.assignment_key == dependency)
                    else {
                        return ToolOutcome::fail("Dependency unavailable", "unavailable");
                    };
                    if !self
                        .task(&other.work_id)
                        .await
                        .is_ok_and(|t| t.state == "completed")
                    {
                        ready = false;
                        break;
                    }
                }
                if !ready {
                    continue;
                }
                pending.retain(|p| p != &key);
                occupied.insert(pin.agent_key.clone());
                let agent = pin.agent_key.clone();
                running.push(async move {
                    let dispatched = loop {
                        match self.dispatch_plan_assignment(receipt, parent, &key).await {
                            Err(
                                AppError::ExecutionCapacityExceeded
                                | AppError::OrganizationCapacityExceeded
                                | AppError::TeamCapacityExceeded
                                | AppError::PrincipalCapacityExceeded,
                            ) => {
                                // Capacity is a queue condition, not a failed assignment.
                                // The enclosing parent completion select cancels this wait.
                                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                            }
                            result => break result,
                        }
                    };
                    let result = match dispatched {
                        Ok(outcome) => {
                            self.collect_plan_contributions(receipt, &key, outcome, remaining)
                                .await
                        }
                        Err(error) => Err(error),
                    };
                    let outcome = result.unwrap_or_else(|error| {
                        ToolOutcome::fail(error.employee_message(), "blocked")
                    });
                    (key, agent, outcome)
                });
            }
            let Some((key, agent, outcome)) = running.next().await else {
                break;
            };
            occupied.remove(&agent);
            if !grouped {
                return outcome;
            }
            all_ok &= outcome.ok;
            results.push(serde_json::json!({"assignment_key":key,"ok":outcome.ok,"result":serde_json::from_str::<serde_json::Value>(&outcome.content).ok(),"message":outcome.summary}));
        }
        // No running producer can satisfy these dependencies. Never run them with
        // missing evidence, or let one failure prevent unrelated ready workers.
        for key in pending {
            let outcome =
                ToolOutcome::fail(format!("{key} is waiting for its dependencies."), "blocked");
            if !grouped {
                return outcome;
            }
            all_ok = false;
            results.push(serde_json::json!({"assignment_key":key,"ok":false,"result":null,"message":outcome.summary}));
        }
        let Ok(outstanding) = remaining.lock() else {
            return ToolOutcome::fail("Cannot confirm outstanding assignments", "unavailable");
        };
        // Return in the requested order, independently of completion timing.
        results.sort_by_key(|r| keys.iter().position(|key| r["assignment_key"] == *key));
        let mut outcome = if all_ok {
            ToolOutcome::ok("Requested assignments dispatched", "")
        } else {
            ToolOutcome::fail(
                "Some assignments need attention; independent work continued",
                "blocked",
            )
        };
        outcome.content = serde_json::json!({"message":outcome.summary,"results":results,"outstanding_assignments":receipt.assignments.iter().filter(|p| outstanding.contains(&p.assignment_key)).map(|p| &p.assignment_key).collect::<Vec<_>>()}).to_string();
        outcome
    }
}

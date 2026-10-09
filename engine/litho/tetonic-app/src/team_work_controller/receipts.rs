//! Persist controller requests/responses without introducing execution ownership.
use super::*;
use tetonic_memory::{HuddleDispatchCommand, HuddleDispatchResult};

#[cfg(test)]
#[path = "receipt_tests.rs"]
mod tests;

#[derive(Clone)]
struct DispatchBinding {
    reader: ProgressReader,
    source: String,
    run: String,
    attempt: String,
    lease: tetonic_domain::LeaseProof,
    call: String,
    keys: Vec<String>,
    grouped: bool,
}

impl DispatchBinding {
    fn command(&self) -> HuddleDispatchCommand<'_> {
        HuddleDispatchCommand {
            actor: &self.reader.actor,
            org: &self.reader.org,
            team: &self.reader.team,
            source: &self.source,
            run: &self.run,
            attempt: &self.attempt,
            lease: &self.lease,
            call: &self.call,
            keys: &self.keys,
            grouped: self.grouped,
            now: chrono::Utc::now().timestamp() as u64,
        }
    }
}

impl TeamWorkController {
    pub(super) async fn dispatch_recorded(&self, call: &DispatchCall) -> ToolOutcome {
        match self.try_dispatch_recorded(call).await {
            Ok(outcome) => outcome,
            Err(_) => ToolOutcome::fail(
                "The dispatch receipt could not be confirmed. Saved contributions remain available; inspect the plan before starting more work.",
                "unavailable",
            ),
        }
    }

    async fn try_dispatch_recorded(&self, call: &DispatchCall) -> Result<ToolOutcome, AppError> {
        // A serialized attempt ID cannot authorize a controller. Recheck the
        // original runtime handle/credential, then transact under its lease fence.
        let lease = self.parent.authorize_dispatch().await.map_err(|_| {
            AppError::PolicyDenied("The coordinator no longer owns this execution.".into())
        })?;
        if call.attempt != self.parent.binding().attempt_id.0 {
            return Err(AppError::PolicyDenied(
                "The coordinator no longer owns this execution.".into(),
            ));
        }
        let binding = DispatchBinding {
            reader: self.reader.clone(),
            source: self.receipt.source_work_id.clone(),
            run: self.parent.binding().run_id.0.clone(),
            attempt: call.attempt.clone(),
            lease,
            call: call.call_id.clone(),
            keys: call.keys.clone(),
            grouped: call.grouped,
        };
        let accepted = self
            .reader
            .store
            .write({
                let binding = binding.clone();
                move |db| db.accept_huddle_dispatch(binding.command())
            })
            .await
            .map_err(|_| AppError::InferenceUnavailable)?
            .map_err(|_| AppError::InferenceUnavailable)?;
        let result = match accepted.result {
            Some(saved) => saved,
            None => {
                // Only a persisted response may advance the live finish guard.
                // Concurrent child collection uses a provisional delivery cache.
                let before = self
                    .remaining
                    .lock()
                    .map_err(|_| AppError::InferenceUnavailable)?
                    .clone();
                let pending = Arc::new(Mutex::new(before.clone()));
                let outcome = self
                    .dispatch_group(call.keys.clone(), call.grouped, &pending)
                    .await;
                let delivered_keys = if !response_contains_contributions(&outcome) {
                    // A group may collect a child and then abort on a later
                    // progress read. That early error contains no contributions.
                    vec![]
                } else {
                    let after = pending.lock().map_err(|_| AppError::InferenceUnavailable)?;
                    self.receipt
                        .assignments
                        .iter()
                        .filter(|p| {
                            before.contains(&p.assignment_key) && !after.contains(&p.assignment_key)
                        })
                        .map(|p| p.assignment_key.clone())
                        .collect()
                };
                let result = HuddleDispatchResult {
                    ok: outcome.ok,
                    summary: outcome.summary,
                    content: outcome.content,
                    error_kind: outcome.error_kind,
                    delivered_keys,
                };
                self.parent.authorize_dispatch().await.map_err(|_| {
                    AppError::PolicyDenied("The coordinator no longer owns this execution.".into())
                })?;
                self.reader
                    .store
                    .write(move |db| db.complete_huddle_dispatch(binding.command(), result))
                    .await
                    .map_err(|_| AppError::InferenceUnavailable)?
                    .map_err(|_| AppError::InferenceUnavailable)?
                    .result
                    .ok_or(AppError::InferenceUnavailable)?
            }
        };
        let mut remaining = self
            .remaining
            .lock()
            .map_err(|_| AppError::InferenceUnavailable)?;
        for key in &result.delivered_keys {
            remaining.remove(key);
        }
        Ok(ToolOutcome {
            ok: result.ok,
            summary: result.summary,
            content: result.content,
            error_kind: result.error_kind,
            change: None,
        })
    }
}

fn response_contains_contributions(outcome: &ToolOutcome) -> bool {
    serde_json::from_str::<serde_json::Value>(&outcome.content)
        .is_ok_and(|payload| payload["outstanding_assignments"].is_array())
}

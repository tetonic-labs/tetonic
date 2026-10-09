//! Rebuild delivery from the coordinator's received calls, never task completion.
use super::receipts::{receipt_failure, DispatchBinding};
use super::*;
use tetonic_memory::HuddleDispatchReceipt;

pub(super) fn validate_call(call: &DispatchCall) -> Result<(), AppError> {
    let pending = &call.checkpoint.pending;
    call.checkpoint
        .received_host_calls()
        .map_err(|_| AppError::InferenceUnavailable)?;
    if pending.call_id != call.call_id
        || pending.attempt_id.as_deref() != Some(&call.attempt)
        || pending.tool_name != crate::resources::plan_dispatch::DISPATCH
        || !selection_matches(&pending.arguments, &call.keys, call.grouped)
    {
        return Err(AppError::InferenceUnavailable);
    }
    Ok(())
}

impl TeamWorkController {
    pub(super) async fn bind_checkpoint(
        &self,
        call: &DispatchCall,
        binding: &DispatchBinding,
        accepted: &HuddleDispatchReceipt,
    ) -> Result<(), AppError> {
        let scope = self.reader.clone();
        let source = self.receipt.source_work_id.clone();
        let records = self
            .reader
            .store
            .read(move |db| {
                db.huddle_dispatch_receipts(&scope.actor, &scope.org, &scope.team, &source)
            })
            .await
            .map_err(|_| AppError::InferenceUnavailable)?
            .map_err(|_| AppError::InferenceUnavailable)?;
        let remaining = rebuild_delivery(&self.receipt, accepted, &call.checkpoint, &records)?;
        match &accepted.checkpoint {
            Some(reference) => {
                let saved = self
                    .manager
                    .read_dispatch_checkpoint(&self.parent, reference)
                    .await
                    .map_err(|_| AppError::InferenceUnavailable)?;
                if serde_json::to_value(saved).map_err(|_| AppError::InferenceUnavailable)?
                    != serde_json::to_value(&call.checkpoint)
                        .map_err(|_| AppError::InferenceUnavailable)?
                {
                    return Err(AppError::InferenceUnavailable);
                }
            }
            None => {
                if accepted.result.is_some() {
                    return Err(AppError::InferenceUnavailable);
                }
                let reference = self
                    .manager
                    .save_dispatch_checkpoint(&self.parent, &call.checkpoint)
                    .await
                    .map_err(|_| AppError::InferenceUnavailable)?;
                let binding = binding.clone();
                self.reader
                    .store
                    .write(move |db| {
                        db.bind_huddle_dispatch_checkpoint(binding.command(), reference)
                    })
                    .await
                    .map_err(|_| AppError::InferenceUnavailable)?
                    .map_err(|_| AppError::InferenceUnavailable)?;
            }
        }
        *self
            .remaining
            .lock()
            .map_err(|_| AppError::InferenceUnavailable)? = remaining;
        Ok(())
    }
}

fn rebuild_delivery(
    execution: &HuddleExecution,
    current: &HuddleDispatchReceipt,
    checkpoint: &tetonic_core::WaitCheckpoint,
    records: &[HuddleDispatchReceipt],
) -> Result<HashSet<String>, AppError> {
    let mut remaining: HashSet<_> = execution
        .assignments
        .iter()
        .map(|p| p.assignment_key.clone())
        .collect();
    for received in checkpoint
        .received_host_calls()
        .map_err(|_| AppError::InferenceUnavailable)?
    {
        if received.request.tool_name != crate::resources::plan_dispatch::DISPATCH {
            continue;
        }
        // A persistence failure response contains no contributions. Even if the
        // result committed before the reply was lost, it has not been delivered.
        if received.matches_response(&receipt_failure().to_model_string()) {
            continue;
        }
        let record = records
            .iter()
            .find(|record| {
                record.call_id == received.request.call_id
                    && record.run_id == current.run_id
                    && record.attempt_id == current.attempt_id
            })
            .ok_or(AppError::InferenceUnavailable)?;
        if record.stop_binding != current.stop_binding
            || record.checkpoint.is_none()
            || !selection_matches(&received.request.arguments, &record.keys, record.grouped)
        {
            return Err(AppError::InferenceUnavailable);
        }
        let result = record
            .result
            .as_ref()
            .ok_or(AppError::InferenceUnavailable)?;
        let response = if result.content.is_empty() {
            &result.summary
        } else {
            &result.content
        };
        if !received.matches_response(response) {
            return Err(AppError::InferenceUnavailable);
        }
        for key in &result.delivered_keys {
            remaining.remove(key);
        }
    }
    Ok(remaining)
}

fn selection_matches(arguments: &serde_json::Value, keys: &[String], grouped: bool) -> bool {
    if grouped {
        arguments == &serde_json::json!({"assignment_keys":keys})
    } else {
        keys.len() == 1 && arguments == &serde_json::json!({"assignment_key":keys[0]})
    }
}

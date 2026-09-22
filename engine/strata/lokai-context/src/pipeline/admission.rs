use super::ExpansionLimits;
use lokai_domain::{ContextExpansionRequest, RunId, SessionId, TaskId};
use std::{collections::HashMap, sync::Mutex};

type Owner = (SessionId, RunId, TaskId);

#[derive(Default)]
pub(super) struct Admission(Mutex<HashMap<Owner, usize>>);

pub(super) struct Permit<'a> {
    admission: &'a Admission,
    owner: Owner,
}

impl Admission {
    pub(super) fn enter(
        &self,
        request: &ContextExpansionRequest,
        limits: &ExpansionLimits,
    ) -> Result<Permit<'_>, String> {
        let owner = (
            request.session_id.clone(),
            request.run_id.clone(),
            request.task_id.clone(),
        );
        let mut active = self
            .0
            .lock()
            .map_err(|_| "expansion admission unavailable")?;
        if active.values().sum::<usize>() >= limits.max_concurrent
            || active.get(&owner).copied().unwrap_or(0) >= limits.max_concurrent_per_owner
        {
            return Err(
                "context expansion concurrency limit reached; retry after active work completes"
                    .into(),
            );
        }
        *active.entry(owner.clone()).or_default() += 1;
        Ok(Permit {
            admission: self,
            owner,
        })
    }
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.admission.0.lock() {
            if let Some(count) = active.get_mut(&self.owner) {
                *count -= 1;
                if *count == 0 {
                    active.remove(&self.owner);
                }
            }
        }
    }
}

//! Active fabric job / attempt tracking (AC2-7).

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;

use crate::new_fabric_attempt_id;

/// Maximum in-flight registry entries before oldest jobs are evicted.
const MAX_REGISTRY_JOBS: usize = 512;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StaleResultError {
    #[error("unknown job")]
    UnknownJob,
    #[error("job cancelled")]
    Cancelled,
    #[error("attempt superseded")]
    Superseded,
    #[error("duplicate result")]
    Duplicate,
}

/// Live attempt flags for M5-4 `ExpectedResultBinding` (non-mutating).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttemptBindingFlags {
    pub known: bool,
    pub attempt_canceled: bool,
    pub attempt_superseded: bool,
    pub another_attempt_won: bool,
}

struct JobRecord {
    active_attempt: String,
    cancel_generation: u64,
    settled: bool,
    session_id: Option<String>,
    run_id: Option<String>,
    dispatched_worker: Option<String>,
    dispatched_class: Option<lokai_domain::DataClass>,
    project_policy: Option<lokai_domain::ProjectPlacementPolicy>,
    #[allow(dead_code)]
    created_ms: u64,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Default)]
struct RegistryState {
    jobs: HashMap<String, JobRecord>,
    order: VecDeque<String>,
}

/// Coordinator-side registry: one active attempt per job id.
#[derive(Default)]
pub struct ActiveJobRegistry {
    state: Mutex<RegistryState>,
}

impl ActiveJobRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.state.lock().unwrap().jobs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.state.lock().unwrap().jobs.is_empty()
    }

    pub fn begin_attempt(&self, job_id: &str, session_id: Option<&str>) -> String {
        self.begin_or_bind_attempt(job_id, session_id, None, None)
    }

    /// Bind a coordinator-leased attempt id into the AJR (R2-1). Does not mint a competing id.
    pub fn begin_attempt_with_id(
        &self,
        job_id: &str,
        session_id: Option<&str>,
        attempt_id: &str,
    ) -> String {
        self.begin_or_bind_attempt(job_id, session_id, Some(attempt_id), None)
    }

    pub fn begin_or_bind_attempt(
        &self,
        job_id: &str,
        session_id: Option<&str>,
        bound_attempt_id: Option<&str>,
        run_id: Option<&str>,
    ) -> String {
        let attempt_id = match bound_attempt_id {
            Some(id) if !id.is_empty() => id.to_string(),
            _ => new_fabric_attempt_id(),
        };
        let mut g = self.state.lock().unwrap();
        Self::prune_if_needed(&mut g);
        g.jobs.insert(
            job_id.to_string(),
            JobRecord {
                active_attempt: attempt_id.clone(),
                cancel_generation: 0,
                settled: false,
                session_id: session_id.map(String::from),
                run_id: run_id.filter(|s| !s.is_empty()).map(String::from),
                dispatched_worker: None,
                dispatched_class: None,
                project_policy: None,
                created_ms: now_ms(),
            },
        );
        g.order.push_back(job_id.to_string());
        attempt_id
    }

    /// Failover or user cancel starts a new attempt generation.
    pub fn supersede_attempt(&self, job_id: &str) -> String {
        let attempt_id = new_fabric_attempt_id();
        let mut g = self.state.lock().unwrap();
        if let Some(rec) = g.jobs.get_mut(job_id) {
            rec.cancel_generation += 1;
            rec.active_attempt = attempt_id.clone();
            rec.settled = false;
        } else {
            g.jobs.insert(
                job_id.to_string(),
                JobRecord {
                    active_attempt: attempt_id.clone(),
                    cancel_generation: 0,
                    settled: false,
                    session_id: None,
                    run_id: None,
                    dispatched_worker: None,
                    dispatched_class: None,
                    project_policy: None,
                    created_ms: now_ms(),
                },
            );
            g.order.push_back(job_id.to_string());
        }
        attempt_id
    }

    /// Drop all in-flight jobs when trust or policy invalidates queued dispatches (M5-3).
    pub fn cancel_all_pending(&self) {
        let mut g = self.state.lock().unwrap();
        for rec in g.jobs.values_mut() {
            rec.cancel_generation += 1;
            rec.active_attempt.clear();
            rec.settled = false;
        }
    }

    pub fn mark_dispatched(
        &self,
        job_id: &str,
        attempt_id: &str,
        worker_id: &str,
        data_class: lokai_domain::DataClass,
        project_policy: lokai_domain::ProjectPlacementPolicy,
    ) -> Result<(), StaleResultError> {
        let mut g = self.state.lock().unwrap();
        let Some(rec) = g.jobs.get_mut(job_id) else {
            return Err(StaleResultError::UnknownJob);
        };
        if rec.active_attempt != attempt_id {
            return Err(StaleResultError::Superseded);
        }
        rec.dispatched_worker = Some(worker_id.to_string());
        rec.dispatched_class = Some(data_class);
        rec.project_policy = Some(project_policy);
        Ok(())
    }

    pub fn dispatched_for_worker(
        &self,
        worker_id: &str,
    ) -> Vec<(
        String,
        Option<String>,
        lokai_domain::DataClass,
        lokai_domain::ProjectPlacementPolicy,
    )> {
        self.state
            .lock()
            .unwrap()
            .jobs
            .iter()
            .filter(|&(_job_id, rec)| rec.dispatched_worker.as_deref() == Some(worker_id))
            .map(|(job_id, rec)| {
                (
                    job_id.clone(),
                    rec.session_id.clone(),
                    rec.dispatched_class.unwrap_or_default(),
                    rec.project_policy.clone().unwrap_or_default(),
                )
            })
            .collect()
    }

    pub fn cancel_job(&self, job_id: &str) {
        let mut g = self.state.lock().unwrap();
        if let Some(rec) = g.jobs.get_mut(job_id) {
            rec.cancel_generation += 1;
            rec.active_attempt.clear();
            rec.settled = false;
        }
    }

    /// Drop all in-flight jobs for a cancelled session (RPC `session/cancel`).
    pub fn cancel_session(&self, session_id: &str) {
        let mut g = self.state.lock().unwrap();
        g.jobs
            .retain(|_, rec| rec.session_id.as_deref() != Some(session_id));
    }

    /// Active (job_id, attempt_id, stored_run_id) triples for a session index leftover.
    pub fn jobs_for_session(&self, session_id: &str) -> Vec<(String, String, Option<String>)> {
        let g = self.state.lock().unwrap();
        g.jobs
            .iter()
            .filter(|(_, rec)| rec.session_id.as_deref() == Some(session_id))
            .filter(|(_, rec)| !rec.active_attempt.is_empty())
            .map(|(job_id, rec)| {
                (
                    job_id.clone(),
                    rec.active_attempt.clone(),
                    rec.run_id.clone(),
                )
            })
            .collect()
    }

    /// Non-mutating binding flags for M5-4 envelope validation (does not settle).
    pub fn peek_attempt(&self, job_id: &str, attempt_id: &str) -> AttemptBindingFlags {
        let g = self.state.lock().unwrap();
        let Some(rec) = g.jobs.get(job_id) else {
            return AttemptBindingFlags {
                known: false,
                attempt_canceled: true,
                attempt_superseded: false,
                another_attempt_won: false,
            };
        };
        if rec.active_attempt.is_empty() {
            return AttemptBindingFlags {
                known: true,
                attempt_canceled: true,
                attempt_superseded: false,
                another_attempt_won: false,
            };
        }
        if rec.active_attempt != attempt_id {
            return AttemptBindingFlags {
                known: true,
                attempt_canceled: false,
                attempt_superseded: true,
                // Settled under a different active attempt ⇒ another attempt already won.
                another_attempt_won: rec.settled,
            };
        }
        AttemptBindingFlags {
            known: true,
            attempt_canceled: false,
            attempt_superseded: false,
            // Same attempt already settled is duplicate delivery (disposition idempotency),
            // not a competing winner.
            another_attempt_won: false,
        }
    }

    pub fn validate(&self, job_id: &str, attempt_id: &str) -> Result<(), StaleResultError> {
        let mut g = self.state.lock().unwrap();
        let Some(rec) = g.jobs.get_mut(job_id) else {
            return Err(StaleResultError::UnknownJob);
        };
        if rec.active_attempt.is_empty() {
            return Err(StaleResultError::Cancelled);
        }
        if rec.active_attempt != attempt_id {
            return Err(StaleResultError::Superseded);
        }
        if rec.settled {
            return Err(StaleResultError::Duplicate);
        }
        rec.settled = true;
        Ok(())
    }

    pub fn finish_job(&self, job_id: &str) {
        self.state.lock().unwrap().jobs.remove(job_id);
    }

    fn prune_if_needed(g: &mut RegistryState) {
        while g.jobs.len() >= MAX_REGISTRY_JOBS {
            let Some(id) = g.order.pop_front() else {
                break;
            };
            g.jobs.remove(&id);
        }
    }
}

/// Ensures a pooled chat job is removed from the registry on every exit path.
pub(crate) struct JobFinishGuard<'a> {
    registry: &'a ActiveJobRegistry,
    job_id: String,
    done: bool,
}

impl<'a> JobFinishGuard<'a> {
    pub fn new(registry: &'a ActiveJobRegistry, job_id: &str) -> Self {
        Self {
            registry,
            job_id: job_id.to_string(),
            done: false,
        }
    }

    #[allow(dead_code)]
    pub fn finish(mut self) {
        self.registry.finish_job(&self.job_id);
        self.done = true;
    }
}

impl Drop for JobFinishGuard<'_> {
    fn drop(&mut self) {
        if !self.done {
            self.registry.finish_job(&self.job_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_or_bind_honors_leased_attempt_id() {
        let reg = ActiveJobRegistry::new();
        let id = reg.begin_or_bind_attempt("job_1", Some("sess"), Some("att_leased"), None);
        assert_eq!(id, "att_leased");
        assert_eq!(reg.validate("job_1", "att_leased"), Ok(()));
    }

    #[test]
    fn rejects_superseded_attempt() {
        let reg = ActiveJobRegistry::new();
        let job = "job_1";
        let _a1 = reg.begin_attempt(job, None);
        let a2 = reg.supersede_attempt(job);
        assert_eq!(reg.validate(job, &a2), Ok(()));
        assert_eq!(
            reg.validate(job, "att_stale"),
            Err(StaleResultError::Superseded)
        );
    }

    #[test]
    fn rejects_post_cancel() {
        let reg = ActiveJobRegistry::new();
        let job = "job_1";
        let a1 = reg.begin_attempt(job, None);
        reg.cancel_job(job);
        assert_eq!(reg.validate(job, &a1), Err(StaleResultError::Cancelled));
    }

    #[test]
    fn cancel_all_pending_invalidates_active_attempts() {
        let reg = ActiveJobRegistry::new();
        let a1 = reg.begin_attempt("job_a", Some("s1"));
        let a2 = reg.begin_attempt("job_b", None);
        reg.cancel_all_pending();
        assert!(reg.validate("job_a", &a1).is_err());
        assert!(reg.validate("job_b", &a2).is_err());
    }

    #[test]
    fn records_dispatched_inputs_for_post_dispatch_trust_audit() {
        let reg = ActiveJobRegistry::new();
        let attempt = reg.begin_attempt("job_a", Some("s1"));
        reg.mark_dispatched(
            "job_a",
            &attempt,
            "worker_a",
            lokai_domain::DataClass::SensitiveSource,
            lokai_domain::ProjectPlacementPolicy::default(),
        )
        .unwrap();
        let records = reg.dispatched_for_worker("worker_a");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].0, "job_a");
        assert_eq!(records[0].2, lokai_domain::DataClass::SensitiveSource);
    }

    #[test]
    fn rejects_duplicate_result() {
        let reg = ActiveJobRegistry::new();
        let job = "job_1";
        let a1 = reg.begin_attempt(job, None);
        assert!(reg.validate(job, &a1).is_ok());
        assert_eq!(reg.validate(job, &a1), Err(StaleResultError::Duplicate));
    }

    #[test]
    fn failover_supersedes_stale_attempt() {
        let reg = ActiveJobRegistry::new();
        let job = "job_1";
        let a1 = reg.begin_attempt(job, None);
        let a2 = reg.supersede_attempt(job);
        assert_eq!(reg.validate(job, &a1), Err(StaleResultError::Superseded));
        assert!(reg.validate(job, &a2).is_ok());
    }

    #[test]
    fn cancel_session_removes_session_jobs() {
        let reg = ActiveJobRegistry::new();
        reg.begin_attempt("job_a", Some("sess_1"));
        reg.begin_attempt("job_b", Some("sess_2"));
        reg.cancel_session("sess_1");
        assert_eq!(reg.len(), 1);
        assert_eq!(
            reg.validate("job_a", "x"),
            Err(StaleResultError::UnknownJob)
        );
    }

    #[test]
    fn job_finish_guard_cleans_on_drop() {
        let reg = ActiveJobRegistry::new();
        reg.begin_attempt("job_x", None);
        {
            let _g = JobFinishGuard::new(&reg, "job_x");
            assert_eq!(reg.len(), 1);
        }
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn registry_prunes_when_over_cap() {
        let reg = ActiveJobRegistry::new();
        for i in 0..MAX_REGISTRY_JOBS + 4 {
            reg.begin_attempt(&format!("job_{i}"), None);
        }
        assert!(reg.len() <= MAX_REGISTRY_JOBS);
    }

    #[test]
    fn speculative_loser_late_rejected() {
        let reg = ActiveJobRegistry::new();
        let primary_job = "job_primary";
        let spec_job = "job_spec";
        let primary = reg.begin_attempt(primary_job, Some("sess:primary"));
        let speculative = reg.begin_attempt(spec_job, Some("sess:spec"));
        // First valid wins — settle primary, cancel speculative loser.
        assert_eq!(reg.validate(primary_job, &primary), Ok(()));
        reg.cancel_job(spec_job);
        assert_eq!(
            reg.validate(spec_job, &speculative),
            Err(StaleResultError::Cancelled)
        );
        // Late duplicate of winner also rejected.
        assert_eq!(
            reg.validate(primary_job, &primary),
            Err(StaleResultError::Duplicate)
        );
    }
}

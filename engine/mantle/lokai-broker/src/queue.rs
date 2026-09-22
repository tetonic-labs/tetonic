//! Bounded admission queue and backpressure (M6-1).

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use lokai_domain::ids::{AttemptId, RunId, TaskId};
use serde::{Deserialize, Serialize};

use crate::priority::{ComputePriority, FairnessPolicy};
use crate::types::ComputeRequest;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueAdmission {
    pub attempt_id: AttemptId,
    pub position: usize,
    pub queued_at: DateTime<Utc>,
    pub queue_deadline: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct QueueLimits {
    pub global_max: usize,
    pub per_run_max: usize,
    pub per_worker_max: usize,
}

impl Default for QueueLimits {
    fn default() -> Self {
        Self {
            global_max: 256,
            per_run_max: 64,
            per_worker_max: 64,
        }
    }
}

#[derive(Debug, Clone)]
struct QueueEntry {
    attempt_id: AttemptId,
    run_id: RunId,
    task_id: TaskId,
    priority: ComputePriority,
    queued_at: DateTime<Utc>,
    queue_deadline: DateTime<Utc>,
    worker_key: String,
}

struct QueueState {
    limits: QueueLimits,
    fairness: FairnessPolicy,
    entries: VecDeque<QueueEntry>,
    members: HashSet<String>,
    per_run: HashMap<String, usize>,
    per_worker: HashMap<String, usize>,
    consecutive_background: u32,
}

pub struct QueueManager {
    inner: Mutex<QueueState>,
}

impl QueueManager {
    pub fn new(limits: QueueLimits, fairness: FairnessPolicy) -> Self {
        Self {
            inner: Mutex::new(QueueState {
                limits,
                fairness,
                entries: VecDeque::new(),
                members: HashSet::new(),
                per_run: HashMap::new(),
                per_worker: HashMap::new(),
                consecutive_background: 0,
            }),
        }
    }

    pub fn depth(&self) -> usize {
        self.inner.lock().unwrap().members.len()
    }

    #[allow(clippy::result_unit_err)]
    pub fn enqueue(&self, req: &ComputeRequest, now: DateTime<Utc>) -> Result<QueueAdmission, ()> {
        let mut g = self.inner.lock().unwrap();
        if g.members.contains(&req.attempt_id.0) {
            let position = g
                .entries
                .iter()
                .filter(|e| e.priority.rank() < req.priority.rank())
                .count();
            return Ok(QueueAdmission {
                attempt_id: req.attempt_id.clone(),
                position,
                queued_at: now,
                queue_deadline: req.deadline.queue_deadline,
            });
        }
        if g.members.len() >= g.limits.global_max {
            if req.priority.rank() > ComputePriority::Interactive.rank() {
                return Err(());
            }
            if !evict_lowest(&mut g) {
                return Err(());
            }
        }
        let run_count = g.per_run.get(&req.run_id.0).copied().unwrap_or(0);
        if run_count >= g.limits.per_run_max {
            return Err(());
        }
        let worker_key = req
            .target_worker_id
            .as_ref()
            .map(|w| w.0.clone())
            .unwrap_or_else(|| "local".into());
        let worker_count = g.per_worker.get(&worker_key).copied().unwrap_or(0);
        if worker_count >= g.limits.per_worker_max {
            return Err(());
        }

        // Preserve interactive capacity when the queue is nearly full.
        if req.priority >= ComputePriority::Background {
            let free = g.limits.global_max.saturating_sub(g.members.len());
            if free <= g.fairness.interactive_reserved_slots as usize {
                return Err(());
            }
        }

        g.entries.push_back(QueueEntry {
            attempt_id: req.attempt_id.clone(),
            run_id: req.run_id.clone(),
            task_id: req.task_id.clone(),
            priority: req.priority,
            queued_at: now,
            queue_deadline: req.deadline.queue_deadline,
            worker_key: worker_key.clone(),
        });
        g.members.insert(req.attempt_id.0.clone());
        *g.per_run.entry(req.run_id.0.clone()).or_insert(0) += 1;
        *g.per_worker.entry(worker_key).or_insert(0) += 1;
        let position = g.members.len().saturating_sub(1);
        Ok(QueueAdmission {
            attempt_id: req.attempt_id.clone(),
            position,
            queued_at: now,
            queue_deadline: req.deadline.queue_deadline,
        })
    }

    pub fn remove(&self, attempt_id: &AttemptId) -> bool {
        let mut g = self.inner.lock().unwrap();
        if !g.members.remove(&attempt_id.0) {
            return false;
        }
        if let Some(pos) = g.entries.iter().position(|e| e.attempt_id == *attempt_id) {
            if let Some(e) = g.entries.remove(pos) {
                if let Some(c) = g.per_run.get_mut(&e.run_id.0) {
                    *c = c.saturating_sub(1);
                }
                if let Some(c) = g.per_worker.get_mut(&e.worker_key) {
                    *c = c.saturating_sub(1);
                }
            }
        }
        true
    }

    pub fn pop_next(
        &self,
        now: DateTime<Utc>,
    ) -> Option<(AttemptId, RunId, TaskId, ComputePriority)> {
        let mut g = self.inner.lock().unwrap();
        // Drop expired.
        let expired: Vec<QueueEntry> = g
            .entries
            .iter()
            .filter(|e| e.queue_deadline <= now)
            .cloned()
            .collect();
        for e in expired {
            g.entries.retain(|x| x.attempt_id != e.attempt_id);
            g.members.remove(&e.attempt_id.0);
            if let Some(c) = g.per_run.get_mut(&e.run_id.0) {
                *c = c.saturating_sub(1);
            }
            if let Some(c) = g.per_worker.get_mut(&e.worker_key) {
                *c = c.saturating_sub(1);
            }
        }

        if g.entries.is_empty() {
            return None;
        }

        let prefer_interactive = g.consecutive_background >= g.fairness.max_consecutive_background
            && g.entries
                .iter()
                .any(|e| e.priority <= ComputePriority::Interactive);

        let idx = g
            .entries
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                if prefer_interactive {
                    let a_i = a.priority <= ComputePriority::Interactive;
                    let b_i = b.priority <= ComputePriority::Interactive;
                    match (a_i, b_i) {
                        (true, false) => std::cmp::Ordering::Less,
                        (false, true) => std::cmp::Ordering::Greater,
                        _ => a
                            .priority
                            .rank()
                            .cmp(&b.priority.rank())
                            .then_with(|| a.queued_at.cmp(&b.queued_at)),
                    }
                } else {
                    a.priority
                        .rank()
                        .cmp(&b.priority.rank())
                        .then_with(|| a.queued_at.cmp(&b.queued_at))
                }
            })
            .map(|(i, _)| i)?;

        let e = g.entries.remove(idx).unwrap();
        g.members.remove(&e.attempt_id.0);
        if let Some(c) = g.per_run.get_mut(&e.run_id.0) {
            *c = c.saturating_sub(1);
        }
        if let Some(c) = g.per_worker.get_mut(&e.worker_key) {
            *c = c.saturating_sub(1);
        }
        if e.priority >= ComputePriority::Background {
            g.consecutive_background += 1;
        } else {
            g.consecutive_background = 0;
        }
        Some((e.attempt_id, e.run_id, e.task_id, e.priority))
    }

    pub fn contains(&self, attempt_id: &AttemptId) -> bool {
        self.inner.lock().unwrap().members.contains(&attempt_id.0)
    }
}

fn evict_lowest(g: &mut QueueState) -> bool {
    let lowest = g
        .entries
        .iter()
        .enumerate()
        .max_by_key(|(_, e)| e.priority.rank())
        .map(|(i, _)| i);
    let Some(idx) = lowest else {
        return false;
    };
    if let Some(e) = g.entries.remove(idx) {
        g.members.remove(&e.attempt_id.0);
        if let Some(c) = g.per_run.get_mut(&e.run_id.0) {
            *c = c.saturating_sub(1);
        }
        if let Some(c) = g.per_worker.get_mut(&e.worker_key) {
            *c = c.saturating_sub(1);
        }
        true
    } else {
        false
    }
}

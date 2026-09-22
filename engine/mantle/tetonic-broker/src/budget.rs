//! Hierarchical budgets, resource requests, and reservations (M6-1).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tetonic_domain::ids::{AttemptId, ReservationId, RunId, TaskId, WorkerId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ResourceAmount {
    pub milli_cores: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GpuRequirement {
    pub device_index: Option<u32>,
    pub exclusive: bool,
    pub min_vram_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DurationEstimate {
    pub expected_ms: u64,
    pub hard_limit_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceRequest {
    pub cpu_cores: ResourceAmount,
    pub memory_bytes: u64,
    pub gpu_devices: Vec<GpuRequirement>,
    pub vram_bytes: u64,
    pub process_slots: u32,
    pub inference_slots: u32,
    pub input_tokens: u64,
    pub maximum_output_tokens: u64,
    pub total_token_budget: u64,
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub temporary_storage_bytes: u64,
    pub estimated_duration: DurationEstimate,
}

impl Default for ResourceRequest {
    fn default() -> Self {
        Self {
            cpu_cores: ResourceAmount { milli_cores: 500 },
            memory_bytes: 512 * 1024 * 1024,
            gpu_devices: vec![],
            vram_bytes: 0,
            process_slots: 0,
            inference_slots: 1,
            input_tokens: 4_096,
            maximum_output_tokens: 4_096,
            total_token_budget: 8_192,
            input_bytes: 256 * 1024,
            output_bytes: 256 * 1024,
            temporary_storage_bytes: 64 * 1024 * 1024,
            estimated_duration: DurationEstimate {
                expected_ms: 30_000,
                hard_limit_ms: 300_000,
            },
        }
    }
}

impl ResourceRequest {
    /// Conservative default for Infer jobs when the caller omits a profile.
    pub fn infer_default() -> Self {
        Self::default()
    }

    /// One spawned specialist context competing for Infer concurrency (H2-3).
    ///
    /// Holds an `inference_slots` unit (and a small token budget) so fan-out is
    /// paid for on the ledger before the child agent is built — independent of
    /// the child's later Infer reservations.
    pub fn spawned_agent_default() -> Self {
        Self {
            inference_slots: 1,
            input_tokens: 2_048,
            maximum_output_tokens: 2_048,
            total_token_budget: 4_096,
            memory_bytes: 256 * 1024 * 1024,
            vram_bytes: 0,
            process_slots: 0,
            ..Self::default()
        }
    }

    pub fn exceeds_hard_limits(&self, limits: &BudgetLimits) -> bool {
        self.memory_bytes > limits.max_memory_bytes_per_attempt
            || self.vram_bytes > limits.max_vram_bytes_per_attempt
            || self.total_token_budget > limits.max_tokens_per_task
            || self.process_slots > limits.max_processes_per_attempt
            || self.inference_slots > limits.max_inference_slots_per_attempt
            || self.temporary_storage_bytes > limits.max_temp_storage_bytes
    }
}

#[derive(Debug, Clone)]
pub struct BudgetLimits {
    pub max_active_tasks_per_run: u32,
    pub max_attempts_per_task: u32,
    pub max_local_processes: u32,
    pub max_processes_per_worker: u32,
    pub max_processes_per_attempt: u32,
    pub max_concurrent_inference: u32,
    pub max_inference_slots_per_attempt: u32,
    pub max_memory_bytes_global: u64,
    pub max_memory_bytes_per_run: u64,
    pub max_memory_bytes_per_attempt: u64,
    pub max_vram_bytes_global: u64,
    pub max_vram_bytes_per_worker: u64,
    pub max_vram_bytes_per_attempt: u64,
    pub max_tokens_per_task: u64,
    pub max_tokens_per_run: u64,
    pub max_temp_storage_bytes: u64,
    pub max_speculation_slots: u32,
    pub reservation_ttl: Duration,
}

impl Default for BudgetLimits {
    fn default() -> Self {
        Self {
            max_active_tasks_per_run: 32,
            max_attempts_per_task: 8,
            max_local_processes: 16,
            max_processes_per_worker: 8,
            max_processes_per_attempt: 2,
            max_concurrent_inference: 8,
            max_inference_slots_per_attempt: 2,
            max_memory_bytes_global: 16 * 1024 * 1024 * 1024,
            max_memory_bytes_per_run: 4 * 1024 * 1024 * 1024,
            max_memory_bytes_per_attempt: 2 * 1024 * 1024 * 1024,
            max_vram_bytes_global: 24 * 1024 * 1024 * 1024,
            max_vram_bytes_per_worker: 24 * 1024 * 1024 * 1024,
            max_vram_bytes_per_attempt: 24 * 1024 * 1024 * 1024,
            max_tokens_per_task: 200_000,
            max_tokens_per_run: 2_000_000,
            max_temp_storage_bytes: 8 * 1024 * 1024 * 1024,
            max_speculation_slots: 4,
            reservation_ttl: Duration::from_secs(300),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReservationTarget {
    Local,
    Worker { worker_id: WorkerId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReservedResources {
    pub memory_bytes: u64,
    pub vram_bytes: u64,
    pub process_slots: u32,
    pub inference_slots: u32,
    pub token_budget: u64,
    pub temporary_storage_bytes: u64,
    pub exclusive_gpu_indices: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReservationState {
    Proposed,
    Reserved,
    Dispatched,
    Running,
    Released,
    Expired,
    Canceled,
    OverBudget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceReservation {
    pub reservation_id: ReservationId,
    pub run_id: RunId,
    pub task_id: TaskId,
    pub attempt_id: AttemptId,
    pub target_scope: ReservationTarget,
    pub resources: ReservedResources,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub reservation_epoch: u64,
    pub state: ReservationState,
    #[serde(default)]
    pub speculative: bool,
}

#[derive(Debug, Clone, Default)]
struct ScopeUsage {
    memory_bytes: u64,
    vram_bytes: u64,
    process_slots: u32,
    inference_slots: u32,
    token_budget: u64,
    active_tasks: u32,
    speculation_slots: u32,
    exclusive_gpus: HashMap<u32, AttemptId>,
}

struct BudgetState {
    limits: BudgetLimits,
    global: ScopeUsage,
    projects: HashMap<String, ScopeUsage>,
    runs: HashMap<String, ScopeUsage>,
    tasks: HashMap<String, ScopeUsage>,
    workers: HashMap<String, ScopeUsage>,
    reservations: HashMap<String, ResourceReservation>,
    by_attempt: HashMap<String, String>,
    epoch: u64,
}

/// Hierarchical budget ledger — every request must fit every applicable scope.
pub struct HierarchicalBudgetLedger {
    inner: Mutex<BudgetState>,
}

impl HierarchicalBudgetLedger {
    pub fn new(limits: BudgetLimits) -> Self {
        Self {
            inner: Mutex::new(BudgetState {
                limits,
                global: ScopeUsage::default(),
                projects: HashMap::new(),
                runs: HashMap::new(),
                tasks: HashMap::new(),
                workers: HashMap::new(),
                reservations: HashMap::new(),
                by_attempt: HashMap::new(),
                epoch: 1,
            }),
        }
    }

    pub fn limits(&self) -> BudgetLimits {
        self.inner.lock().unwrap().limits.clone()
    }

    pub fn get_by_attempt(&self, attempt_id: &AttemptId) -> Option<ResourceReservation> {
        let g = self.inner.lock().unwrap();
        let rid = g.by_attempt.get(&attempt_id.0)?;
        g.reservations.get(rid).cloned()
    }

    pub fn get(&self, reservation_id: &ReservationId) -> Option<ResourceReservation> {
        self.inner
            .lock()
            .unwrap()
            .reservations
            .get(&reservation_id.0)
            .cloned()
    }

    pub fn check_run_spawn_limit(&self, run_id: &RunId) -> Result<(), BudgetReject> {
        let g = self.inner.lock().unwrap();
        if let Some(run_u) = g.runs.get(&run_id.0) {
            if run_u.active_tasks >= g.limits.max_active_tasks_per_run {
                return Err(BudgetReject::RunConcurrencyLimit);
            }
        }
        Ok(())
    }

    /// Idempotent reserve bound to one attempt.
    #[allow(clippy::too_many_arguments)]
    pub fn reserve(
        &self,
        run_id: &RunId,
        task_id: &TaskId,
        attempt_id: &AttemptId,
        project_id: Option<&str>,
        target: ReservationTarget,
        request: &ResourceRequest,
        speculative: bool,
        now: DateTime<Utc>,
    ) -> Result<ResourceReservation, BudgetReject> {
        let mut g = self.inner.lock().unwrap();
        if let Some(existing_id) = g.by_attempt.get(&attempt_id.0) {
            if let Some(existing) = g.reservations.get(existing_id) {
                if matches!(
                    existing.state,
                    ReservationState::Reserved
                        | ReservationState::Dispatched
                        | ReservationState::Running
                ) {
                    return Ok(existing.clone());
                }
            }
        }
        let limits = g.limits.clone();
        if request.exceeds_hard_limits(&limits) {
            return Err(BudgetReject::ResourceRequestTooLarge);
        }
        Self::expire_in(&mut g, now);

        let worker_key = match &target {
            ReservationTarget::Local => "local".to_string(),
            ReservationTarget::Worker { worker_id } => worker_id.0.clone(),
        };

        if speculative && g.global.speculation_slots >= limits.max_speculation_slots {
            return Err(BudgetReject::SpeculationBudgetExceeded);
        }

        {
            let run_u = g.runs.entry(run_id.0.clone()).or_default();
            if run_u.active_tasks >= limits.max_active_tasks_per_run {
                return Err(BudgetReject::RunConcurrencyLimit);
            }
            if run_u.token_budget + request.total_token_budget > limits.max_tokens_per_run {
                return Err(BudgetReject::TokenBudgetExceeded);
            }
            if run_u.memory_bytes + request.memory_bytes > limits.max_memory_bytes_per_run {
                return Err(BudgetReject::MemoryBudgetExceeded);
            }
        }

        {
            let task_u = g.tasks.entry(task_id.0.clone()).or_default();
            if task_u.active_tasks >= limits.max_attempts_per_task {
                return Err(BudgetReject::RunConcurrencyLimit);
            }
            if task_u.token_budget + request.total_token_budget > limits.max_tokens_per_task {
                return Err(BudgetReject::TokenBudgetExceeded);
            }
        }

        if g.global.inference_slots + request.inference_slots > limits.max_concurrent_inference {
            return Err(BudgetReject::WorkerConcurrencyLimit);
        }
        if g.global.memory_bytes + request.memory_bytes > limits.max_memory_bytes_global {
            return Err(BudgetReject::MemoryBudgetExceeded);
        }
        if g.global.vram_bytes + request.vram_bytes > limits.max_vram_bytes_global {
            return Err(BudgetReject::VramBudgetExceeded);
        }
        if g.global.process_slots + request.process_slots > limits.max_local_processes {
            return Err(BudgetReject::ProcessLimitExceeded);
        }

        {
            let worker_u = g.workers.entry(worker_key.clone()).or_default();
            if worker_u.process_slots + request.process_slots > limits.max_processes_per_worker {
                return Err(BudgetReject::ProcessLimitExceeded);
            }
            if worker_u.vram_bytes + request.vram_bytes > limits.max_vram_bytes_per_worker {
                return Err(BudgetReject::VramBudgetExceeded);
            }
            if worker_u.inference_slots + request.inference_slots > limits.max_concurrent_inference
            {
                return Err(BudgetReject::WorkerConcurrencyLimit);
            }
            for gpu in &request.gpu_devices {
                if let Some(idx) = gpu.device_index {
                    if gpu.exclusive {
                        if let Some(holder) = worker_u.exclusive_gpus.get(&idx) {
                            if holder != attempt_id {
                                return Err(BudgetReject::VramBudgetExceeded);
                            }
                        }
                    }
                }
            }
        }

        if let Some(pid) = project_id {
            let p = g.projects.entry(pid.to_string()).or_default();
            if p.memory_bytes + request.memory_bytes > limits.max_memory_bytes_global {
                return Err(BudgetReject::MemoryBudgetExceeded);
            }
        }

        let resources = ReservedResources {
            memory_bytes: request.memory_bytes,
            vram_bytes: request.vram_bytes,
            process_slots: request.process_slots,
            inference_slots: request.inference_slots,
            token_budget: request.total_token_budget,
            temporary_storage_bytes: request.temporary_storage_bytes,
            exclusive_gpu_indices: request
                .gpu_devices
                .iter()
                .filter(|g| g.exclusive)
                .filter_map(|g| g.device_index)
                .collect(),
        };

        apply_usage(&mut g.global, &resources, speculative, true, attempt_id);
        apply_usage(
            g.runs.entry(run_id.0.clone()).or_default(),
            &resources,
            speculative,
            true,
            attempt_id,
        );
        apply_usage(
            g.tasks.entry(task_id.0.clone()).or_default(),
            &resources,
            speculative,
            true,
            attempt_id,
        );
        apply_usage(
            g.workers.entry(worker_key).or_default(),
            &resources,
            speculative,
            true,
            attempt_id,
        );
        if let Some(pid) = project_id {
            apply_usage(
                g.projects.entry(pid.to_string()).or_default(),
                &resources,
                speculative,
                true,
                attempt_id,
            );
        }

        g.epoch += 1;
        let reservation_id = ReservationId::new(format!("res_{}", uuid::Uuid::new_v4()));
        let ttl = limits.reservation_ttl;
        let reservation = ResourceReservation {
            reservation_id: reservation_id.clone(),
            run_id: run_id.clone(),
            task_id: task_id.clone(),
            attempt_id: attempt_id.clone(),
            target_scope: target,
            resources,
            issued_at: now,
            expires_at: now
                + chrono::Duration::from_std(ttl)
                    .unwrap_or_else(|_| chrono::Duration::seconds(300)),
            reservation_epoch: g.epoch,
            state: ReservationState::Reserved,
            speculative,
        };
        g.by_attempt
            .insert(attempt_id.0.clone(), reservation_id.0.clone());
        g.reservations
            .insert(reservation_id.0.clone(), reservation.clone());
        Ok(reservation)
    }

    pub fn transition(
        &self,
        reservation_id: &ReservationId,
        state: ReservationState,
    ) -> Result<(), BudgetReject> {
        let mut g = self.inner.lock().unwrap();
        let Some(rec) = g.reservations.get_mut(&reservation_id.0) else {
            return Err(BudgetReject::UnknownReservation);
        };
        if matches!(
            rec.state,
            ReservationState::Released | ReservationState::Expired | ReservationState::Canceled
        ) {
            return Ok(());
        }
        rec.state = state;
        Ok(())
    }

    pub fn release(&self, reservation_id: &ReservationId, terminal: ReservationState) {
        let mut g = self.inner.lock().unwrap();
        let Some(rec) = g.reservations.get(&reservation_id.0).cloned() else {
            return;
        };
        if matches!(
            rec.state,
            ReservationState::Released | ReservationState::Expired | ReservationState::Canceled
        ) {
            return;
        }
        reverse_usage(
            &mut g.global,
            &rec.resources,
            rec.speculative,
            &rec.attempt_id,
        );
        if let Some(u) = g.runs.get_mut(&rec.run_id.0) {
            reverse_usage(u, &rec.resources, rec.speculative, &rec.attempt_id);
        }
        if let Some(u) = g.tasks.get_mut(&rec.task_id.0) {
            reverse_usage(u, &rec.resources, rec.speculative, &rec.attempt_id);
        }
        let worker_key = match &rec.target_scope {
            ReservationTarget::Local => "local".to_string(),
            ReservationTarget::Worker { worker_id } => worker_id.0.clone(),
        };
        if let Some(u) = g.workers.get_mut(&worker_key) {
            reverse_usage(u, &rec.resources, rec.speculative, &rec.attempt_id);
        }
        if let Some(rec_mut) = g.reservations.get_mut(&reservation_id.0) {
            rec_mut.state = terminal;
        }
        g.by_attempt.remove(&rec.attempt_id.0);
    }

    pub fn release_attempt(&self, attempt_id: &AttemptId, terminal: ReservationState) {
        let id = {
            let g = self.inner.lock().unwrap();
            g.by_attempt.get(&attempt_id.0).cloned()
        };
        if let Some(rid) = id {
            self.release(&ReservationId::new(rid), terminal);
        }
    }

    /// Release every active reservation for a run (parent CancelRun / session cancel).
    pub fn release_for_run(&self, run_id: &RunId, terminal: ReservationState) -> usize {
        let ids: Vec<_> = {
            let g = self.inner.lock().unwrap();
            g.reservations
                .values()
                .filter(|r| {
                    r.run_id == *run_id
                        && matches!(
                            r.state,
                            ReservationState::Reserved
                                | ReservationState::Dispatched
                                | ReservationState::Running
                                | ReservationState::Proposed
                        )
                })
                .map(|r| r.reservation_id.clone())
                .collect()
        };
        let n = ids.len();
        for id in ids {
            self.release(&id, terminal);
        }
        n
    }

    /// Release every active reservation targeting a worker (worker loss / revoke).
    pub fn release_for_worker(&self, worker_id: &WorkerId, terminal: ReservationState) -> usize {
        let ids: Vec<_> = {
            let g = self.inner.lock().unwrap();
            g.reservations
                .values()
                .filter(|r| {
                    matches!(
                        &r.target_scope,
                        ReservationTarget::Worker { worker_id: w } if w.0 == worker_id.0
                    ) && matches!(
                        r.state,
                        ReservationState::Reserved
                            | ReservationState::Dispatched
                            | ReservationState::Running
                    )
                })
                .map(|r| r.reservation_id.clone())
                .collect()
        };
        let n = ids.len();
        for rid in ids {
            self.release(&rid, terminal);
        }
        n
    }

    /// When measured use exceeds the reserved envelope, mark OverBudget and release.
    pub fn enforce_actual_usage(
        &self,
        attempt_id: &AttemptId,
        actual: &ReservedResources,
    ) -> Result<(), BudgetReject> {
        let Some(res) = self.get_by_attempt(attempt_id) else {
            return Err(BudgetReject::UnknownReservation);
        };
        let over = actual.memory_bytes > res.resources.memory_bytes
            || actual.vram_bytes > res.resources.vram_bytes
            || actual.process_slots > res.resources.process_slots
            || actual.inference_slots > res.resources.inference_slots
            || actual.token_budget > res.resources.token_budget
            || actual.temporary_storage_bytes > res.resources.temporary_storage_bytes;
        if over {
            self.release(&res.reservation_id, ReservationState::OverBudget);
            return Err(BudgetReject::OverBudget);
        }
        Ok(())
    }

    pub fn reconcile_uncertain(&self, now: DateTime<Utc>) -> usize {
        let mut g = self.inner.lock().unwrap();
        Self::expire_in(&mut g, now);
        let uncertain: Vec<_> = g
            .reservations
            .values()
            .filter(|r| {
                matches!(
                    r.state,
                    ReservationState::Dispatched | ReservationState::Running
                )
            })
            .map(|r| r.reservation_id.clone())
            .collect();
        let n = uncertain.len();
        for rid in uncertain {
            // Fail-closed: do not double-reserve after restart until confirmed.
            drop_reservation_locked(&mut g, &rid, ReservationState::Canceled);
        }
        n
    }

    pub fn active_count(&self) -> usize {
        self.inner
            .lock()
            .unwrap()
            .reservations
            .values()
            .filter(|r| {
                matches!(
                    r.state,
                    ReservationState::Reserved
                        | ReservationState::Dispatched
                        | ReservationState::Running
                )
            })
            .count()
    }

    fn expire_in(g: &mut BudgetState, now: DateTime<Utc>) {
        let expired: Vec<_> = g
            .reservations
            .values()
            .filter(|r| {
                matches!(
                    r.state,
                    ReservationState::Reserved | ReservationState::Proposed
                ) && r.expires_at <= now
            })
            .map(|r| r.reservation_id.clone())
            .collect();
        for rid in expired {
            drop_reservation_locked(g, &rid, ReservationState::Expired);
        }
    }
}

fn drop_reservation_locked(g: &mut BudgetState, rid: &ReservationId, terminal: ReservationState) {
    let Some(rec) = g.reservations.get(&rid.0).cloned() else {
        return;
    };
    if matches!(
        rec.state,
        ReservationState::Released | ReservationState::Expired | ReservationState::Canceled
    ) {
        return;
    }
    reverse_usage(
        &mut g.global,
        &rec.resources,
        rec.speculative,
        &rec.attempt_id,
    );
    if let Some(u) = g.runs.get_mut(&rec.run_id.0) {
        reverse_usage(u, &rec.resources, rec.speculative, &rec.attempt_id);
    }
    if let Some(u) = g.tasks.get_mut(&rec.task_id.0) {
        reverse_usage(u, &rec.resources, rec.speculative, &rec.attempt_id);
    }
    let worker_key = match &rec.target_scope {
        ReservationTarget::Local => "local".to_string(),
        ReservationTarget::Worker { worker_id } => worker_id.0.clone(),
    };
    if let Some(u) = g.workers.get_mut(&worker_key) {
        reverse_usage(u, &rec.resources, rec.speculative, &rec.attempt_id);
    }
    if let Some(rec_mut) = g.reservations.get_mut(&rid.0) {
        rec_mut.state = terminal;
    }
    g.by_attempt.remove(&rec.attempt_id.0);
}

fn apply_usage(
    usage: &mut ScopeUsage,
    resources: &ReservedResources,
    speculative: bool,
    count_task: bool,
    attempt_id: &AttemptId,
) {
    usage.memory_bytes += resources.memory_bytes;
    usage.vram_bytes += resources.vram_bytes;
    usage.process_slots += resources.process_slots;
    usage.inference_slots += resources.inference_slots;
    usage.token_budget += resources.token_budget;
    if count_task {
        usage.active_tasks += 1;
    }
    if speculative {
        usage.speculation_slots += 1;
    }
    for idx in &resources.exclusive_gpu_indices {
        usage.exclusive_gpus.insert(*idx, attempt_id.clone());
    }
}

fn reverse_usage(
    usage: &mut ScopeUsage,
    resources: &ReservedResources,
    speculative: bool,
    attempt_id: &AttemptId,
) {
    usage.memory_bytes = usage.memory_bytes.saturating_sub(resources.memory_bytes);
    usage.vram_bytes = usage.vram_bytes.saturating_sub(resources.vram_bytes);
    usage.process_slots = usage.process_slots.saturating_sub(resources.process_slots);
    usage.inference_slots = usage
        .inference_slots
        .saturating_sub(resources.inference_slots);
    usage.token_budget = usage.token_budget.saturating_sub(resources.token_budget);
    usage.active_tasks = usage.active_tasks.saturating_sub(1);
    if speculative {
        usage.speculation_slots = usage.speculation_slots.saturating_sub(1);
    }
    usage
        .exclusive_gpus
        .retain(|_, holder| holder != attempt_id);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetReject {
    ResourceRequestTooLarge,
    RunConcurrencyLimit,
    WorkerConcurrencyLimit,
    MemoryBudgetExceeded,
    VramBudgetExceeded,
    ProcessLimitExceeded,
    TokenBudgetExceeded,
    SpeculationBudgetExceeded,
    UnknownReservation,
    OverBudget,
}

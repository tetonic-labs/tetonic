//! Durable run supervisor — sole authority for run/task/attempt state (M3-1).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tetonic_domain::{
    EventActor, EventType, RunCommand, RunCommandResult, RunEventEnvelope, RunId, RunSnapshot,
    RunState, RunSupervisorError,
};
use tetonic_memory::{RecoverMutex, SharedStore};
use tokio::sync::Mutex as AsyncMutex;
use tracing::debug;

use crate::metrics::{emit_failure_trace, metric_command_replay};
use crate::payload_digest::digest_event_payload;
use crate::recovery::{detect_recovery_required, recover_expired_leases};
use crate::replay::{empty_snapshot, replay_from_events};
use crate::transition::{apply_command, event_type_for, failure_class_for_command};

pub trait RunEventHook: Send + Sync {
    fn on_run_committed(&self, result: &RunCommandResult);
}

#[async_trait]
pub trait RunSupervisor: Send + Sync {
    async fn handle(&self, command: RunCommand) -> Result<RunCommandResult, RunSupervisorError>;
    async fn snapshot(&self, run_id: RunId) -> Result<RunSnapshot, RunSupervisorError>;
    async fn resume_from_sequence(
        &self,
        run_id: RunId,
        after_sequence: u64,
        limit: u32,
    ) -> Result<Result<Vec<RunEventEnvelope>, tetonic_domain::ReplayGap>, RunSupervisorError>;
    fn recovery_required(&self) -> bool {
        false
    }
    /// Why recovery is required, including the offending run when known.
    ///
    /// Interrupted runs remain quarantined until an operator explicitly abandons
    /// the inspected revision. Storage failures still require global Safe Mode.
    fn recovery_required_reason(&self) -> Option<String> {
        None
    }
    fn enter_safe_mode(&self, _reason: &str) {}
    fn is_safe_mode(&self) -> bool {
        false
    }
}

pub struct DurableRunSupervisor {
    store: Option<SharedStore>,
    memory: Mutex<HashMap<String, RunSnapshot>>,
    command_dedup: Mutex<HashMap<String, HashMap<String, RunCommandResult>>>,
    run_locks: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
    hooks: Vec<Arc<dyn RunEventHook>>,
    pub quotas: Arc<crate::quotas::StorageQuotaManager>,
    pub migration: Arc<crate::migration::MigrationManager>,
}

impl DurableRunSupervisor {
    pub fn new(store: Option<SharedStore>) -> Self {
        let migration = Arc::new(crate::migration::MigrationManager::new());
        if let Some(ref s) = store {
            migration.set_db_path(s.path().to_path_buf());
        }
        let sup = Self {
            store,
            memory: Mutex::new(HashMap::new()),
            command_dedup: Mutex::new(HashMap::new()),
            run_locks: Mutex::new(HashMap::new()),
            hooks: Vec::new(),
            quotas: Arc::new(crate::quotas::StorageQuotaManager::default()),
            migration,
        };
        if let Err(e) = sup.recover_at_startup() {
            // Fail-closed (M6, INV-RUN-003). The reason names the run because
            // `RunState::RecoveryRequired` has no code path that clears it
            // (M6 CONVERGE C-B): an operator seeing Safe Mode needs to know
            // which run provoked it and that it will not resolve itself.
            sup.migration
                .enter_safe_mode(&format!("startup recovery failed, not self-clearing: {e}"));
        }
        sup
    }

    pub fn with_hook(mut self, hook: Arc<dyn RunEventHook>) -> Self {
        self.hooks.push(hook);
        self
    }

    pub fn with_quotas(mut self, quotas: Arc<crate::quotas::StorageQuotaManager>) -> Self {
        self.quotas = quotas;
        self
    }

    fn run_lock(&self, run_id: &RunId) -> Arc<AsyncMutex<()>> {
        let mut locks = self.run_locks.lock_recover();
        locks
            .entry(run_id.0.clone())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }

    fn load_snapshot(&self, run_id: &RunId) -> Result<Option<RunSnapshot>, RunSupervisorError> {
        if let Some(store) = &self.store {
            store
                .read_sync({
                    let run_id = run_id.0.clone();
                    move |db| {
                        db.load_run_snapshot(&run_id)
                            .map_err(|e| RunSupervisorError::Persistence(e.to_string()))
                    }
                })
                .map_err(|e| RunSupervisorError::Persistence(e.to_string()))?
        } else {
            Ok(self.memory.lock_recover().get(&run_id.0).cloned())
        }
    }

    async fn load_snapshot_async(
        &self,
        run_id: &RunId,
    ) -> Result<Option<RunSnapshot>, RunSupervisorError> {
        if let Some(store) = &self.store {
            store
                .read({
                    let run_id = run_id.0.clone();
                    move |db| {
                        db.load_run_snapshot(&run_id)
                            .map_err(|e| RunSupervisorError::Persistence(e.to_string()))
                    }
                })
                .await
                .map_err(|e| RunSupervisorError::Persistence(e.to_string()))?
        } else {
            Ok(self.memory.lock_recover().get(&run_id.0).cloned())
        }
    }

    async fn persist_async(
        &self,
        snapshot: &RunSnapshot,
        event: &RunEventEnvelope,
        idempotency: Option<(&str, &RunCommandResult)>,
    ) -> Result<(), RunSupervisorError> {
        if let Some(store) = &self.store {
            store
                .write({
                    let snapshot = snapshot.clone();
                    let event = event.clone();
                    let idempotency = idempotency.map(|(k, v)| (k.to_string(), v.clone()));
                    move |db| {
                        db.commit_run_command(
                            &snapshot,
                            &event,
                            idempotency.as_ref().map(|(k, v)| (k.as_str(), v)),
                        )
                        .map_err(|e| match e {
                            tetonic_memory::StoreError::OrganizationCapacityExceeded => RunSupervisorError::OrganizationCapacityExceeded,
                            tetonic_memory::StoreError::TeamCapacityExceeded => RunSupervisorError::TeamCapacityExceeded,
                            tetonic_memory::StoreError::PrincipalCapacityExceeded => RunSupervisorError::PrincipalCapacityExceeded,
                            tetonic_memory::StoreError::ExecutionCapacityExceeded => {
                                RunSupervisorError::ExecutionCapacityExceeded
                            }
                            other => RunSupervisorError::Persistence(other.to_string()),
                        })
                    }
                })
                .await
                .map_err(|e| RunSupervisorError::Persistence(e.to_string()))??;
        } else {
            self.memory
                .lock_recover()
                .insert(snapshot.run_id.0.clone(), snapshot.clone());
        }
        Ok(())
    }

    /// Restart recovery, fail-closed (M6, INV-RUN-003).
    ///
    /// Every stage propagates. Before M6 all four discarded their outcome, and
    /// the last one mattered most: the write below is what persists
    /// `RecoveryRequired`, and `recovery_required()` re-reads that state from
    /// the store to decide whether production enters Safe Mode. Dropping the
    /// write therefore silenced its own detector.
    ///
    /// "`RecoveryRequired` survives restart" does not depend on this write
    /// succeeding. `detect_recovery_required` is a pure function of the
    /// persisted snapshot, and the write is one transaction, so a failed write
    /// leaves the triggering condition on disk and the next start derives the
    /// same answer. The flag is a memo of those conditions, not an independent
    /// fact — which is why `mark_recovery_required`, a setter with no detector
    /// guard, was deleted in M6 rather than left available.
    ///
    /// Recovery persists the **projection only**. Before M6 it appended a
    /// `recovery.scan` event through `commit_run_command` at the snapshot's
    /// current sequence, which could not work: `run_events` has
    /// `UNIQUE (run_id, sequence)` and the tip sequence is already occupied by
    /// the event that produced it, so the write failed with a constraint
    /// violation on every run that had any history. `let _ =` hid a total
    /// failure rate. Had it succeeded it would have been worse — replay
    /// deserializes every event payload into a `RunCommand` and refuses on a
    /// sequence gap, and `"recovery"` is not a command, so a landed
    /// `recovery.scan` would have made the run unreplayable. Recovery state is
    /// projection state, so it is written as projection state.
    fn recover_at_startup(&self) -> Result<(), RunSupervisorError> {
        let run_ids: Vec<String> = if let Some(store) = &self.store {
            store
                .read_sync(|db| db.list_all_run_ids())
                .map_err(|e| RunSupervisorError::Persistence(e.to_string()))?
                .map_err(|e| RunSupervisorError::Persistence(e.to_string()))?
        } else {
            self.memory.lock_recover().keys().cloned().collect()
        };
        let now = chrono_now();
        for run_id in run_ids {
            let loaded = self
                .load_snapshot(&RunId::new(&run_id))
                .map_err(|e| RunSupervisorError::Persistence(format!("run {run_id}: {e}")))?;
            let Some(mut snap) = loaded else {
                continue;
            };
            let before = snap.clone();
            recover_expired_leases(&mut snap, now)?;
            if detect_recovery_required(&snap, now) && snap.state != RunState::RecoveryRequired {
                snap.state = RunState::RecoveryRequired;
            }
            // Persist only what the pass actually changed. Writing on every
            // start would rewrite every run's projection on every boot, and a
            // fail-closed write that has nothing to say would turn any
            // transient store fault into Safe Mode for no reason.
            if snap == before {
                continue;
            }
            if let Some(store) = &self.store {
                store
                    .write_sync({
                        let snap = snap.clone();
                        move |db| db.persist_run_projection(&snap)
                    })
                    .map_err(|e| RunSupervisorError::Persistence(format!("run {run_id}: {e}")))?
                    .map_err(|e| RunSupervisorError::Persistence(format!("run {run_id}: {e}")))?;
            } else {
                self.memory.lock_recover().insert(run_id, snap);
            }
        }
        Ok(())
    }

    async fn handle_internal(
        &self,
        command: RunCommand,
    ) -> Result<RunCommandResult, RunSupervisorError> {
        if self.migration.is_safe_mode() && !matches!(command, RunCommand::FinishRun(_)) {
            return Err(RunSupervisorError::Conflict(
                "System is in Safe Mode. Mutating commands are rejected.".into(),
            ));
        }
        let envelope = command.envelope().clone();
        let run_id = match &command {
            RunCommand::CreateRun(c) => c.run_id.clone(),
            _ => command
                .run_id()
                .cloned()
                .ok_or_else(|| RunSupervisorError::RunNotFound("missing run_id".into()))?,
        };

        if let Some(hit) = self
            .check_command_id_async(&run_id, &envelope.command_id)
            .await?
        {
            if matches!(command, RunCommand::ClaimExecution(_)) {
                return Err(RunSupervisorError::DuplicateDelivery(
                    "execution permission cannot be replayed".into(),
                ));
            }
            metric_command_replay(&hit);
            return Ok(hit);
        }

        if let Some(key) = &envelope.idempotency_key {
            if let Some(hit) = self.check_idempotency_async(&run_id, key).await? {
                if matches!(command, RunCommand::ClaimExecution(_)) {
                    return Err(RunSupervisorError::DuplicateDelivery(
                        "execution permission cannot be replayed".into(),
                    ));
                }
                return Ok(hit);
            }
        }

        let current = self.load_snapshot_async(&run_id).await?;
        let base = match (&command, current) {
            (RunCommand::CreateRun(c), None) => {
                empty_snapshot(c.run_id.clone(), c.session_id.clone())
            }
            (RunCommand::CreateRun(_), Some(_)) => {
                return Err(RunSupervisorError::Conflict(format!(
                    "run {} exists",
                    run_id
                )));
            }
            (_, None) => return Err(RunSupervisorError::RunNotFound(run_id.to_string())),
            (_, Some(s)) => s,
        };

        if let Some(expected) = envelope.expected_sequence {
            if expected != base.sequence {
                return Err(RunSupervisorError::StaleSequence {
                    expected,
                    actual: base.sequence,
                });
            }
        }

        if base.state == RunState::RecoveryRequired
            && !(matches!(command, RunCommand::CancelRun(_))
                && envelope.expected_sequence == Some(base.sequence))
        {
            return Err(RunSupervisorError::RecoveryRequired);
        }

        if (base.state == RunState::Canceled
            || base.state == RunState::Failed
            || base.state == RunState::Succeeded)
            && !matches!(
                command,
                RunCommand::FinishRun(_)
                    | RunCommand::CreateRun(_)
                    | RunCommand::RecordAttemptQuiescence(_)
            )
        {
            return Err(RunSupervisorError::RunNotAccepting(base.state.clone()));
        }

        let mut next = apply_command(&base, &command)?;
        next.sequence = base.sequence + 1;
        let payload = serde_json::to_value(&command).unwrap();
        let event = RunEventEnvelope {
            event_id: tetonic_domain::ids::EventId::new(uuid::Uuid::new_v4().to_string()),
            run_id: next.run_id.clone(),
            sequence: next.sequence,
            event_type: EventType::Other(event_type_for(&command).to_string()),
            schema_version: 1,
            command_id: Some(tetonic_domain::ids::CommandId::new(
                envelope.command_id.clone(),
            )),
            causation_id: None,
            correlation_id: Some(tetonic_domain::ids::TraceId::new(
                envelope.trace.trace_id.clone(),
            )),
            actor: EventActor {
                name: "system".into(),
            },
            occurred_at: chrono::Utc::now(),
            recorded_at: chrono::Utc::now(),
            data_class: Default::default(),
            payload_digest: digest_event_payload(&payload)
                .map_err(|e| RunSupervisorError::Persistence(e.to_string()))?,
            payload,
        };
        next.events.push(event.clone());

        let result = RunCommandResult {
            run_id: next.run_id.clone(),
            sequence: next.sequence,
            idempotent_replay: false,
            snapshot: next.clone(),
        };

        let payload_size = event.payload.to_string().len() as u64;
        let should_compact = match self.quotas.check_before_append(payload_size) {
            Ok(c) => c,
            Err(e) => {
                self.migration.enter_safe_mode("storage quota exceeded");
                return Err(RunSupervisorError::StorageLimitExceeded(e.to_string()));
            }
        };

        self.persist_async(
            &next,
            &event,
            envelope.idempotency_key.as_deref().map(|k| (k, &result)),
        )
        .await?;

        self.quotas.record_usage(payload_size);

        if should_compact {
            if let Some(store) = &self.store {
                let floor = next.sequence.saturating_sub(100);
                if floor > 0 {
                    let run_id_s = run_id.0.clone();
                    let session_fallback = next.session_id.clone();
                    match store
                        .write({
                            let run_id_s = run_id_s.clone();
                            move |db| -> std::result::Result<
                                tetonic_memory::CompactReport,
                                RunSupervisorError,
                            > {
                                let events = db
                                    .list_run_events(&run_id_s)
                                    .map_err(|e| RunSupervisorError::Persistence(e.to_string()))?;
                                let below: Vec<_> =
                                    events.into_iter().filter(|e| e.sequence < floor).collect();
                                let session_id = below
                                    .first()
                                    .and_then(|e| {
                                        serde_json::from_value::<RunCommand>(e.payload.clone()).ok()
                                    })
                                    .and_then(|c| match c {
                                        RunCommand::CreateRun(cr) => cr.session_id,
                                        _ => None,
                                    })
                                    .or(session_fallback);
                                let recovery = replay_from_events(
                                    &empty_snapshot(RunId::new(&run_id_s), session_id),
                                    &below,
                                )?;
                                db.compact_run_events_at_floor(&run_id_s, floor, &recovery)
                                    .map_err(|e| RunSupervisorError::Persistence(e.to_string()))
                            }
                        })
                        .await
                    {
                        Ok(Ok(report)) => {
                            self.quotas.free_usage(report.freed_payload_bytes);
                        }
                        Ok(Err(e)) => {
                            tracing::warn!(error = %e, "run event compaction failed");
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "run event compaction store write failed");
                        }
                    }
                }
            }
        }

        if self.store.is_none() {
            self.store_command_id_async(&run_id, &envelope.command_id, &result)
                .await?;
        }

        for hook in &self.hooks {
            hook.on_run_committed(&result);
        }
        emit_failure_trace(
            next.events.last().unwrap(),
            failure_class_for_command(&command).as_ref(),
        );
        debug!(run_id = %next.run_id, seq = next.sequence, "run command committed");
        Ok(result)
    }

    #[allow(dead_code)] // sync twin of check_command_id_async
    fn check_command_id(
        &self,
        run_id: &RunId,
        command_id: &str,
    ) -> Result<Option<RunCommandResult>, RunSupervisorError> {
        if let Some(store) = &self.store {
            store
                .read_sync({
                    let run_id = run_id.0.clone();
                    let command_id = command_id.to_string();
                    move |db| {
                        db.load_run_command_dedup(&run_id, &command_id)
                            .map(|r| {
                                r.map(|mut x| {
                                    x.idempotent_replay = true;
                                    x
                                })
                            })
                            .map_err(|e| RunSupervisorError::Persistence(e.to_string()))
                    }
                })
                .map_err(|e| RunSupervisorError::Persistence(e.to_string()))?
        } else {
            Ok(self
                .command_dedup
                .lock_recover()
                .get(&run_id.0)
                .and_then(|m| m.get(command_id))
                .cloned()
                .map(|mut x| {
                    x.idempotent_replay = true;
                    x
                }))
        }
    }

    async fn check_command_id_async(
        &self,
        run_id: &RunId,
        command_id: &str,
    ) -> Result<Option<RunCommandResult>, RunSupervisorError> {
        if let Some(store) = &self.store {
            store
                .read({
                    let run_id = run_id.0.clone();
                    let command_id = command_id.to_string();
                    move |db| {
                        db.load_run_command_dedup(&run_id, &command_id)
                            .map(|r| {
                                r.map(|mut x| {
                                    x.idempotent_replay = true;
                                    x
                                })
                            })
                            .map_err(|e| RunSupervisorError::Persistence(e.to_string()))
                    }
                })
                .await
                .map_err(|e| RunSupervisorError::Persistence(e.to_string()))?
        } else {
            Ok(self
                .command_dedup
                .lock_recover()
                .get(&run_id.0)
                .and_then(|m| m.get(command_id))
                .cloned()
                .map(|mut x| {
                    x.idempotent_replay = true;
                    x
                }))
        }
    }

    #[allow(dead_code)] // sync twin of store_command_id_async
    fn store_command_id(
        &self,
        run_id: &RunId,
        command_id: &str,
        result: &RunCommandResult,
    ) -> Result<(), RunSupervisorError> {
        if let Some(store) = &self.store {
            store
                .write_sync({
                    let run_id = run_id.0.clone();
                    let command_id = command_id.to_string();
                    let result = result.clone();
                    move |db| {
                        db.store_run_command_dedup(&run_id, &command_id, &result)
                            .map_err(|e| RunSupervisorError::Persistence(e.to_string()))
                    }
                })
                .map_err(|e| RunSupervisorError::Persistence(e.to_string()))?
        } else {
            self.command_dedup
                .lock_recover()
                .entry(run_id.0.clone())
                .or_default()
                .insert(command_id.to_string(), result.clone());
            Ok(())
        }
    }

    async fn store_command_id_async(
        &self,
        run_id: &RunId,
        command_id: &str,
        result: &RunCommandResult,
    ) -> Result<(), RunSupervisorError> {
        if let Some(store) = &self.store {
            store
                .write({
                    let run_id = run_id.0.clone();
                    let command_id = command_id.to_string();
                    let result = result.clone();
                    move |db| {
                        db.store_run_command_dedup(&run_id, &command_id, &result)
                            .map_err(|e| RunSupervisorError::Persistence(e.to_string()))
                    }
                })
                .await
                .map_err(|e| RunSupervisorError::Persistence(e.to_string()))?
        } else {
            self.command_dedup
                .lock_recover()
                .entry(run_id.0.clone())
                .or_default()
                .insert(command_id.to_string(), result.clone());
            Ok(())
        }
    }

    #[allow(dead_code)] // sync twin of check_idempotency_async
    fn check_idempotency(
        &self,
        run_id: &RunId,
        key: &str,
    ) -> Result<Option<RunCommandResult>, RunSupervisorError> {
        if let Some(store) = &self.store {
            store
                .read_sync({
                    let run_id = run_id.0.clone();
                    let key = key.to_string();
                    move |db| {
                        db.load_run_idempotency(&run_id, &key)
                            .map(|r| {
                                r.map(|mut x| {
                                    x.idempotent_replay = true;
                                    x
                                })
                            })
                            .map_err(|e| RunSupervisorError::Persistence(e.to_string()))
                    }
                })
                .map_err(|e| RunSupervisorError::Persistence(e.to_string()))?
        } else {
            Ok(None)
        }
    }

    async fn check_idempotency_async(
        &self,
        run_id: &RunId,
        key: &str,
    ) -> Result<Option<RunCommandResult>, RunSupervisorError> {
        if let Some(store) = &self.store {
            store
                .read({
                    let run_id = run_id.0.clone();
                    let key = key.to_string();
                    move |db| {
                        db.load_run_idempotency(&run_id, &key)
                            .map(|r| {
                                r.map(|mut x| {
                                    x.idempotent_replay = true;
                                    x
                                })
                            })
                            .map_err(|e| RunSupervisorError::Persistence(e.to_string()))
                    }
                })
                .await
                .map_err(|e| RunSupervisorError::Persistence(e.to_string()))?
        } else {
            Ok(None)
        }
    }

    pub fn replay_run(&self, run_id: &RunId) -> Result<RunSnapshot, RunSupervisorError> {
        let (floor_base, events) = if let Some(store) = &self.store {
            store
                .read_sync({
                    let run_id = run_id.0.clone();
                    move |db| {
                        let floor = db
                            .load_replay_floor(&run_id)
                            .map_err(|e| RunSupervisorError::Persistence(e.to_string()))?;
                        let events = db
                            .list_run_events(&run_id)
                            .map_err(|e| RunSupervisorError::Persistence(e.to_string()))?;
                        Ok::<_, RunSupervisorError>((floor, events))
                    }
                })
                .map_err(|e| RunSupervisorError::Persistence(e.to_string()))??
        } else if let Some(snap) = self.memory.lock_recover().get(&run_id.0) {
            (None, snap.events.clone())
        } else {
            return Err(RunSupervisorError::RunNotFound(run_id.to_string()));
        };

        let (base, tail) = if let Some((floor, recovery)) = floor_base {
            if floor > 0 {
                let base = recovery.unwrap_or_else(|| empty_snapshot(run_id.clone(), None));
                let tail: Vec<_> = events.into_iter().filter(|e| e.sequence >= floor).collect();
                (base, tail)
            } else {
                let session_id = events
                    .first()
                    .and_then(|e| serde_json::from_value::<RunCommand>(e.payload.clone()).ok())
                    .and_then(|c| match c {
                        RunCommand::CreateRun(cr) => cr.session_id,
                        _ => None,
                    });
                (empty_snapshot(run_id.clone(), session_id), events)
            }
        } else {
            let session_id = events
                .first()
                .and_then(|e| serde_json::from_value::<RunCommand>(e.payload.clone()).ok())
                .and_then(|c| match c {
                    RunCommand::CreateRun(cr) => cr.session_id,
                    _ => None,
                });
            (empty_snapshot(run_id.clone(), session_id), events)
        };
        replay_from_events(&base, &tail)
    }
}

fn chrono_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[async_trait]
impl RunSupervisor for DurableRunSupervisor {
    async fn handle(&self, command: RunCommand) -> Result<RunCommandResult, RunSupervisorError> {
        let run_id = match &command {
            RunCommand::CreateRun(c) => c.run_id.clone(),
            _ => command
                .run_id()
                .cloned()
                .ok_or_else(|| RunSupervisorError::RunNotFound("missing".into()))?,
        };
        let lock = self.run_lock(&run_id);
        let _guard = lock.lock().await;
        self.handle_internal(command).await
    }

    async fn snapshot(&self, run_id: RunId) -> Result<RunSnapshot, RunSupervisorError> {
        let lock = self.run_lock(&run_id);
        let _guard = lock.lock().await;
        self.load_snapshot_async(&run_id)
            .await?
            .ok_or_else(|| RunSupervisorError::RunNotFound(run_id.to_string()))
    }

    async fn resume_from_sequence(
        &self,
        run_id: RunId,
        after_sequence: u64,
        limit: u32,
    ) -> Result<Result<Vec<RunEventEnvelope>, tetonic_domain::ReplayGap>, RunSupervisorError> {
        let lock = self.run_lock(&run_id);
        let _guard = lock.lock().await;

        let all_events = if let Some(store) = &self.store {
            store
                .read({
                    let run_id = run_id.0.clone();
                    move |db| {
                        db.list_run_events(&run_id)
                            .map_err(|e| RunSupervisorError::Persistence(e.to_string()))
                    }
                })
                .await
                .map_err(|e| RunSupervisorError::Persistence(e.to_string()))??
        } else if let Some(snap) = self.memory.lock_recover().get(&run_id.0) {
            snap.events.clone()
        } else {
            return Err(RunSupervisorError::RunNotFound(run_id.to_string()));
        };

        let stored_floor = if let Some(store) = &self.store {
            store
                .read({
                    let run_id = run_id.0.clone();
                    move |db| {
                        db.load_replay_floor(&run_id)
                            .map_err(|e| RunSupervisorError::Persistence(e.to_string()))
                    }
                })
                .await
                .map_err(|e| RunSupervisorError::Persistence(e.to_string()))??
        } else {
            None
        };
        let replay_floor = stored_floor.as_ref().map(|(f, _)| *f).unwrap_or(0);
        if after_sequence > 0 && replay_floor > 0 && after_sequence < replay_floor {
            return Ok(Err(tetonic_domain::ReplayGap {
                requested_after: after_sequence,
                earliest_available: replay_floor,
                snapshot_sequence: stored_floor
                    .as_ref()
                    .and_then(|(_, s)| s.as_ref())
                    .map(|s| s.sequence)
                    .or(Some(replay_floor.saturating_sub(1))),
                reason: tetonic_domain::ReplayGapReason::OlderThanFloor,
            }));
        }

        if all_events.is_empty() {
            return Ok(Ok(Vec::new()));
        }

        let earliest_available = all_events.first().map(|e| e.sequence).unwrap_or(0);
        if after_sequence > 0 && after_sequence < earliest_available && earliest_available > 0 {
            let snap = self.load_snapshot(&run_id)?;
            return Ok(Err(tetonic_domain::ReplayGap {
                requested_after: after_sequence,
                earliest_available,
                snapshot_sequence: snap.map(|s| s.sequence),
                reason: tetonic_domain::ReplayGapReason::OlderThanFloor,
            }));
        }

        let mut result = Vec::new();
        for event in all_events {
            if event.sequence > after_sequence {
                result.push(event);
                if result.len() as u32 >= limit {
                    break;
                }
            }
        }
        Ok(Ok(result))
    }

    /// Whether any run needs recovery. Fail-closed (M6): a store it cannot read
    /// answers `true`.
    ///
    /// This is the detector production consults before entering Safe Mode, so
    /// "cannot tell" must not be reported as "nothing to recover" — that is the
    /// shape that let the dropped recovery write go unnoticed.
    fn recovery_required(&self) -> bool {
        self.recovery_required_reason().is_some()
    }

    fn recovery_required_reason(&self) -> Option<String> {
        let run_ids: Vec<String> = if let Some(store) = &self.store {
            match store.read_sync(|db| db.list_all_run_ids()) {
                Ok(Ok(ids)) => ids,
                Ok(Err(_)) | Err(_) => {
                    return Some("store unreadable at recovery check, not self-clearing".into())
                }
            }
        } else {
            self.memory.lock_recover().keys().cloned().collect()
        };
        for run_id in run_ids {
            match self.load_snapshot(&RunId::new(&run_id)) {
                Ok(Some(snap)) => {
                    if snap.state == RunState::RecoveryRequired {
                        return Some(format!(
                            "run {run_id} is RecoveryRequired, not self-clearing"
                        ));
                    }
                }
                Ok(None) => {}
                Err(_) => {
                    return Some(format!(
                        "run {run_id} snapshot unreadable, not self-clearing"
                    ))
                }
            }
        }
        None
    }

    fn enter_safe_mode(&self, reason: &str) {
        self.migration.enter_safe_mode(reason);
    }

    fn is_safe_mode(&self) -> bool {
        self.migration.is_safe_mode()
    }
}

/// Helper to build command envelope defaults for production wiring.
pub fn command_envelope(
    command_id: impl Into<String>,
    expected_sequence: Option<u64>,
    actor: impl Into<String>,
) -> tetonic_domain::CommandEnvelope {
    tetonic_domain::CommandEnvelope {
        command_id: command_id.into(),
        expected_sequence,
        trace: tetonic_domain::TraceContext::default(),
        actor: actor.into(),
        timestamp: chrono_now(),
        workspace_version: None,
        idempotency_key: None,
    }
}

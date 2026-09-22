//! Reservation persistence and restart recovery (M6-1).

use lokai_domain::ids::{AttemptId, ReservationId, RunId, TaskId};
use lokai_memory::{mutex_lock, SharedStore};

use crate::budget::{ReservationState, ReservationTarget, ReservedResources, ResourceReservation};

#[async_trait::async_trait]
pub trait ReservationStore: Send + Sync {
    fn upsert(&self, reservation: &ResourceReservation) -> Result<(), String>;
    fn get(&self, reservation_id: &str) -> Result<Option<ResourceReservation>, String>;
    fn list_active(&self) -> Result<Vec<ResourceReservation>, String>;
    fn mark_released(&self, reservation_id: &str, state: ReservationState) -> Result<(), String>;

    async fn upsert_async(&self, reservation: &ResourceReservation) -> Result<(), String> {
        self.upsert(reservation)
    }
    async fn get_async(&self, reservation_id: &str) -> Result<Option<ResourceReservation>, String> {
        self.get(reservation_id)
    }
    async fn list_active_async(&self) -> Result<Vec<ResourceReservation>, String> {
        self.list_active()
    }
    async fn mark_released_async(
        &self,
        reservation_id: &str,
        state: ReservationState,
    ) -> Result<(), String> {
        self.mark_released(reservation_id, state)
    }
}

pub struct MemoryReservationStore {
    store: SharedStore,
}

impl MemoryReservationStore {
    pub fn new(store: SharedStore) -> Self {
        Self { store }
    }
}

#[async_trait::async_trait]
impl ReservationStore for MemoryReservationStore {
    fn upsert(&self, reservation: &ResourceReservation) -> Result<(), String> {
        self.store
            .write_sync({
                let reservation = reservation.clone();
                move |db| {
                    db.upsert_compute_reservation_row(
                        &reservation.reservation_id.0,
                        &reservation.run_id.0,
                        &reservation.task_id.0,
                        &reservation.attempt_id.0,
                        &encode_target(&reservation.target_scope),
                        &encode_resources(&reservation.resources),
                        &reservation.issued_at.to_rfc3339(),
                        &reservation.expires_at.to_rfc3339(),
                        reservation.reservation_epoch as i64,
                        encode_state(reservation.state),
                        reservation.speculative,
                    )
                    .map_err(|e| e.to_string())
                }
            })
            .map_err(|e| e.to_string())?
    }

    async fn upsert_async(&self, reservation: &ResourceReservation) -> Result<(), String> {
        self.store
            .write({
                let reservation = reservation.clone();
                move |db| {
                    db.upsert_compute_reservation_row(
                        &reservation.reservation_id.0,
                        &reservation.run_id.0,
                        &reservation.task_id.0,
                        &reservation.attempt_id.0,
                        &encode_target(&reservation.target_scope),
                        &encode_resources(&reservation.resources),
                        &reservation.issued_at.to_rfc3339(),
                        &reservation.expires_at.to_rfc3339(),
                        reservation.reservation_epoch as i64,
                        encode_state(reservation.state),
                        reservation.speculative,
                    )
                    .map_err(|e| e.to_string())
                }
            })
            .await
            .map_err(|e| e.to_string())?
    }

    fn get(&self, reservation_id: &str) -> Result<Option<ResourceReservation>, String> {
        self.store
            .read_sync({
                let reservation_id = reservation_id.to_string();
                move |db| {
                    Ok(db
                        .get_compute_reservation_row(&reservation_id)
                        .map_err(|e| e.to_string())?
                        .map(reservation_from_row))
                }
            })
            .map_err(|e| e.to_string())?
    }

    fn list_active(&self) -> Result<Vec<ResourceReservation>, String> {
        self.store
            .read_sync(move |db| {
                Ok(db
                    .list_active_compute_reservation_rows()
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .map(reservation_from_row)
                    .collect())
            })
            .map_err(|e| e.to_string())?
    }

    fn mark_released(&self, reservation_id: &str, state: ReservationState) -> Result<(), String> {
        self.store
            .write_sync({
                let reservation_id = reservation_id.to_string();
                move |db| {
                    db.mark_compute_reservation_released_row(&reservation_id, encode_state(state))
                        .map_err(|e| e.to_string())
                }
            })
            .map_err(|e| e.to_string())?
    }
}

/// In-memory fallback when lokai.db is unavailable.
#[derive(Default)]
pub struct InMemoryReservationStore {
    rows: std::sync::Mutex<Vec<ResourceReservation>>,
}

impl ReservationStore for InMemoryReservationStore {
    fn upsert(&self, reservation: &ResourceReservation) -> Result<(), String> {
        let mut g = mutex_lock(&self.rows);
        if let Some(existing) = g
            .iter_mut()
            .find(|r| r.reservation_id == reservation.reservation_id)
        {
            *existing = reservation.clone();
        } else {
            g.push(reservation.clone());
        }
        Ok(())
    }

    fn get(&self, reservation_id: &str) -> Result<Option<ResourceReservation>, String> {
        let g = mutex_lock(&self.rows);
        Ok(g.iter()
            .find(|r| r.reservation_id.0 == reservation_id)
            .cloned())
    }

    fn list_active(&self) -> Result<Vec<ResourceReservation>, String> {
        let g = mutex_lock(&self.rows);
        Ok(g.iter()
            .filter(|r| {
                matches!(
                    r.state,
                    ReservationState::Reserved
                        | ReservationState::Dispatched
                        | ReservationState::Running
                )
            })
            .cloned()
            .collect())
    }

    fn mark_released(&self, reservation_id: &str, state: ReservationState) -> Result<(), String> {
        let mut g = mutex_lock(&self.rows);
        if let Some(r) = g.iter_mut().find(|r| r.reservation_id.0 == reservation_id) {
            r.state = state;
        }
        Ok(())
    }
}

fn encode_target(target: &ReservationTarget) -> String {
    match target {
        ReservationTarget::Local => "local".into(),
        ReservationTarget::Worker { worker_id } => format!("worker:{}", worker_id.0),
    }
}

fn decode_target(raw: &str) -> ReservationTarget {
    if let Some(id) = raw.strip_prefix("worker:") {
        ReservationTarget::Worker {
            worker_id: lokai_domain::WorkerId::new(id),
        }
    } else {
        ReservationTarget::Local
    }
}

fn encode_resources(r: &ReservedResources) -> String {
    serde_json::to_string(r).unwrap_or_else(|_| "{}".into())
}

fn decode_resources(raw: &str) -> ReservedResources {
    serde_json::from_str(raw).unwrap_or_default()
}

fn encode_state(s: ReservationState) -> &'static str {
    match s {
        ReservationState::Proposed => "proposed",
        ReservationState::Reserved => "reserved",
        ReservationState::Dispatched => "dispatched",
        ReservationState::Running => "running",
        ReservationState::Released => "released",
        ReservationState::Expired => "expired",
        ReservationState::Canceled => "canceled",
        ReservationState::OverBudget => "over_budget",
    }
}

fn decode_state(raw: &str) -> ReservationState {
    match raw {
        "proposed" => ReservationState::Proposed,
        "reserved" => ReservationState::Reserved,
        "dispatched" => ReservationState::Dispatched,
        "running" => ReservationState::Running,
        "released" => ReservationState::Released,
        "expired" => ReservationState::Expired,
        "canceled" => ReservationState::Canceled,
        "over_budget" => ReservationState::OverBudget,
        _ => ReservationState::Canceled,
    }
}

fn reservation_from_row(r: lokai_memory::ComputeReservationRow) -> ResourceReservation {
    ResourceReservation {
        reservation_id: ReservationId::new(r.reservation_id),
        run_id: RunId::new(r.run_id),
        task_id: TaskId::new(r.task_id),
        attempt_id: AttemptId::new(r.attempt_id),
        target_scope: decode_target(&r.target_scope),
        resources: decode_resources(&r.resources_json),
        issued_at: chrono::DateTime::parse_from_rfc3339(&r.issued_at)
            .map(|d| d.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now()),
        expires_at: chrono::DateTime::parse_from_rfc3339(&r.expires_at)
            .map(|d| d.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now()),
        reservation_epoch: r.reservation_epoch as u64,
        state: decode_state(&r.state),
        speculative: r.speculative,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn reserved(id: &str) -> ResourceReservation {
        let now = Utc::now();
        ResourceReservation {
            reservation_id: ReservationId::new(id),
            run_id: RunId::new("run_1"),
            task_id: TaskId::new("task_1"),
            attempt_id: AttemptId::new("att_1"),
            target_scope: ReservationTarget::Local,
            resources: ReservedResources::default(),
            issued_at: now,
            expires_at: now + chrono::Duration::seconds(3600),
            reservation_epoch: 1,
            state: ReservationState::Reserved,
            speculative: false,
        }
    }

    #[test]
    fn memory_reservation_survives_sqlite_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lokai.db");
        {
            let shared = lokai_memory::SharedStore::open(&path, 1).unwrap();
            let persist = MemoryReservationStore::new(shared);
            persist.upsert(&reserved("res_1")).unwrap();
            assert_eq!(persist.list_active().unwrap().len(), 1);
        }
        let shared = lokai_memory::SharedStore::open(&path, 1).unwrap();
        let persist = MemoryReservationStore::new(shared);
        let got = persist.get("res_1").unwrap().expect("durable row");
        assert_eq!(got.state, ReservationState::Reserved);
        persist
            .mark_released("res_1", ReservationState::Canceled)
            .unwrap();
        assert!(persist.list_active().unwrap().is_empty());
    }

    #[test]
    fn in_memory_reservation_is_intentional_loss() {
        let persist = InMemoryReservationStore::default();
        persist.upsert(&reserved("res_ephemeral")).unwrap();
        assert_eq!(persist.list_active().unwrap().len(), 1);
        drop(persist);
        let restarted = InMemoryReservationStore::default();
        assert!(restarted.list_active().unwrap().is_empty());
        assert!(restarted.get("res_ephemeral").unwrap().is_none());
    }
}

//! Worker-local profile store (`worker.db`, ES5-4).

use std::sync::Arc;

use lokai_memory::{WorkerStore, WorkerStoreError};

use crate::client::InferenceClient;
use crate::defaults::LOCAL_NODE_ID;
use crate::detect::detect_hardware;
use crate::doctor::diagnose;
use crate::profile::{RuntimeProfile, TierRole};
use crate::status::{CapacityDiagnosis, CapacityDoctorStatus, CapacityStatus};
use crate::store::{tier_role_str, ProfileStoreError};

pub type Result<T> = std::result::Result<T, ProfileStoreError>;

pub struct WorkerProfileStore<'a> {
    store: &'a WorkerStore,
}

impl<'a> WorkerProfileStore<'a> {
    pub fn new(store: &'a WorkerStore) -> Self {
        Self { store }
    }

    pub fn active_profile(&self, node_id: &str, role: TierRole) -> Result<Option<RuntimeProfile>> {
        let Some(id) = self
            .store
            .get_capacity_binding(node_id, tier_role_str(role))
            .map_err(map_err)?
        else {
            return Ok(None);
        };
        self.get(&id)
    }

    pub fn get(&self, id: &str) -> Result<Option<RuntimeProfile>> {
        let Some(row) = self.store.get_runtime_profile(id).map_err(map_err)? else {
            return Ok(None);
        };
        let p: RuntimeProfile = serde_json::from_str(&row.json)?;
        Ok(Some(p))
    }
}

fn map_err(e: WorkerStoreError) -> ProfileStoreError {
    ProfileStoreError::Sqlite(e.into())
}

pub fn status_for_worker(store: &WorkerStore) -> CapacityStatus {
    let (status, _) = capacity_status_blocking(store, None, None);
    status
}

/// Sync capacity doctor (DB + optional live Ollama observation).
pub fn capacity_status_blocking(
    store: &WorkerStore,
    ollama_version: Option<String>,
    live_obs: Option<&crate::profile::ObservedPlacement>,
) -> (CapacityStatus, CapacityDiagnosis) {
    let ps = WorkerProfileStore::new(store);
    let profile = ps
        .active_profile(LOCAL_NODE_ID, TierRole::Coder)
        .ok()
        .flatten();
    let live_hw = detect_hardware(ollama_version);
    let diagnosis = diagnose(profile.as_ref(), &live_hw, live_obs);
    let status = super::service::build_capacity_status(profile.as_ref(), &live_hw, &diagnosis);
    (status, diagnosis)
}

/// Live doctor for worker `worker.db` (async Ollama probe + blocking SQLite).
pub async fn capacity_status_for_worker_path(
    db_path: impl Into<std::path::PathBuf> + Send,
    client: Arc<dyn InferenceClient>,
    ollama_version: Option<String>,
) -> (CapacityStatus, CapacityDiagnosis) {
    let db_path = db_path.into();
    let lookup_path = db_path.clone();
    let model = tokio::task::spawn_blocking(move || {
        let store = WorkerStore::open(&lookup_path).ok()?;
        WorkerProfileStore::new(&store)
            .active_profile(LOCAL_NODE_ID, TierRole::Coder)
            .ok()
            .flatten()
            .map(|p| p.recipe.estate_model)
    })
    .await
    .ok()
    .flatten();
    let live_obs = match model {
        Some(model) => client.fetch_observed_placement(&model).await,
        None => None,
    };
    let ver = ollama_version;
    match tokio::task::spawn_blocking(move || {
        WorkerStore::open(&db_path).map(|ws| capacity_status_blocking(&ws, ver, live_obs.as_ref()))
    })
    .await
    {
        Ok(Ok(pair)) => pair,
        _ => (
            CapacityStatus::default(),
            CapacityDiagnosis {
                status: CapacityDoctorStatus::NoProfile,
                codes: vec![],
                summary: "worker.db unavailable".into(),
                recommendations: vec![],
            },
        ),
    }
}

/// JSON payload for worker `GET /v1/capacity/status`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkerCapacityWire {
    pub completed: bool,
    pub doctor: String,
    pub stale: bool,
    pub gates_ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_profile_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardware_summary: Option<String>,
}

impl From<&CapacityStatus> for WorkerCapacityWire {
    fn from(s: &CapacityStatus) -> Self {
        Self {
            completed: s.completed,
            doctor: crate::status::doctor_status_str(s.doctor).into(),
            stale: s.stale,
            gates_ok: s.gates_ok,
            active_profile_id: s.active_profile_id.clone(),
            active_profile_label: s.active_profile_label.clone(),
            hardware_summary: s.hardware_summary.clone(),
        }
    }
}

pub fn worker_capacity_wire(status: &CapacityStatus) -> WorkerCapacityWire {
    WorkerCapacityWire::from(status)
}

pub fn node_capacity_from_wire(wire: &WorkerCapacityWire) -> lokai_inference::NodeCapacityHealth {
    lokai_inference::NodeCapacityHealth {
        doctor: wire.doctor.clone(),
        active_profile_id: wire.active_profile_id.clone(),
        gates_ok: wire.gates_ok,
        stale: wire.stale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lokai_memory::WorkerStore;
    use tempfile::NamedTempFile;

    #[test]
    fn worker_store_status_no_profile() {
        let tmp = NamedTempFile::new().unwrap();
        let ws = WorkerStore::open(tmp.path()).unwrap();
        let s = status_for_worker(&ws);
        assert!(!s.completed);
        assert_eq!(s.doctor, CapacityDoctorStatus::NoProfile);
    }

    #[test]
    fn worker_wire_round_trip() {
        let w = worker_capacity_wire(&CapacityStatus {
            completed: true,
            stale: false,
            doctor: CapacityDoctorStatus::Healthy,
            active_profile_id: Some("p1".into()),
            active_profile_label: Some("lab".into()),
            hardware_summary: Some("GPU".into()),
            gates_ok: true,
            last_setup_at: None,
            profile_model: None,
            profile_base_model: None,
        });
        let node = node_capacity_from_wire(&w);
        assert!(node.gates_ok);
        assert_eq!(node.doctor, "healthy");
    }
}

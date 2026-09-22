//! Build RPC/CLI capacity status from store + doctor.

use std::sync::Arc;

use tetonic_memory::{SharedStore, Store};

use crate::client::InferenceClient;
use crate::defaults::LOCAL_NODE_ID;
use crate::detect::{detect_hardware, hardware_summary};
use crate::doctor::{diagnose, is_fingerprint_stale};
use crate::profile::RuntimeProfile;
use crate::status::{CapacityDiagnosis, CapacityDoctorStatus, CapacityStatus};
use crate::store::ProfileStore;

pub async fn capacity_status_for_store(
    store: &SharedStore,
    node_id: &str,
    client: &Arc<dyn InferenceClient>,
    ollama_version: Option<String>,
) -> (CapacityStatus, CapacityDiagnosis) {
    let node_id_owned = node_id.to_string();
    let profile = store
        .read(move |db| {
            ProfileStore::new(db)
                .active_profile(&node_id_owned, crate::profile::TierRole::Coder)
                .ok()
                .flatten()
        })
        .await
        .unwrap_or(None);
    let live_hw = detect_hardware(ollama_version);
    let live_obs = match profile.as_ref() {
        Some(profile) => {
            client
                .fetch_observed_placement(&profile.recipe.estate_model)
                .await
        }
        None => None,
    };
    let diagnosis = diagnose(profile.as_ref(), &live_hw, live_obs.as_ref());
    let status = build_capacity_status(profile.as_ref(), &live_hw, &diagnosis);
    (status, diagnosis)
}

pub fn build_capacity_status(
    profile: Option<&RuntimeProfile>,
    live_hw: &crate::profile::HardwareSnapshot,
    diagnosis: &CapacityDiagnosis,
) -> CapacityStatus {
    match profile {
        Some(p) => CapacityStatus {
            completed: true,
            stale: is_fingerprint_stale(p, live_hw),
            doctor: diagnosis.status,
            active_profile_id: Some(p.id.clone()),
            active_profile_label: Some(p.label.clone()),
            hardware_summary: Some(hardware_summary(live_hw)),
            gates_ok: p.gates_passed && diagnosis.status == CapacityDoctorStatus::Healthy,
            last_setup_at: Some(p.created_at),
            profile_model: Some(p.recipe.estate_model.clone()),
            profile_base_model: Some(p.recipe.base_model.clone()),
        },
        None => CapacityStatus {
            completed: false,
            stale: false,
            doctor: CapacityDoctorStatus::NoProfile,
            active_profile_id: None,
            active_profile_label: None,
            hardware_summary: Some(hardware_summary(live_hw)),
            gates_ok: false,
            last_setup_at: None,
            profile_model: None,
            profile_base_model: None,
        },
    }
}

pub fn status_for_node(store: &Store) -> CapacityStatus {
    status_for_node_id(store, LOCAL_NODE_ID)
}

pub fn status_for_node_id(store: &Store, node_id: &str) -> CapacityStatus {
    let ps = ProfileStore::new(store);
    let profile = ps
        .active_profile(node_id, crate::profile::TierRole::Coder)
        .ok()
        .flatten();
    let live_hw = detect_hardware(None);
    let diagnosis = diagnose(profile.as_ref(), &live_hw, None);
    build_capacity_status(profile.as_ref(), &live_hw, &diagnosis)
}

/// Saved-profile snapshot for turn admission (H2-1). No NVML, no Ollama.
pub fn admission_status_from_store(store: &Store, node_id: &str) -> CapacityStatus {
    let ps = ProfileStore::new(store);
    let profile = ps
        .active_profile(node_id, crate::profile::TierRole::Coder)
        .ok()
        .flatten();
    match profile {
        Some(p) => CapacityStatus {
            completed: true,
            stale: false,
            doctor: if p.gates_passed {
                CapacityDoctorStatus::Healthy
            } else {
                CapacityDoctorStatus::Degraded
            },
            active_profile_id: Some(p.id.clone()),
            active_profile_label: Some(p.label.clone()),
            hardware_summary: None,
            gates_ok: p.gates_passed,
            last_setup_at: Some(p.created_at),
            profile_model: Some(p.recipe.estate_model.clone()),
            profile_base_model: Some(p.recipe.base_model.clone()),
        },
        None => CapacityStatus {
            completed: false,
            stale: false,
            doctor: CapacityDoctorStatus::NoProfile,
            active_profile_id: None,
            active_profile_label: None,
            hardware_summary: None,
            gates_ok: false,
            last_setup_at: None,
            profile_model: None,
            profile_base_model: None,
        },
    }
}

/// Attach capacity doctor summary to the loopback fabric node (ES5-3).
pub fn enrich_local_node_capacity(
    snap: &mut tetonic_inference::FabricSnapshot,
    status: &CapacityStatus,
) {
    use tetonic_inference::{NodeCapacityHealth, LOCAL_NODE_ID as FABRIC_LOCAL};
    for node in &mut snap.nodes {
        if node.id == FABRIC_LOCAL {
            node.capacity = Some(NodeCapacityHealth {
                doctor: crate::status::doctor_status_str(status.doctor).into(),
                active_profile_id: status.active_profile_id.clone(),
                gates_ok: status.gates_ok,
                stale: status.stale,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{
        GpuInfo, GpuRole, HardwareSnapshot, InferenceRecipe, ObservedPlacement, ProfileMetrics,
        ProfileSource, TierRole, SCHEMA_VERSION,
    };
    use crate::store::ProfileStore;
    use chrono::Utc;
    use tempfile::NamedTempFile;

    fn sample_profile(id: &str, gates_passed: bool) -> RuntimeProfile {
        RuntimeProfile {
            schema_version: SCHEMA_VERSION,
            id: id.into(),
            label: "test".into(),
            created_at: Utc::now(),
            node_id: LOCAL_NODE_ID.into(),
            role: TierRole::Coder,
            source: ProfileSource::Import,
            hardware: HardwareSnapshot {
                fingerprint: "fp_test".into(),
                gpus: vec![GpuInfo {
                    name: "Test GPU".into(),
                    uuid: None,
                    bus_id: None,
                    vram_total_mb: 24_000,
                    role: GpuRole::Compute,
                }],
                cpu_cores: 8,
                ram_mb: 32_768,
                os: "linux-x86_64".into(),
                ollama_version: None,
            },
            recipe: InferenceRecipe {
                base_model: "qwen3.6:latest".into(),
                estate_model: "qwen3.6-estate".into(),
                num_ctx: 4096,
                num_gpu: Some(999),
                keep_alive: "30m".into(),
                modelfile_path: None,
                env_hints: vec![],
                draft_model: None,
                draft_count: None,
            },
            observed: ObservedPlacement {
                gpu_processor_pct: Some(85.0),
                processor_split: Some("85%/15%".into()),
                vram_used_mb: 18_000,
                resident_model: "qwen3.6-estate".into(),
                load_wall_s: 10.0,
                measured_at: Utc::now(),
            },
            metrics: ProfileMetrics {
                raw_short_wall_s: 8.0,
                ..Default::default()
            },
            gates_passed,
        }
    }

    #[test]
    fn admission_snapshot_uses_saved_gates_without_live_probe() {
        let tmp = NamedTempFile::new().unwrap();
        let store = Store::open(tmp.path()).unwrap();
        let none = admission_status_from_store(&store, LOCAL_NODE_ID);
        assert_eq!(none.doctor, CapacityDoctorStatus::NoProfile);
        assert!(!none.gates_ok);

        let ps = ProfileStore::new(&store);
        ps.append(&sample_profile("profile_a", false)).unwrap();
        ps.activate(LOCAL_NODE_ID, TierRole::Coder, "profile_a")
            .unwrap();
        let degraded = admission_status_from_store(&store, LOCAL_NODE_ID);
        assert_eq!(degraded.doctor, CapacityDoctorStatus::Degraded);
        assert!(!degraded.gates_ok);
        assert_eq!(degraded.profile_model.as_deref(), Some("qwen3.6-estate"));
    }
}

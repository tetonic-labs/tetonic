//! ProfileStore — append-only profiles and activation pointers.

use tetonic_memory::{RuntimeProfileRow, Store, StoreError};

use crate::profile::{RuntimeProfile, TierRole, SCHEMA_VERSION};

#[derive(Debug, thiserror::Error)]
pub enum ProfileStoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] StoreError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("profile: {0}")]
    Profile(String),
    #[error("profile id already exists: {0}")]
    DuplicateId(String),
}

pub type Result<T> = std::result::Result<T, ProfileStoreError>;

pub struct ProfileStore<'a> {
    store: &'a Store,
}

impl<'a> ProfileStore<'a> {
    pub fn new(store: &'a Store) -> Self {
        Self { store }
    }

    pub fn append(&self, profile: &RuntimeProfile) -> Result<()> {
        profile
            .validate_schema()
            .map_err(ProfileStoreError::Profile)?;
        if profile.schema_version != SCHEMA_VERSION {
            return Err(ProfileStoreError::Profile(format!(
                "unsupported schema_version {}",
                profile.schema_version
            )));
        }
        if self.store.get_runtime_profile(&profile.id)?.is_some() {
            return Err(ProfileStoreError::DuplicateId(profile.id.clone()));
        }
        let json = serde_json::to_string(profile)?;
        let row = RuntimeProfileRow {
            id: profile.id.clone(),
            node_id: profile.node_id.clone(),
            role: tier_role_str(profile.role).to_string(),
            fingerprint: profile.hardware.fingerprint.clone(),
            created_at: profile.created_at.to_rfc3339(),
            gates_passed: profile.gates_passed,
            json,
        };
        self.store.insert_runtime_profile(&row)?;
        Ok(())
    }

    pub fn activate(&self, node_id: &str, role: TierRole, profile_id: &str) -> Result<()> {
        if self.store.get_runtime_profile(profile_id)?.is_none() {
            return Err(ProfileStoreError::Profile(format!(
                "unknown profile id {profile_id}"
            )));
        }
        self.store
            .set_capacity_binding(node_id, tier_role_str(role), Some(profile_id))?;
        Ok(())
    }

    pub fn active_profile(&self, node_id: &str, role: TierRole) -> Result<Option<RuntimeProfile>> {
        let Some(id) = self
            .store
            .get_capacity_binding(node_id, tier_role_str(role))?
        else {
            return Ok(None);
        };
        self.get(&id)
    }

    pub fn get(&self, id: &str) -> Result<Option<RuntimeProfile>> {
        let Some(row) = self.store.get_runtime_profile(id)? else {
            return Ok(None);
        };
        let p: RuntimeProfile = serde_json::from_str(&row.json)?;
        Ok(Some(p))
    }

    pub fn list(&self, node_id: Option<&str>) -> Result<Vec<RuntimeProfile>> {
        let rows = self.store.list_runtime_profiles(node_id)?;
        rows.into_iter()
            .map(|r| serde_json::from_str(&r.json).map_err(ProfileStoreError::from))
            .collect()
    }

    /// Summaries for RPC/CLI list (newest first).
    pub fn list_summaries(&self, node_id: &str, role: TierRole) -> Result<Vec<ProfileSummary>> {
        let active = self
            .store
            .get_capacity_binding(node_id, tier_role_str(role))?;
        let profiles: Vec<RuntimeProfile> = self
            .list(Some(node_id))?
            .into_iter()
            .filter(|p| p.role == role)
            .collect();
        Ok(profiles
            .into_iter()
            .map(|p| ProfileSummary {
                id: p.id.clone(),
                label: p.label.clone(),
                created_at: p.created_at.to_rfc3339(),
                gates_passed: p.gates_passed,
                estate_model: p.recipe.estate_model.clone(),
                num_ctx: p.recipe.num_ctx,
                active: active.as_deref() == Some(p.id.as_str()),
            })
            .collect())
    }

    /// Activate the next-older profile for this node/role (append-only rollback).
    pub fn rollback(&self, node_id: &str, role: TierRole) -> Result<RuntimeProfile> {
        let Some(active_id) = self
            .store
            .get_capacity_binding(node_id, tier_role_str(role))?
        else {
            return Err(ProfileStoreError::Profile(
                "no active profile to rollback".into(),
            ));
        };
        let profiles: Vec<RuntimeProfile> = self
            .list(Some(node_id))?
            .into_iter()
            .filter(|p| p.role == role)
            .collect();
        let pos = profiles
            .iter()
            .position(|p| p.id == active_id)
            .ok_or_else(|| {
                ProfileStoreError::Profile("active profile missing from store".into())
            })?;
        let prev = profiles
            .get(pos + 1)
            .ok_or_else(|| ProfileStoreError::Profile("no older profile to rollback to".into()))?;
        self.activate(node_id, role, &prev.id)?;
        Ok(prev.clone())
    }
}

#[derive(Debug, Clone)]
pub struct ProfileSummary {
    pub id: String,
    pub label: String,
    pub created_at: String,
    pub gates_passed: bool,
    pub estate_model: String,
    pub num_ctx: u32,
    pub active: bool,
}

pub fn tier_role_str(role: TierRole) -> &'static str {
    match role {
        TierRole::Coder => "coder",
        TierRole::Fast => "fast",
        TierRole::Hard => "hard",
        TierRole::Embed => "embed",
    }
}

pub fn parse_tier_role(s: &str) -> Option<TierRole> {
    match s {
        "coder" => Some(TierRole::Coder),
        "fast" => Some(TierRole::Fast),
        "hard" => Some(TierRole::Hard),
        "embed" => Some(TierRole::Embed),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tempfile::NamedTempFile;
    use tetonic_memory::Store;

    use crate::profile::{
        GpuInfo, GpuRole, HardwareSnapshot, InferenceRecipe, ObservedPlacement, ProfileMetrics,
        ProfileSource,
    };

    fn sample_profile(id: &str) -> RuntimeProfile {
        RuntimeProfile {
            schema_version: SCHEMA_VERSION,
            id: id.into(),
            label: "test".into(),
            created_at: Utc::now(),
            node_id: "local".into(),
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
            gates_passed: true,
        }
    }

    #[test]
    fn append_and_activate() {
        let tmp = NamedTempFile::new().unwrap();
        let store = Store::open(tmp.path()).unwrap();
        let ps = ProfileStore::new(&store);
        let p = sample_profile("profile_a");
        ps.append(&p).unwrap();
        ps.activate("local", TierRole::Coder, "profile_a").unwrap();
        let active = ps
            .active_profile("local", TierRole::Coder)
            .unwrap()
            .unwrap();
        assert_eq!(active.recipe.estate_model, "qwen3.6-estate");
    }

    #[test]
    fn rollback_activates_older_profile() {
        let tmp = NamedTempFile::new().unwrap();
        let store = Store::open(tmp.path()).unwrap();
        let ps = ProfileStore::new(&store);
        let mut a = sample_profile("profile_a");
        a.created_at = Utc::now();
        let mut b = sample_profile("profile_b");
        b.label = "older".into();
        b.created_at = Utc::now() - chrono::Duration::seconds(60);
        ps.append(&b).unwrap();
        ps.append(&a).unwrap();
        ps.activate("local", TierRole::Coder, "profile_a").unwrap();
        let rolled = ps.rollback("local", TierRole::Coder).unwrap();
        assert_eq!(rolled.id, "profile_b");
        let active = ps
            .active_profile("local", TierRole::Coder)
            .unwrap()
            .unwrap();
        assert_eq!(active.id, "profile_b");
    }
}

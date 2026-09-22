//! Load active profile bindings into session defaults.

use lokai_memory::Store;

use crate::profile::{RuntimeProfile, TierRole};
use crate::store::ProfileStore;

pub const LOCAL_NODE_ID: &str = "local";

#[derive(Debug, Clone)]
pub struct InferenceDefaults {
    pub model_fast: String,
    pub model_hard: String,
    pub num_ctx: u32,
    pub profile_id: Option<String>,
    pub profile_label: Option<String>,
    pub draft_model: Option<String>,
    pub draft_count: Option<u32>,
}

impl InferenceDefaults {
    pub fn from_profile(profile: &RuntimeProfile) -> Self {
        let model = profile.recipe.estate_model.clone();
        Self {
            model_fast: model.clone(),
            model_hard: model,
            num_ctx: profile.recipe.num_ctx,
            profile_id: Some(profile.id.clone()),
            profile_label: Some(profile.label.clone()),
            draft_model: profile.recipe.draft_model.clone(),
            draft_count: profile.recipe.draft_count,
        }
    }

    pub fn fallback() -> Self {
        let model = std::env::var("LOKAI_MODEL")
            .unwrap_or_else(|_| lokai_inference::DEFAULT_MODEL.to_string());
        let draft_model = std::env::var("LOKAI_DRAFT_MODEL").ok();
        let draft_count = std::env::var("LOKAI_DRAFT_COUNT")
            .ok()
            .and_then(|v| v.parse::<u32>().ok());
        Self {
            model_fast: model.clone(),
            model_hard: model,
            num_ctx: 8192,
            profile_id: None,
            profile_label: None,
            draft_model,
            draft_count,
        }
    }
}

pub fn load_inference_defaults(store: &Store, node_id: &str) -> InferenceDefaults {
    let ps = ProfileStore::new(store);
    match ps.active_profile(node_id, TierRole::Coder) {
        Ok(Some(p)) => InferenceDefaults::from_profile(&p),
        _ => InferenceDefaults::fallback(),
    }
}

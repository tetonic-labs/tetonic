use std::sync::Arc;

use lokai_memory::Store;
use tetonic_policy::{PolicyEngine, PolicyMode, PolicySettings};

/// Load persisted policy settings or default to homelab estate stub.
pub fn load_policy_engine(store: Option<&Store>) -> Arc<PolicyEngine> {
    let settings = store
        .map(load_policy_settings)
        .unwrap_or_else(PolicySettings::default_settings);
    Arc::new(PolicyEngine::from_settings(settings))
}

fn load_policy_settings(store: &Store) -> PolicySettings {
    let mode = store
        .get_policy_mode()
        .ok()
        .and_then(|m| PolicyMode::parse(&m))
        .unwrap_or(PolicyMode::EstateStub);
    let verify_allowed = store.get_policy_verify_allowed().unwrap_or(true);
    let mutations_allowed = store.get_policy_mutations_allowed().unwrap_or(true);
    let placement = store.get_project_placement_policy().unwrap_or_default();
    PolicySettings {
        mode,
        verify_allowed,
        mutations_allowed,
        placement,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lokai_memory::Store;

    #[test]
    fn loads_persisted_toggles() {
        let store = Store::open(":memory:").unwrap();
        store.set_policy_mode("full").unwrap();
        store.set_policy_verify_allowed(false).unwrap();
        store.set_policy_mutations_allowed(false).unwrap();
        let engine = load_policy_engine(Some(&store));
        assert_eq!(engine.mode(), PolicyMode::Full);
        assert!(!engine.verify_allowed());
        assert!(!engine.mutations_allowed());
    }
}

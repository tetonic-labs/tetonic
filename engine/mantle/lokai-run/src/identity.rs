//! Manager writer for durable `AgentIdentity` records (WORK-01).
//!
//! Product compiles policy to a domain record. This module writes it.
//! Does not import `CodingAgentDefinition`.

use lokai_domain::{AgentIdentity, IdentityId};
use lokai_memory::{AgentIdentityRow, Store};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("identity persist: {0}")]
    Persist(String),
}

pub fn put_identity(store: &Store, rec: &AgentIdentity) -> Result<(), IdentityError> {
    let toolsets = serde_json::to_string(&rec.toolset_subscriptions)
        .map_err(|e| IdentityError::Persist(e.to_string()))?;
    let bindings = serde_json::to_string(&rec.context_bindings)
        .map_err(|e| IdentityError::Persist(e.to_string()))?;
    store
        .put_agent_identity(&AgentIdentityRow {
            identity_id: rec.id.0.clone(),
            owning_application: rec.owning_application.clone(),
            bound_definition_digest: rec.bound_definition_digest.clone(),
            privilege_class: rec.privilege_class.clone(),
            toolset_subscriptions_json: toolsets,
            context_bindings_json: bindings,
            recovery_id: rec.recovery_id.clone(),
        })
        .map_err(|e| IdentityError::Persist(e.to_string()))
}

pub fn get_identity(
    store: &Store,
    id: &IdentityId,
) -> Result<Option<AgentIdentity>, IdentityError> {
    let row = store
        .get_agent_identity(&id.0)
        .map_err(|e| IdentityError::Persist(e.to_string()))?;
    let Some(row) = row else {
        return Ok(None);
    };
    let toolset_subscriptions = serde_json::from_str(&row.toolset_subscriptions_json)
        .map_err(|e| IdentityError::Persist(e.to_string()))?;
    let context_bindings = serde_json::from_str(&row.context_bindings_json)
        .map_err(|e| IdentityError::Persist(e.to_string()))?;
    Ok(Some(AgentIdentity {
        id: IdentityId::new(row.identity_id),
        owning_application: row.owning_application,
        bound_definition_digest: row.bound_definition_digest,
        privilege_class: row.privilege_class,
        toolset_subscriptions,
        context_bindings,
        recovery_id: row.recovery_id,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lokai_memory::Store;

    #[test]
    fn work01_put_identity_round_trip() {
        let store = Store::open(":memory:").unwrap();
        let rec = AgentIdentity {
            id: IdentityId::new("id_coding_production"),
            owning_application: "coding".into(),
            bound_definition_digest: "digest".into(),
            privilege_class: "default".into(),
            toolset_subscriptions: vec!["planner".into()],
            context_bindings: vec!["memory".into()],
            recovery_id: "id_coding_production".into(),
        };
        put_identity(&store, &rec).unwrap();
        let got = get_identity(&store, &rec.id).unwrap().expect("row");
        assert_eq!(got, rec);
    }
}

//! Capability registry with optional durable backing (M2-1 / R6-1).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use lokai_domain::ids::CapabilityId;
use lokai_domain::{
    validate_authorized_action, AuthorizedAction, CapabilityConsumer, CapabilityError,
    IssuedCapability,
};
use lokai_memory::{RecoverMutex, SharedStore};

/// Process-local registry; when `durable` is set, rows persist in `lokai.db` and
/// all live capabilities are revoked when this store is constructed (restart).
pub struct InMemoryCapabilityStore {
    caps: Mutex<HashMap<CapabilityId, IssuedCapability>>,
    durable: Option<Arc<SharedStore>>,
    fail_durable: AtomicBool,
}

impl InMemoryCapabilityStore {
    pub fn new() -> Self {
        Self {
            caps: Mutex::new(HashMap::new()),
            durable: None,
            fail_durable: AtomicBool::new(false),
        }
    }

    /// Open with durable store: revoke any pre-restart live capabilities, then serve
    /// register/authorize through SQLite + in-memory cache.
    pub fn with_durable(store: Arc<SharedStore>) -> Result<Self, CapabilityError> {
        let out = Self {
            caps: Mutex::new(HashMap::new()),
            durable: Some(store),
            fail_durable: AtomicBool::new(false),
        };
        out.durable_write(|db| db.revoke_all_issued_capabilities().map(|_| ()))?;
        Ok(out)
    }

    /// Test injection: next durable write fails without mutating the memory map.
    pub fn fail_next_durable_write(&self) {
        self.fail_durable.store(true, Ordering::SeqCst);
    }

    pub fn register(&self, cap: IssuedCapability) -> Result<(), CapabilityError> {
        let mut caps = self.caps.lock_recover();
        if caps.contains_key(&cap.capability_id) {
            return Err(CapabilityError::ScopeMismatch);
        }
        if self.durable.is_some() {
            let cap_row = cap.clone();
            self.durable_write(move |db| db.insert_new_issued_capability(&cap_row))?;
        }
        caps.insert(cap.capability_id.clone(), cap);
        Ok(())
    }

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    fn durable_write<F>(&self, f: F) -> Result<(), CapabilityError>
    where
        F: FnOnce(&mut lokai_memory::Store) -> lokai_memory::Result<()> + Send + 'static,
    {
        if self.fail_durable.swap(false, Ordering::SeqCst) {
            return Err(CapabilityError::PersistFailed);
        }
        let Some(store) = &self.durable else {
            return Ok(());
        };
        match store.write_sync(f) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => match e {
                lokai_memory::StoreError::CapabilityRevoked => Err(CapabilityError::Revoked),
                lokai_memory::StoreError::CapabilityExpired => Err(CapabilityError::Expired),
                lokai_memory::StoreError::CapabilityAlreadyConsumed => {
                    Err(CapabilityError::AlreadyConsumed)
                }
                _ => Err(CapabilityError::PersistFailed),
            },
            Err(_) => Err(CapabilityError::PersistFailed),
        }
    }
}

impl Default for InMemoryCapabilityStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CapabilityConsumer for InMemoryCapabilityStore {
    fn authorize(&self, authorized: &AuthorizedAction) -> Result<(), CapabilityError> {
        let mut caps = self.caps.lock_recover();
        let id = &authorized.capability.capability_id;
        let mut stored = match caps.get(id).cloned() {
            Some(c) => c,
            None => {
                if let Some(store) = &self.durable {
                    match store.read_sync(|db| db.load_issued_capability(&id.0)) {
                        Ok(Ok(Some(cap))) => cap,
                        _ => return Err(CapabilityError::ScopeMismatch),
                    }
                } else {
                    return Err(CapabilityError::ScopeMismatch);
                }
            }
        };
        let bound = AuthorizedAction {
            capability: stored.clone(),
            action: authorized.action.clone(),
        };
        validate_authorized_action(&bound, Self::now_secs())?;
        if stored.revoked {
            return Err(CapabilityError::Revoked);
        }
        if stored.current_use_count >= stored.max_use_count {
            return Err(CapabilityError::AlreadyConsumed);
        }
        if self.durable.is_some() {
            let cap_id = stored.capability_id.0.clone();
            let max_use = stored.max_use_count;
            let now = Self::now_secs();
            let cap_res = Arc::new(Mutex::new(None));
            let cap_res_cb = cap_res.clone();
            self.durable_write(move |db| {
                let res = db.consume_issued_capability(&cap_id, max_use, now)?;
                *cap_res_cb.lock().unwrap() = Some(res);
                Ok(())
            })?;
            let updated = cap_res.lock().unwrap().take();
            if let Some(c) = updated {
                stored = c;
            } else {
                stored.current_use_count += 1;
            }
        } else {
            stored.current_use_count += 1;
        }
        caps.insert(stored.capability_id.clone(), stored);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lokai_domain::classify::DataClass;
    use lokai_domain::execution::{ActionKind, CanonicalActionParameters, ProposedAction};
    use lokai_domain::ids::{ActionId, CapabilityId, SessionId};
    use lokai_domain::{compute_canonical_digest, finalize_parameters, prepare_proposed_action};

    fn sample_action() -> ProposedAction {
        prepare_proposed_action(ProposedAction {
            action_id: ActionId::new("a"),
            session_id: SessionId::new("s"),
            run_id: None,
            task_id: None,
            attempt_id: None,
            agent_id: None,
            workspace_version: None,
            data_class: DataClass::RepositorySource,
            kind: ActionKind::WriteFile,
            parameters: finalize_parameters(
                &ActionKind::WriteFile,
                CanonicalActionParameters {
                    digest: String::new(),
                    executable_identity: None,
                    resolved_path: Some("x.txt".into()),
                    arguments: vec![],
                    shell_identity: None,
                    shell_mode: None,
                    script_bytes: None,
                    working_directory: Some("/ws".into()),
                    env_vars: None,
                    stdin_source_classification: None,
                    filesystem_access_scope: None,
                    network_policy: None,
                    resource_limits: None,
                    process_class: None,
                    sandbox_profile: None,
                    expected_output_limits: None,
                    schema_version: 1,
                    tool_arguments: Some(serde_json::json!({"path":"x.txt"})),
                },
            ),
            requested_capabilities: Default::default(),
            trace_context: Default::default(),
        })
    }

    fn sample_cap(action: &ProposedAction, id: &str) -> IssuedCapability {
        IssuedCapability {
            capability_id: CapabilityId::new(id),
            session_id: action.session_id.clone(),
            run_id: None,
            task_id: None,
            attempt_id: None,
            agent_id: None,
            action_kind: action.kind.clone(),
            canonical_parameter_digest: action.parameters.digest.clone(),
            workspace_version: None,
            data_classification: action.data_class,
            issuance_timestamp: 0,
            expiration: u64::MAX,
            max_use_count: 1,
            current_use_count: 0,
            issuing_policy_version: "v2".into(),
            approval_record_id: None,
            revoked: false,
        }
    }

    #[test]
    fn replay_after_consumption_is_rejected() {
        let store = InMemoryCapabilityStore::new();
        let action = sample_action();
        let cap = sample_cap(&action, "cap_1");
        store.register(cap.clone()).unwrap();
        let authorized = AuthorizedAction {
            capability: cap,
            action,
        };
        store.authorize(&authorized).unwrap();
        assert!(matches!(
            store.authorize(&authorized),
            Err(CapabilityError::AlreadyConsumed)
        ));
    }

    #[test]
    fn durable_capability_revoked_after_runtime_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cap-r61.db");
        let shared = Arc::new(SharedStore::open(&path, 1).unwrap());
        let store = InMemoryCapabilityStore::with_durable(shared.clone()).unwrap();
        let action = sample_action();
        let cap = sample_cap(&action, "cap_restart");
        store.register(cap.clone()).unwrap();
        let authorized = AuthorizedAction {
            capability: cap,
            action,
        };
        store.authorize(&authorized).unwrap();

        let restarted = InMemoryCapabilityStore::with_durable(shared).unwrap();
        assert!(
            matches!(
                restarted.authorize(&authorized),
                Err(CapabilityError::Revoked) | Err(CapabilityError::AlreadyConsumed)
            ),
            "R6-1: restart must not authorize a pre-crash capability"
        );
    }

    #[test]
    fn request_blob_matching_tampered_params_is_rejected() {
        let store = InMemoryCapabilityStore::new();
        let action = sample_action();
        let cap = sample_cap(&action, "cap_tamper");
        store.register(cap.clone()).unwrap();
        let mut tampered = action.clone();
        tampered.parameters.resolved_path = Some("evil.txt".into());
        let expected = compute_canonical_digest(&tampered.kind, &tampered.parameters);
        let mut forged = cap;
        forged.canonical_parameter_digest = expected;
        let authorized = AuthorizedAction {
            capability: forged,
            action: tampered,
        };
        assert!(matches!(
            store.authorize(&authorized),
            Err(CapabilityError::ScopeMismatch)
        ));
    }

    #[test]
    fn durable_persist_err_does_not_register() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cap-persist-err.db");
        let shared = Arc::new(SharedStore::open(&path, 1).unwrap());
        let store = InMemoryCapabilityStore::with_durable(shared).unwrap();
        let action = sample_action();
        let cap = sample_cap(&action, "cap_fail");
        store.fail_next_durable_write();
        assert!(matches!(
            store.register(cap.clone()),
            Err(CapabilityError::PersistFailed)
        ));
        let authorized = AuthorizedAction {
            capability: cap,
            action,
        };
        assert!(matches!(
            store.authorize(&authorized),
            Err(CapabilityError::ScopeMismatch)
        ));
    }
}

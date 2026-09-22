//! Input digest and delivery deduplication (M3-2).

use lokai_domain::{RunSnapshot, RunSupervisorError, TaskId, TaskInputBinding};

pub fn job_input_digest(user_input: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(user_input.as_bytes());
    format!("job:{:x}", h.finalize())
}

pub fn binding_input_digest(binding: &TaskInputBinding) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    binding.task_definition_version.hash(&mut h);
    if let Some(wv) = &binding.workspace_version {
        wv.state_fingerprint().hash(&mut h);
    }
    for art in &binding.input_artifacts {
        art.digest.hash(&mut h);
    }
    format!("{:?}", binding.data_class).hash(&mut h);
    format!("input:{:x}", h.finish())
}

pub fn check_delivery_key(
    snapshot: &RunSnapshot,
    delivery_key: &str,
    attempt_id: &lokai_domain::AttemptId,
) -> Result<(), RunSupervisorError> {
    if let Some(existing) = snapshot.delivery_index.get(delivery_key) {
        if existing != attempt_id {
            return Err(RunSupervisorError::DuplicateDelivery(format!(
                "delivery key {delivery_key} already bound to {existing}"
            )));
        }
    }
    Ok(())
}

pub fn register_delivery_key(
    snapshot: &mut RunSnapshot,
    delivery_key: String,
    attempt_id: lokai_domain::AttemptId,
) {
    snapshot.delivery_index.insert(delivery_key, attempt_id);
}

pub fn workspace_matches(
    binding: &TaskInputBinding,
    workspace_version: &Option<lokai_domain::WorkspaceVersion>,
) -> bool {
    binding.workspace_version == *workspace_version
}

pub fn input_digest_matches(binding: &TaskInputBinding, digest: &str) -> bool {
    binding_input_digest(binding) == digest
}

pub fn task_idempotency_digest(
    run_id: &lokai_domain::RunId,
    task_id: &TaskId,
    binding: &TaskInputBinding,
) -> String {
    lokai_domain::TaskIdempotencyKey::from_binding(run_id, task_id, binding).digest()
}

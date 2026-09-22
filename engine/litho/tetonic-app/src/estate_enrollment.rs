//! Worker enrollment and removal (estate fleet management).

use std::net::IpAddr;
use std::path::Path;

use tetonic_egress::EgressGuard;
use tetonic_enroll::{allow_fabric_workers, complete_enrollment, KeyPair};
use tetonic_memory::{Store, WorkerEnrollmentRow};
use tetonic_node::push_revoke;

use crate::commands::{
    EnrollWorkerCommand, EnrollWorkerResultPayload, RemoveWorkerCommand, RemoveWorkerResultPayload,
};
use crate::errors::AppError;

const COORDINATOR_KEY_FILE: &str = "coordinator.key";

fn base64_pk(bytes: &[u8]) -> String {
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes)
}

pub fn load_or_create_coordinator(store: &Store, data_dir: &Path) -> Result<KeyPair, AppError> {
    let path = data_dir.join(COORDINATOR_KEY_FILE);
    if path.is_file() {
        let bytes = std::fs::read(&path)
            .map_err(|e| AppError::PersistenceFailed(format!("read coordinator.key: {e}")))?;
        let kp = KeyPair::from_signing_bytes(&bytes)
            .map_err(|e| AppError::InvalidRequest(format!("coordinator key: {e}")))?;
        store
            .ensure_owner_identity(&kp.public().0, "default")
            .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?;
        return Ok(kp);
    }
    let kp = KeyPair::generate();
    tetonic_secrets::private_file::write_new(&path, kp.signing_bytes().as_ref())
        .map_err(|e| AppError::PersistenceFailed(format!("write coordinator.key: {e}")))?;
    store
        .ensure_owner_identity(&kp.public().0, "default")
        .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?;
    Ok(kp)
}

pub fn reload_enrollment_egress(store: &Store, guard: &EgressGuard) -> Result<(), AppError> {
    let workers = store
        .list_worker_enrollments()
        .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?
        .into_iter()
        .filter_map(|w| {
            let ip: IpAddr = w.ip.parse().ok()?;
            Some((w.label, ip, w.fabric_port))
        })
        .collect::<Vec<_>>();
    allow_fabric_workers(guard, workers);
    Ok(())
}

pub async fn enroll_worker(
    store: &tetonic_memory::SharedStore,
    cmd: EnrollWorkerCommand,
) -> Result<EnrollWorkerResultPayload, AppError> {
    let data_dir = cmd.data_dir.clone();
    let (coordinator, owner) = store
        .write(move |db| {
            let coordinator = load_or_create_coordinator(db, &data_dir)?;
            let owner = db
                .ensure_owner_identity(&coordinator.public().0, "default")
                .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?;
            Ok::<_, AppError>((coordinator, owner))
        })
        .await
        .map_err(|e| AppError::PersistenceFailed(e.to_string()))??;

    let default_label = cmd.label.as_deref().unwrap_or("worker");
    let guard = std::sync::Arc::new(EgressGuard::new());
    let guard_clone = guard.clone();
    store
        .read(move |db| reload_enrollment_egress(db, &guard_clone))
        .await
        .map_err(|e| AppError::PersistenceFailed(e.to_string()))??;

    let (code_obj, resp, ip) = complete_enrollment(&guard, &cmd.code, default_label, &coordinator)
        .await
        .map_err(|e| AppError::InvalidRequest(format!("enrollment handshake: {e}")))?;

    let label = cmd.label.clone().unwrap_or_else(|| resp.worker_id.clone());

    guard.allow_node(&label, ip, Some(code_obj.fabric_port));

    let fabric_tls_cert = resp.fabric_tls_cert_b64.as_ref().and_then(|b64| {
        base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, b64).ok()
    });

    let coord_pks = serde_json::json!([base64_pk(&coordinator.public().0)]).to_string();
    let row = WorkerEnrollmentRow {
        id: resp.worker_id.clone(),
        estate_id: owner.id.clone(),
        label: label.clone(),
        host: code_obj.host.clone(),
        ip: ip.to_string(),
        worker_pubkey: resp.worker_pubkey.0.clone(),
        audit_pubkey: resp.audit_pubkey.0.clone(),
        fabric_port: code_obj.fabric_port,
        coordinator_pubkeys_json: coord_pks,
        enrolled_at: chrono::Utc::now().to_rfc3339(),
        last_seen: None,
        fabric_tls_cert,
        worker_trust: Some("owner_controlled_estate".into()),
    };
    store
        .write({
            let row_id = row.id.clone();
            let value = row.clone();
            move |db| {
                db.upsert_worker_enrollment(&value)
                    .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?;
                db.set_worker_trust(&row_id, "owner_controlled_estate", 0, "enrollment")
                    .map_err(|e| AppError::PersistenceFailed(format!("trust audit: {e}")))
            }
        })
        .await
        .map_err(|e| AppError::PersistenceFailed(e.to_string()))??;

    Ok(EnrollWorkerResultPayload {
        worker_id: row.id,
        label: row.label,
        host: row.host,
        ip: row.ip,
        fabric_port: row.fabric_port,
    })
}

pub async fn remove_worker(
    store: &tetonic_memory::SharedStore,
    cmd: RemoveWorkerCommand,
) -> Result<RemoveWorkerResultPayload, AppError> {
    let data_dir = cmd.data_dir.clone();
    let ref_id = cmd.ref_id.clone();
    let (coordinator, worker) = store
        .read(move |db| {
            let coordinator = load_or_create_coordinator(db, &data_dir)?;
            let worker = db
                .find_worker_enrollment(&ref_id)
                .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?
                .ok_or_else(|| AppError::InvalidRequest(format!("worker not found: {}", ref_id)))?;
            Ok::<_, AppError>((coordinator, worker))
        })
        .await
        .map_err(|e| AppError::PersistenceFailed(e.to_string()))??;

    let guard = std::sync::Arc::new(EgressGuard::new());
    let guard_clone = guard.clone();
    store
        .read(move |db| reload_enrollment_egress(db, &guard_clone))
        .await
        .map_err(|e| AppError::PersistenceFailed(e.to_string()))??;

    let ip: IpAddr = worker
        .ip
        .parse()
        .map_err(|e| AppError::InvalidRequest(format!("stored worker ip: {e}")))?;
    let epoch = chrono::Utc::now().timestamp() as u64;

    let mut revoke_pushed = false;
    if let Some(cert) = worker.fabric_tls_cert.as_deref() {
        revoke_pushed = push_revoke(
            &guard,
            ip,
            worker.fabric_port,
            cert,
            &coordinator,
            epoch,
            Some(&worker.id),
        )
        .await
        .is_ok();
    }

    let worker_id = worker.id.clone();
    let deleted = store
        .write(move |db| {
            db.delete_worker_enrollment(&worker_id)
                .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))
        })
        .await
        .map_err(|e| AppError::PersistenceFailed(e.to_string()))??;
    if !deleted {
        return Err(AppError::InvalidRequest(format!(
            "worker {} not found in store",
            worker.id
        )));
    }

    Ok(RemoveWorkerResultPayload {
        worker_id: worker.id,
        label: worker.label,
        revoke_pushed,
    })
}

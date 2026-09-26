use super::prelude::*;
use tetonic_rpc::protocol::{
    FabricWorkerTrustAuditEntry, FabricWorkerTrustGetParams, FabricWorkerTrustGetResult,
    FabricWorkerTrustSetParams, FabricWorkerTrustSetResult,
};

impl Daemon {
    pub(in crate::daemon) async fn fabric_status(&self) -> Result<Value, RpcError> {
        let services = self.services()?;
        let value = services
            .app
            .fabric_status()
            .await
            .map_err(crate::daemon::rpc::map::map_app_error)?;
        Ok(value)
    }

    pub(in crate::daemon) async fn fabric_worker_trust_set(
        &mut self,
        params: Value,
    ) -> Result<Value, RpcError> {
        if !rpc_control_plane_mutations_allowed() {
            return Err(RpcError::new(
                ErrorCode::InvalidRequest,
                "fabric/worker.trust.set is disabled when LOKAI_STRICT_RPC=1 — use tetonic CLI",
            ));
        }
        let p: FabricWorkerTrustSetParams = parse(params)?;
        let services = self.services()?;
        let res = services
            .app
            .set_fabric_worker_trust(&p.worker_id, &p.trust)
            .await
            .map_err(|e| match e {
                tetonic_app::errors::AppError::InvalidRequest(msg) => {
                    RpcError::new(ErrorCode::InvalidParams, msg)
                }
                _other => {
                    tracing::warn!("daemon hid an internal failure");
                    RpcError::new(ErrorCode::InternalError, "request failed")
                }
            })?;
        tracing::info!(worker = %res.worker_id, trust = %res.trust, epoch = res.policy_epoch, "worker trust updated");
        Ok(to_value(FabricWorkerTrustSetResult {
            ok: true,
            worker_id: res.worker_id,
            trust: res.trust,
            policy_epoch: res.policy_epoch,
        }))
    }

    pub(in crate::daemon) fn fabric_worker_trust_get(
        &self,
        params: Value,
    ) -> Result<Value, RpcError> {
        let p: FabricWorkerTrustGetParams = parse(params)?;
        let services = self.services()?;
        let res = services
            .app
            .get_fabric_worker_trust(&p.worker_id)
            .map_err(|e| match e {
                tetonic_app::errors::AppError::InvalidRequest(msg) => {
                    RpcError::new(ErrorCode::InvalidParams, msg)
                }
                _other => {
                    tracing::warn!("daemon hid an internal failure");
                    RpcError::new(ErrorCode::InternalError, "request failed")
                }
            })?;
        let audit = res
            .audit
            .into_iter()
            .map(|row| FabricWorkerTrustAuditEntry {
                trust: row.trust,
                policy_epoch: row.policy_epoch,
                recorded_at: row.recorded_at,
                source: row.source,
            })
            .collect();
        Ok(to_value(FabricWorkerTrustGetResult {
            worker_id: res.worker_id,
            trust: res.trust,
            policy_epoch: res.policy_epoch,
            audit,
        }))
    }
}

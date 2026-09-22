use crate::daemon::handlers::prelude::*;
use tetonic_app::{Application, ScannerEngine};
use tetonic_rpc::protocol::{
    AddSecretRuleRequest, AllowSecretFingerprintRequest, RevokeSecretFingerprintRequest,
};

fn map_app(err: tetonic_app::errors::AppError) -> RpcError {
    match err {
        tetonic_app::errors::AppError::InvalidRequest(m) => {
            RpcError::new(ErrorCode::InvalidParams, m)
        }
        other => RpcError::new(ErrorCode::InternalError, other.to_string()),
    }
}

pub fn handle_add_secret_rule(
    app: &Application,
    scanner: &ScannerEngine,
    req: AddSecretRuleRequest,
) -> Result<serde_json::Value, RpcError> {
    app.add_secret_rule(scanner, &req.pattern)
        .map_err(map_app)?;
    Ok(serde_json::json!({ "status": "ok" }))
}

pub async fn handle_allow_fingerprint(
    app: &Application,
    scanner: &ScannerEngine,
    req: AllowSecretFingerprintRequest,
) -> Result<serde_json::Value, RpcError> {
    let result = app
        .allow_secret_fingerprint(
            scanner,
            &req.fingerprint,
            &req.scope_kind,
            req.scope_id.as_deref(),
            req.durable,
        )
        .await
        .map_err(map_app)?;
    Ok(serde_json::json!({
        "status": "ok",
        "durable": result.durable,
        "scope_kind": result.scope_kind,
        "scope_id": result.scope_id,
        "override_id": result.override_id,
    }))
}

pub async fn handle_revoke_fingerprint(
    app: &Application,
    scanner: &ScannerEngine,
    req: RevokeSecretFingerprintRequest,
) -> Result<serde_json::Value, RpcError> {
    let result = app
        .revoke_secret_fingerprint(
            scanner,
            &req.fingerprint,
            &req.scope_kind,
            req.scope_id.as_deref(),
        )
        .await
        .map_err(map_app)?;
    Ok(serde_json::json!({
        "status": "ok",
        "revoked_durable": result.revoked_durable,
        "scope_kind": result.scope_kind,
        "scope_id": result.scope_id,
    }))
}

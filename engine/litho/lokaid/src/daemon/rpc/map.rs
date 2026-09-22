//! Application error → JSON-RPC error translation.

use tetonic_app::errors::AppError;
use tetonic_rpc::protocol::{ErrorCode, RpcError};

pub fn map_app_error(err: AppError) -> RpcError {
    match err {
        AppError::InvalidRequest(msg) => RpcError::new(ErrorCode::InvalidRequest, msg),
        AppError::SessionNotFound(msg) => RpcError::new(ErrorCode::UnknownSession, msg),
        AppError::SessionConflict => {
            RpcError::new(ErrorCode::InvalidRequest, String::from("session conflict"))
        }
        AppError::PersistenceFailed(msg) | AppError::InternalViolation(msg) => {
            RpcError::new(ErrorCode::InternalError, msg)
        }
        AppError::PolicyDenied(msg) => RpcError::new(ErrorCode::InvalidRequest, msg),
        AppError::ApprovalRequired(msg) => RpcError::new(ErrorCode::InvalidRequest, msg),
        AppError::Canceled => RpcError::new(ErrorCode::InvalidRequest, String::from("canceled")),
        AppError::WorkspaceUnavailable => RpcError::new(
            ErrorCode::InvalidRequest,
            String::from("workspace unavailable"),
        ),
        AppError::InferenceUnavailable => RpcError::new(
            ErrorCode::InternalError,
            String::from("inference unavailable"),
        ),
        AppError::ToolExecutionFailed(msg) => RpcError::new(ErrorCode::InternalError, msg),
    }
}

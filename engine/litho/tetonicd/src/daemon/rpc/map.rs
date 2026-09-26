//! Application error → JSON-RPC error translation.

use tetonic_app::errors::AppError;
use tetonic_rpc::protocol::{ErrorCode, RpcError};

pub fn map_session_error(err: AppError) -> RpcError {
    match err {
        AppError::SessionNotFound(_) | AppError::SessionConflict => {
            RpcError::new(ErrorCode::UnknownSession, "unknown session_id")
        }
        other => map_app_error(other),
    }
}

pub fn map_app_error(err: AppError) -> RpcError {
    match err {
        AppError::OrganizationCapacityExceeded => RpcError::new(ErrorCode::InvalidRequest, "organization execution capacity is occupied; retry after admitted work quiesces"),
        AppError::TeamCapacityExceeded => RpcError::new(ErrorCode::InvalidRequest, "team execution capacity is occupied; retry after admitted work quiesces"),
        AppError::PrincipalCapacityExceeded => RpcError::new(ErrorCode::InvalidRequest, "initiating principal execution capacity is occupied; retry after admitted work quiesces"),
        AppError::ExecutionCapacityExceeded => RpcError::new(
            ErrorCode::InvalidRequest,
            "registered agent already has admitted work; retry after it has quiesced",
        ),
        AppError::InvalidRequest(msg) => RpcError::new(ErrorCode::InvalidRequest, msg),
        AppError::SessionNotFound(_) => {
            RpcError::new(ErrorCode::UnknownSession, "unknown session_id")
        }
        AppError::SessionConflict => {
            RpcError::new(ErrorCode::InvalidRequest, String::from("session conflict"))
        }
        AppError::PersistenceFailed(_)
        | AppError::InternalViolation(_)
        | AppError::ToolExecutionFailed(_) => {
            tracing::warn!("daemon hid an internal failure");
            RpcError::new(ErrorCode::InternalError, "request failed")
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_failures_do_not_echo_payloads() {
        for error in [
            AppError::PersistenceFailed("audit: sqlite: PRIVATECANARY".into()),
            AppError::ToolExecutionFailed("stdout\nPRIVATECANARY".into()),
            AppError::InternalViolation("digest PRIVATECANARY".into()),
        ] {
            let mapped = map_app_error(error);
            assert_eq!(mapped.message, "request failed");
            assert!(!mapped.message.contains("PRIVATECANARY"));
        }
    }

    #[test]
    fn hidden_failures_are_not_written_to_the_log() {
        let buffer = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || LogWriter(writer.clone()))
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            let _ = map_app_error(AppError::PersistenceFailed(
                "audit: sqlite: PRIVATECANARY".into(),
            ));
        });
        let text = String::from_utf8(buffer.lock().expect("log").clone()).unwrap();
        assert!(
            !text.contains("PRIVATECANARY"),
            "log contained the hidden payload: {text}"
        );
        assert!(text.contains("daemon hid an internal failure"));
    }

    #[test]
    fn session_errors_do_not_echo_the_identifier_or_payload() {
        for error in [
            AppError::SessionNotFound("PRIVATECANARY".into()),
            AppError::SessionConflict,
            AppError::PersistenceFailed("transcript PRIVATECANARY".into()),
        ] {
            let mapped = map_session_error(error);
            assert!(!mapped.message.contains("PRIVATECANARY"), "{}", mapped.message);
        }
        assert_eq!(
            map_session_error(AppError::SessionNotFound("disc-1".into())).message,
            "unknown session_id"
        );
        let forwarded = map_app_error(AppError::SessionNotFound("PRIVATECANARY".into()));
        assert_eq!(forwarded.message, "unknown session_id");
        assert!(!forwarded.message.contains("PRIVATECANARY"));
    }

    struct LogWriter(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for LogWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("log").extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
}

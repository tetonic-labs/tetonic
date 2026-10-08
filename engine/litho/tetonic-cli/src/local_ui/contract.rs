//! HTTP mapping of the application's versioned failure contract.
use super::{json, Bytes, Full, Response, StatusCode};
use tetonic_app::errors::{AppError, FailureCode, PublicFailureV1, RecoveryAction};

pub(super) fn application_error(error: &AppError) -> Response<Full<Bytes>> {
    let failure = PublicFailureV1::from(error);
    json(status(failure.code), serde_json::json!(failure))
}

fn status(code: FailureCode) -> StatusCode {
    use FailureCode::*;
    match code {
        InvalidRequest | UnsupportedVersion => StatusCode::BAD_REQUEST,
        Unauthenticated => StatusCode::UNAUTHORIZED,
        AccessDenied => StatusCode::FORBIDDEN,
        NotFound => StatusCode::NOT_FOUND,
        StateConflict | ApprovalRequired | Canceled => StatusCode::CONFLICT,
        CapacityExhausted => StatusCode::TOO_MANY_REQUESTS,
        WorkspaceUnavailable | InferenceUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        StorageFailure | ToolFailure | InternalFailure => StatusCode::INTERNAL_SERVER_ERROR,
        UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
    }
}

/// Transport validation only. Application failures use `application_error` so
/// their categories cannot be guessed from message strings or flattened to 400.
pub(super) fn error(status: StatusCode, message: &str) -> Response<Full<Bytes>> {
    use FailureCode as Code;
    use RecoveryAction as Recovery;
    let (code, recovery) = match status {
        StatusCode::UNAUTHORIZED => (Code::Unauthenticated, Recovery::Reconnect),
        StatusCode::FORBIDDEN => (Code::AccessDenied, Recovery::CheckPermissions),
        StatusCode::NOT_FOUND => (Code::NotFound, Recovery::Refresh),
        StatusCode::UNSUPPORTED_MEDIA_TYPE => {
            (Code::UnsupportedMediaType, Recovery::CorrectRequest)
        }
        StatusCode::PAYLOAD_TOO_LARGE => (Code::PayloadTooLarge, Recovery::CorrectRequest),
        StatusCode::METHOD_NOT_ALLOWED => (Code::MethodNotAllowed, Recovery::CorrectRequest),
        StatusCode::BAD_REQUEST => (Code::InvalidRequest, Recovery::CorrectRequest),
        _ => (Code::InternalFailure, Recovery::InspectState),
    };
    json(
        status,
        serde_json::json!(PublicFailureV1::new(code, message.into(), recovery)),
    )
}

/// Missing means the original v1 client. An explicitly incompatible or ambiguous
/// version is rejected after authentication and before reading any command body.
pub(super) fn check_version(headers: &hyper::HeaderMap) -> Option<Response<Full<Bytes>>> {
    let values: Vec<_> = headers.get_all("x-tetonic-api-version").iter().collect();
    if values.is_empty() || (values.len() == 1 && values[0].as_bytes() == b"1") {
        return None;
    }
    let failure = PublicFailureV1::new(
        FailureCode::UnsupportedVersion,
        "This engine supports local API version 1.".into(),
        RecoveryAction::UpdateClient,
    );
    Some(json(StatusCode::BAD_REQUEST, serde_json::json!(failure)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn failures_have_accurate_statuses_safe_bodies_and_recovery_guidance() {
        for (failure, expected, code) in [
            (
                AppError::Conflict("Changed".into()),
                StatusCode::CONFLICT,
                "state_conflict",
            ),
            (
                AppError::PolicyDenied("Denied".into()),
                StatusCode::FORBIDDEN,
                "access_denied",
            ),
            (
                AppError::TeamCapacityExceeded,
                StatusCode::TOO_MANY_REQUESTS,
                "capacity_exhausted",
            ),
            (
                AppError::InferenceUnavailable,
                StatusCode::SERVICE_UNAVAILABLE,
                "inference_unavailable",
            ),
            (
                AppError::PersistenceFailed("PRIVATECANARY".into()),
                StatusCode::INTERNAL_SERVER_ERROR,
                "storage_failure",
            ),
            (
                AppError::InvalidRequest("Invalid".into()),
                StatusCode::BAD_REQUEST,
                "invalid_request",
            ),
        ] {
            let response = application_error(&failure);
            assert_eq!(response.status(), expected);
            assert_eq!(response.headers()["x-tetonic-api-version"], "1");
            assert_eq!(response.headers()["cache-control"], "no-store");
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(body["schema_version"], 1);
            assert_eq!(body["code"], code);
            assert!(body["error"].is_string());
            assert!(body["recovery_hint"].is_string());
            assert!(!body.to_string().contains("PRIVATECANARY"));
        }
    }

    #[test]
    fn version_guard_accepts_legacy_and_v1_but_rejects_ambiguous_or_future_versions() {
        let mut headers = hyper::HeaderMap::new();
        assert!(check_version(&headers).is_none());
        for version in ["1", "2", "", "1, 2", "garbage"] {
            headers.insert("x-tetonic-api-version", version.parse().unwrap());
            assert_eq!(check_version(&headers).is_none(), version == "1");
        }
        headers.insert("x-tetonic-api-version", "1".parse().unwrap());
        headers.append("x-tetonic-api-version", "2".parse().unwrap());
        assert!(check_version(&headers).is_some());
    }
}

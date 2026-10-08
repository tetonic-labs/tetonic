//! Version 1 failure contract. Categories are independent of HTTP; adapters map
//! them to transport statuses. Recovery is guidance, never permission to replay
//! a mutation whose outcome is unknown.
use super::AppError;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCode {
    InvalidRequest,
    Unauthenticated,
    AccessDenied,
    NotFound,
    StateConflict,
    CapacityExhausted,
    ApprovalRequired,
    Canceled,
    WorkspaceUnavailable,
    InferenceUnavailable,
    StorageFailure,
    ToolFailure,
    InternalFailure,
    UnsupportedVersion,
    UnsupportedMediaType,
    PayloadTooLarge,
    MethodNotAllowed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
    CorrectRequest,
    Reconnect,
    CheckPermissions,
    Refresh,
    WaitForCapacity,
    ReviewApproval,
    InspectState,
    CheckWorkspace,
    CheckProvider,
    CheckStorage,
    UpdateClient,
}

#[derive(Debug, Serialize)]
pub struct PublicFailureV1 {
    schema_version: u8,
    /// Retained for existing clients that display the `error` field.
    pub error: String,
    pub code: FailureCode,
    pub recovery: RecoveryAction,
    pub recovery_hint: &'static str,
}

impl PublicFailureV1 {
    /// `message` must be employee-safe text, never a raw provider, tool or store error.
    pub fn new(code: FailureCode, message: String, recovery: RecoveryAction) -> Self {
        let recovery_hint = match recovery {
            RecoveryAction::CorrectRequest => "Check the request fields before submitting again.",
            RecoveryAction::Reconnect => "Open the engine's current connection link.",
            RecoveryAction::CheckPermissions => {
                "Ask the workspace administrator to review your access."
            }
            RecoveryAction::Refresh => {
                "Refresh the current item and review it before applying changes."
            }
            RecoveryAction::WaitForCapacity => {
                "Wait for admitted work to stop using its execution slot."
            }
            RecoveryAction::ReviewApproval => {
                "Review the requested action before allowing it to proceed."
            }
            RecoveryAction::InspectState => {
                "Inspect the current work and any effects before starting it again."
            }
            RecoveryAction::CheckWorkspace => "Check that the configured workspace is available.",
            RecoveryAction::CheckProvider => "Check the model connection and its credentials.",
            RecoveryAction::CheckStorage => {
                "Check engine storage and the saved state before repeating a change."
            }
            RecoveryAction::UpdateClient => "Use a client that supports this engine API version.",
        };
        Self {
            schema_version: 1,
            error: message,
            code,
            recovery,
            recovery_hint,
        }
    }
}

impl From<&AppError> for PublicFailureV1 {
    fn from(error: &AppError) -> Self {
        use FailureCode as Code;
        use RecoveryAction as Recovery;
        let (code, recovery, message) = match error {
            AppError::InvalidRequest(_) => (
                Code::InvalidRequest,
                Recovery::CorrectRequest,
                error.employee_message(),
            ),
            AppError::SessionNotFound(_) => {
                (Code::NotFound, Recovery::Refresh, error.employee_message())
            }
            AppError::SessionConflict => (
                Code::StateConflict,
                Recovery::Refresh,
                "This session changed. Refresh it before continuing.".into(),
            ),
            AppError::Conflict(_) => (
                Code::StateConflict,
                Recovery::Refresh,
                error.employee_message(),
            ),
            AppError::PolicyDenied(_) => (
                Code::AccessDenied,
                Recovery::CheckPermissions,
                error.employee_message(),
            ),
            AppError::ApprovalRequired(_) => (
                Code::ApprovalRequired,
                Recovery::ReviewApproval,
                error.employee_message(),
            ),
            AppError::OrganizationCapacityExceeded
            | AppError::TeamCapacityExceeded
            | AppError::PrincipalCapacityExceeded
            | AppError::ExecutionCapacityExceeded => (
                Code::CapacityExhausted,
                Recovery::WaitForCapacity,
                error.employee_message(),
            ),
            AppError::Canceled => (
                Code::Canceled,
                Recovery::InspectState,
                error.employee_message(),
            ),
            AppError::WorkspaceUnavailable => (
                Code::WorkspaceUnavailable,
                Recovery::CheckWorkspace,
                error.employee_message(),
            ),
            AppError::InferenceUnavailable => (
                Code::InferenceUnavailable,
                Recovery::CheckProvider,
                error.employee_message(),
            ),
            AppError::PersistenceFailed(_) => (
                Code::StorageFailure,
                Recovery::CheckStorage,
                "The engine could not confirm the storage operation.".into(),
            ),
            AppError::ToolExecutionFailed(_) => (
                Code::ToolFailure,
                Recovery::InspectState,
                "The tool could not finish. Check the work for any effects.".into(),
            ),
            AppError::InternalViolation(_) => (
                Code::InternalFailure,
                Recovery::InspectState,
                "The engine could not complete this request.".into(),
            ),
        };
        Self::new(code, message, recovery)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::ResourceError;

    #[test]
    fn storage_and_tools_never_publish_raw_errors_or_promise_safe_replay() {
        for error in [
            AppError::PersistenceFailed("PRIVATECANARY sqlite path".into()),
            AppError::ToolExecutionFailed("PRIVATECANARY stdout".into()),
            AppError::InternalViolation("PRIVATECANARY invariant".into()),
        ] {
            let failure = PublicFailureV1::from(&error);
            let json = serde_json::to_value(&failure).unwrap();
            assert_eq!(json["schema_version"], 1);
            assert!(json["error"].is_string());
            assert!(!json.to_string().contains("PRIVATECANARY"));
            assert!(matches!(
                failure.recovery,
                RecoveryAction::CheckStorage | RecoveryAction::InspectState
            ));
        }
    }

    #[test]
    fn resource_failures_keep_their_category_through_the_application_boundary() {
        for (resource, expected) in [
            (ResourceError::Denied, FailureCode::AccessDenied),
            (ResourceError::Conflict, FailureCode::StateConflict),
            (ResourceError::Storage, FailureCode::StorageFailure),
            (ResourceError::StorageRequired, FailureCode::StorageFailure),
            (ResourceError::Invalid, FailureCode::InvalidRequest),
            (
                ResourceError::LastAdministrator,
                FailureCode::InvalidRequest,
            ),
        ] {
            let failure = PublicFailureV1::from(&AppError::from(resource));
            assert_eq!(failure.code, expected);
        }
    }
}

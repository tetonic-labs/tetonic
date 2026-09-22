//! Production OS sandbox backend (M2-3).

pub mod backend;
pub mod exec;
pub mod output;
pub mod process_executor;
pub mod profiles;
pub mod sandbox_bridge;
pub mod sync_service;
pub mod types;

#[cfg(target_os = "linux")]
pub use backend::LinuxSandboxBackend;
#[cfg(target_os = "macos")]
pub use backend::MacOsSandboxBackend;
#[cfg(windows)]
pub use backend::WindowsSandboxBackend;
pub use backend::{platform_backend, SandboxBackend};
pub use exec::*;
pub use process_executor::*;
pub use profiles::{apply_executable, apply_shell, profile_for_class};
pub use sandbox_bridge::*;
pub use sync_service::SyncLongLivedService;
pub use types::*;

use std::path::Path;

use tetonic_domain::execution::ProcessClass;

/// Predict missing OS controls for `class` on this platform without spawning.
/// The approval prompt uses this so the user sees confinement gaps before
/// they authorize a command (AUDIT H1-2).
pub fn preview_confinement(class: ProcessClass, workspace: &Path) -> ConfinementPreview {
    let caps = platform_backend().capabilities();
    let request = profile_for_class(class, workspace);
    let missing = backend::collect_missing_controls(&request, &caps);
    let user_approval_required = missing.iter().any(|m| m.risk_level >= RiskLevel::High);
    ConfinementPreview {
        platform: caps.platform,
        missing_controls: missing,
        user_approval_required,
    }
}

#[cfg(test)]
mod preview_tests {
    use super::*;
    use crate::types::{IsolationLevel, IsolationOutcome, MissingControl, SandboxError};
    use std::path::Path;
    use tetonic_domain::execution::ProcessClass;

    #[test]
    fn deny_all_shell_preview_network_honest() {
        let caps = platform_backend().capabilities();
        let preview = preview_confinement(ProcessClass::ModelRequestedShell, Path::new("."));
        if caps.network_denial {
            assert!(
                preview
                    .missing_controls
                    .iter()
                    .all(|m| m.control != SandboxControl::NetworkDenial),
                "when OS network denial is advertised, preview must not list it as missing"
            );
        } else {
            let network = preview
                .missing_controls
                .iter()
                .find(|m| m.control == SandboxControl::NetworkDenial)
                .expect("network_denial must be reported when the backend cannot enforce it");
            assert_eq!(network.risk_level, RiskLevel::High);
            assert!(preview.user_approval_required);
        }
    }

    #[test]
    fn evaluate_outcome_strict_plus_high_denies() {
        let caps = platform_backend().capabilities();
        let mut request = profile_for_class(ProcessClass::ModelRequestedShell, Path::new("."));
        request.isolation_level = IsolationLevel::Strict;
        // Force a High gap regardless of platform capabilities.
        let missing = vec![MissingControl {
            control: SandboxControl::NetworkDenial,
            reason: "forced for deny-branch test".into(),
            risk_level: RiskLevel::High,
        }];
        let err = crate::backend::evaluate_outcome(&caps, &request, Vec::new(), missing)
            .expect_err("Strict + High missing must deny");
        assert!(matches!(err, SandboxError::Denied(_)), "{err:?}");
    }

    #[test]
    fn evaluate_outcome_standard_plus_high_requires_approval_not_deny() {
        let caps = platform_backend().capabilities();
        let mut request = profile_for_class(ProcessClass::RepositoryTool, Path::new("."));
        request.isolation_level = IsolationLevel::Standard;
        let missing = vec![MissingControl {
            control: SandboxControl::NetworkDenial,
            reason: "forced".into(),
            risk_level: RiskLevel::High,
        }];
        let outcome = crate::backend::evaluate_outcome(&caps, &request, Vec::new(), missing)
            .expect("Standard + High missing must not deny");
        match outcome {
            IsolationOutcome::BrokeredWithWarning {
                user_approval_required,
                ..
            } => {
                assert!(user_approval_required);
            }
            other => panic!("expected BrokeredWithWarning, got {other:?}"),
        }
    }

    #[test]
    fn repository_tool_stays_standard() {
        let req = profile_for_class(ProcessClass::RepositoryTool, Path::new("."));
        assert_eq!(req.isolation_level, IsolationLevel::Standard);
    }
}

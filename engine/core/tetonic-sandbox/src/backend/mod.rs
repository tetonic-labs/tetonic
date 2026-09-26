//! Sandbox backend trait and shared helpers.

#[cfg(target_os = "linux")]
pub(crate) mod linux;
#[cfg(target_os = "linux")]
pub(crate) mod linux_fs;
#[cfg(target_os = "linux")]
pub(crate) mod linux_net;
#[cfg(target_os = "macos")]
pub(crate) mod macos;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) mod unix_common;
#[cfg(windows)]
pub(crate) mod windows;
#[cfg(windows)]
pub(crate) mod windows_net;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tetonic_domain::execution::ProcessClass;

use crate::types::{
    IsolationLevel, IsolationOutcome, MissingControl, NetworkPolicy, RiskLevel,
    SandboxCapabilities, SandboxControl, SandboxError, SandboxRequest, SandboxRunReport,
    SandboxedProcess,
};

#[cfg(target_os = "linux")]
pub use linux::LinuxSandboxBackend;
#[cfg(target_os = "macos")]
pub use macos::MacOsSandboxBackend;
#[cfg(windows)]
pub use windows::WindowsSandboxBackend;

#[async_trait]
pub trait SandboxBackend: Send + Sync {
    fn capabilities(&self) -> SandboxCapabilities;

    async fn execute(&self, request: SandboxRequest) -> Result<SandboxedProcess, SandboxError>;

    /// One-shot implementations retain ownership until cancellation cleanup completes.
    /// Long-lived service cancellation after launch uses the returned service handle.
    /// A backend without this contract fails closed rather than ignoring cancellation.
    async fn execute_cancellable(
        &self,
        _request: SandboxRequest,
        _cancel: tetonic_domain::work_scope::CancellationSignal,
    ) -> Result<SandboxedProcess, SandboxError> {
        Err(SandboxError::Denied(
            "backend does not support cancellable execution".into(),
        ))
    }
}

/// Platform-selected production backend.
pub fn platform_backend() -> Arc<dyn SandboxBackend> {
    #[cfg(windows)]
    {
        Arc::new(WindowsSandboxBackend::new())
    }
    #[cfg(target_os = "macos")]
    {
        Arc::new(MacOsSandboxBackend::new())
    }
    #[cfg(target_os = "linux")]
    {
        Arc::new(LinuxSandboxBackend::new())
    }
    #[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
    {
        compile_error!(
            "lokai-sandbox: unsupported unix target — add a backend or map to linux/macos"
        );
    }
}

pub(crate) fn build_minimal_env(policy: &crate::types::EnvironmentPolicy) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (k, v) in std::env::vars() {
        if policy.strip_secrets && looks_secret(&k, &v) {
            continue;
        }
        if crate::exec::is_profile_home(&k) || crate::exec::is_temp_dir_key(&k) {
            continue;
        }
        if policy.allowlist.iter().any(|a| a.eq_ignore_ascii_case(&k)) {
            out.push((k, v));
        }
    }
    for (k, v) in &policy.extra_vars {
        if policy.strip_secrets && looks_secret(k, v) {
            continue;
        }
        out.push((k.clone(), v.clone()));
    }
    if let Some(locale) = &policy.locale {
        out.push(("LC_ALL".into(), locale.clone()));
        out.push(("LANG".into(), locale.clone()));
    }
    if let Some(tmp) = &policy.controlled_temp_dir {
        let tmp = tmp.display().to_string();
        out.retain(|(key, _)| !crate::exec::is_temp_dir_key(key));
        out.push(("TMP".into(), tmp.clone()));
        out.push(("TEMP".into(), tmp.clone()));
        out.push(("TMPDIR".into(), tmp));
    }
    if let Some(home) = &policy.home_dir {
        let home = home.display().to_string();
        out.retain(|(key, _)| !crate::exec::is_profile_home(key));
        out.push(("HOME".into(), home.clone()));
        out.push(("USERPROFILE".into(), home));
    }
    out
}

fn looks_secret(key: &str, _value: &str) -> bool {
    let k = key.to_ascii_uppercase();
    if k.contains("SECRET")
        || k.contains("TOKEN")
        || k.contains("PASSWORD")
        || k.contains("API_KEY")
        || k.contains("CREDENTIAL")
        || k.contains("PRIVATE_KEY")
    {
        return true;
    }
    false
}

pub(crate) fn validate_working_directory(
    request: &SandboxRequest,
) -> Result<PathBuf, SandboxError> {
    let wd = request
        .working_directory
        .canonicalize()
        .map_err(|e| SandboxError::Denied(format!("invalid working directory: {e}")))?;
    if !path_allowed(&wd, &request.filesystem.read_roots) {
        return Err(SandboxError::Denied(
            "working directory outside allowed read roots".into(),
        ));
    }
    Ok(wd)
}

pub(crate) fn path_allowed(path: &Path, roots: &[crate::types::PathScope]) -> bool {
    let Ok(canonical) = path.canonicalize() else {
        return false;
    };
    roots.iter().any(|scope| scope_matches(&canonical, scope))
}

fn scope_matches(path: &Path, scope: &crate::types::PathScope) -> bool {
    let Ok(root) = scope.path.canonicalize() else {
        return false;
    };
    if scope.recursive {
        path.starts_with(&root)
    } else {
        path == root
    }
}

pub(crate) fn path_denied(path: &Path, denied: &[crate::types::PathScope]) -> bool {
    let Ok(canonical) = path.canonicalize() else {
        return true;
    };
    denied.iter().any(|scope| scope_matches(&canonical, scope))
}

pub fn evaluate_outcome(
    _caps: &SandboxCapabilities,
    request: &SandboxRequest,
    enforced: Vec<SandboxControl>,
    missing: Vec<MissingControl>,
) -> Result<IsolationOutcome, SandboxError> {
    if missing.is_empty() {
        return Ok(IsolationOutcome::Sandboxed {
            enforced_controls: enforced,
        });
    }
    let high_risk = missing.iter().any(|m| m.risk_level >= RiskLevel::High);
    if request.isolation_level == IsolationLevel::Strict && high_risk {
        return Err(SandboxError::Denied(format!(
            "strict isolation required but controls missing: {:?}",
            missing.iter().map(|m| &m.control).collect::<Vec<_>>()
        )));
    }
    Ok(IsolationOutcome::BrokeredWithWarning {
        enforced_controls: enforced,
        missing_controls: missing,
        // Standard profiles used to compute this as `Strict && high_risk`, which
        // is unreachable in the Ok branch (Strict+high denies). High-risk gaps
        // on Standard must still force an interactive prompt (AUDIT H1-2).
        user_approval_required: high_risk,
    })
}

/// Shared missing-control inventory used by execute and by the approval preview.
pub fn collect_missing_controls(
    request: &SandboxRequest,
    caps: &SandboxCapabilities,
) -> Vec<MissingControl> {
    let mut missing = missing_fs_controls(request, caps);
    if let Some(m) = missing_network_policy(request, caps) {
        missing.push(m);
    }
    if !caps.child_breakaway_prevention {
        missing.push(MissingControl {
            control: SandboxControl::ChildBreakawayPrevention,
            reason: "backend does not prevent child breakaway from the process tree".into(),
            risk_level: RiskLevel::Medium,
        });
    }
    if request.resources.max_child_processes.is_some() && !caps.process_count_limits {
        missing.push(MissingControl {
            control: SandboxControl::ProcessCountLimit,
            reason: "process count limits not enforced by this backend".into(),
            risk_level: RiskLevel::Medium,
        });
    }
    missing
}

pub(crate) fn missing_network_policy(
    request: &SandboxRequest,
    caps: &SandboxCapabilities,
) -> Option<MissingControl> {
    match request.network {
        NetworkPolicy::DenyAll | NetworkPolicy::AllowLoopback => {
            if !caps.network_denial {
                Some(MissingControl {
                    control: SandboxControl::NetworkDenial,
                    reason: "OS-level network filtering not available; broker-only policy".into(),
                    risk_level: RiskLevel::High,
                })
            } else {
                None
            }
        }
        NetworkPolicy::AllowDestinations(_) => {
            if !caps.network_allowlisting {
                Some(MissingControl {
                    control: SandboxControl::NetworkAllowlist,
                    reason: "destination allowlist not enforced by OS backend".into(),
                    risk_level: RiskLevel::High,
                })
            } else {
                None
            }
        }
        NetworkPolicy::InheritBrokered => None,
    }
}

pub(crate) fn missing_fs_controls(
    request: &SandboxRequest,
    caps: &SandboxCapabilities,
) -> Vec<MissingControl> {
    let mut missing = Vec::new();
    if !request.filesystem.read_roots.is_empty() && !caps.filesystem_read_restrictions {
        missing.push(MissingControl {
            control: SandboxControl::FilesystemReadRestriction,
            reason: "filesystem read scope validated at broker; no OS sandbox filter".into(),
            risk_level: RiskLevel::Medium,
        });
    }
    if !request.filesystem.write_roots.is_empty() && !caps.filesystem_write_restrictions {
        missing.push(MissingControl {
            control: SandboxControl::FilesystemWriteRestriction,
            reason: "filesystem write scope validated at broker; no OS sandbox filter".into(),
            risk_level: RiskLevel::Medium,
        });
    }
    missing
}

pub(crate) fn make_report(
    caps: SandboxCapabilities,
    outcome: IsolationOutcome,
    process_class: ProcessClass,
) -> SandboxRunReport {
    let user_message = match &outcome {
        IsolationOutcome::Sandboxed { .. } => {
            format!("Process ran in OS sandbox ({})", caps.platform)
        }
        IsolationOutcome::BrokeredWithWarning { .. } => {
            format!(
                "Process ran with partial containment ({}, class {:?})",
                caps.platform, process_class
            )
        }
        IsolationOutcome::Denied { reason } => reason.clone(),
    };
    let audit_summary = serde_json::json!({
        "outcome": outcome,
        "capabilities": caps,
        "process_class": process_class,
    })
    .to_string();
    SandboxRunReport {
        outcome,
        capabilities: caps,
        user_message,
        audit_summary,
    }
}

//! Sandbox domain types (M2-3).

use std::path::PathBuf;
use std::time::Duration;

use lokai_domain::execution::{ProcessClass, TraceContext};
use lokai_domain::ids::WorkspaceVersion;
use serde::{Deserialize, Serialize};

/// Required isolation strictness for a sandbox request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IsolationLevel {
    /// All requested controls must be OS-enforced or execution is denied/downgraded.
    Strict,
    /// Broker may proceed with explicit warnings for missing controls.
    Standard,
}

/// How a process was actually isolated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum IsolationOutcome {
    Sandboxed {
        enforced_controls: Vec<SandboxControl>,
    },
    BrokeredWithWarning {
        enforced_controls: Vec<SandboxControl>,
        missing_controls: Vec<MissingControl>,
        user_approval_required: bool,
    },
    Denied {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxControl {
    ProcessTreeContainment,
    ChildBreakawayPrevention,
    RuntimeLimit,
    MemoryLimit,
    CpuLimit,
    ProcessCountLimit,
    OutputLimit,
    EnvironmentFiltering,
    WorkingDirectoryRestriction,
    FilesystemReadRestriction,
    FilesystemWriteRestriction,
    NetworkDenial,
    NetworkAllowlist,
    HandleInheritanceRestriction,
    LongLivedService,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissingControl {
    pub control: SandboxControl,
    pub reason: String,
    pub risk_level: RiskLevel,
}

impl MissingControl {
    pub fn wire_control(&self) -> String {
        serde_json::to_value(&self.control)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| format!("{:?}", self.control))
    }

    pub fn wire_risk(&self) -> String {
        serde_json::to_value(self.risk_level)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| format!("{:?}", self.risk_level))
    }
}

/// Predicted isolation gaps for a process class, without spawning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfinementPreview {
    pub platform: String,
    pub missing_controls: Vec<MissingControl>,
    pub user_approval_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkPolicy {
    DenyAll,
    AllowLoopback,
    AllowDestinations(Vec<NetworkDestination>),
    InheritBrokered,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkDestination {
    pub host: String,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathScope {
    pub path: PathBuf,
    /// When true, match prefix (directory tree).
    pub recursive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilesystemPolicy {
    pub read_roots: Vec<PathScope>,
    pub write_roots: Vec<PathScope>,
    pub denied_paths: Vec<PathScope>,
    pub temporary_root: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentPolicy {
    pub allowlist: Vec<String>,
    pub extra_vars: Vec<(String, String)>,
    pub strip_secrets: bool,
    pub controlled_temp_dir: Option<PathBuf>,
    pub locale: Option<String>,
}

impl Default for EnvironmentPolicy {
    fn default() -> Self {
        Self {
            allowlist: default_env_allowlist(),
            extra_vars: Vec::new(),
            strip_secrets: true,
            controlled_temp_dir: None,
            locale: Some("C.UTF-8".into()),
        }
    }
}

pub fn default_env_allowlist() -> Vec<String> {
    [
        "PATH",
        "PATHEXT",
        "HOME",
        "USERPROFILE",
        "SYSTEMROOT",
        "SystemRoot",
        "WINDIR",
        "ComSpec",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "TMP",
        "TEMP",
        "OS",
        "ProgramFiles",
        "ProgramFiles(x86)",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub max_memory_bytes: Option<u64>,
    pub max_cpu_percent: Option<u32>,
    pub max_child_processes: Option<u32>,
    pub max_output_bytes_per_stream: usize,
    pub max_line_length: Option<usize>,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_memory_bytes: Some(512 * 1024 * 1024),
            max_cpu_percent: None,
            max_child_processes: Some(32),
            max_output_bytes_per_stream: 60_000,
            max_line_length: Some(16_384),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StdinPolicy {
    Null,
    Pipe,
    Interactive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessMode {
    OneShot,
    LongLived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxRequest {
    pub executable: String,
    pub arguments: Vec<String>,
    pub shell_identity: Option<String>,
    pub shell_script: Option<String>,
    pub working_directory: PathBuf,
    pub process_class: ProcessClass,
    pub filesystem: FilesystemPolicy,
    pub network: NetworkPolicy,
    pub environment: EnvironmentPolicy,
    pub resources: ResourceLimits,
    pub runtime_limit: Duration,
    pub child_process_limit: Option<u32>,
    pub stdin_policy: StdinPolicy,
    pub workspace_version: Option<WorkspaceVersion>,
    pub isolation_level: IsolationLevel,
    pub mode: ProcessMode,
    pub trace_context: TraceContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxCapabilities {
    pub process_tree_containment: bool,
    pub child_breakaway_prevention: bool,
    pub runtime_enforcement: bool,
    pub memory_limits: bool,
    pub cpu_limits: bool,
    pub process_count_limits: bool,
    pub output_limits: bool,
    pub environment_filtering: bool,
    pub filesystem_read_restrictions: bool,
    pub filesystem_write_restrictions: bool,
    pub network_denial: bool,
    pub network_allowlisting: bool,
    pub long_lived_process_support: bool,
    pub interactive_stdin_support: bool,
    pub supported_process_classes: Vec<ProcessClass>,
    pub platform: String,
    pub mechanisms: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxRunReport {
    pub outcome: IsolationOutcome,
    pub capabilities: SandboxCapabilities,
    pub user_message: String,
    pub audit_summary: String,
}

#[derive(Debug, Clone)]
pub struct SandboxRunResult {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub report: SandboxRunReport,
    pub truncated: bool,
}

/// Completed one-shot run or started long-lived service.
pub enum SandboxedProcess {
    Completed(SandboxRunResult),
    Service(LongLivedSandboxHandle),
}

/// Resource accounting for a long-lived sandbox service (audit correlation).
#[derive(Debug, Clone)]
pub struct ServiceResourceAccounting {
    pub execution_id: String,
    pub started_at: std::time::Instant,
    pub restart_count: u32,
    pub process_class: ProcessClass,
}

/// Handle for a long-lived sandboxed service (LSP, etc.).
pub struct LongLivedSandboxHandle {
    pub report: SandboxRunReport,
    pub(crate) inner: LongLivedInner,
}

pub(crate) enum LongLivedInner {
    #[cfg(windows)]
    Windows(crate::backend::windows::WindowsLongLived),
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    Unix(Box<crate::backend::unix_common::UnixLongLived>),
}

impl LongLivedSandboxHandle {
    pub fn report(&self) -> &SandboxRunReport {
        &self.report
    }

    pub async fn write_stdin(&mut self, data: &[u8]) -> Result<(), SandboxError> {
        match &mut self.inner {
            #[cfg(windows)]
            LongLivedInner::Windows(h) => h.write_stdin(data).await,
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            LongLivedInner::Unix(h) => h.write_stdin(data).await,
        }
    }

    pub async fn read_stdout(&mut self, max_bytes: usize) -> Result<Vec<u8>, SandboxError> {
        match &mut self.inner {
            #[cfg(windows)]
            LongLivedInner::Windows(h) => h.read_stdout(max_bytes).await,
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            LongLivedInner::Unix(h) => h.read_stdout(max_bytes).await,
        }
    }

    pub async fn graceful_stop(&mut self, grace: Duration) -> Result<(), SandboxError> {
        match &mut self.inner {
            #[cfg(windows)]
            LongLivedInner::Windows(h) => h.graceful_stop(grace).await,
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            LongLivedInner::Unix(h) => h.graceful_stop(grace).await,
        }
    }

    pub async fn force_terminate(&mut self) -> Result<(), SandboxError> {
        match &mut self.inner {
            #[cfg(windows)]
            LongLivedInner::Windows(h) => h.force_terminate().await,
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            LongLivedInner::Unix(h) => h.force_terminate().await,
        }
    }

    pub fn is_alive(&self) -> bool {
        match &self.inner {
            #[cfg(windows)]
            LongLivedInner::Windows(h) => h.is_alive(),
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            LongLivedInner::Unix(h) => h.is_alive(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("sandbox denied: {0}")]
    Denied(String),
    #[error("sandbox spawn failed: {0}")]
    SpawnFailed(String),
    #[error("sandbox io error: {0}")]
    Io(String),
    #[error("sandbox timeout")]
    Timeout,
    #[error("sandbox canceled")]
    Canceled,
    #[error("sandbox internal: {0}")]
    Internal(String),
}

impl SandboxRunReport {
    pub fn format_user_visible(&self) -> String {
        match &self.outcome {
            IsolationOutcome::Sandboxed { .. } => self.user_message.clone(),
            IsolationOutcome::BrokeredWithWarning {
                missing_controls,
                user_approval_required,
                ..
            } => {
                let missing: Vec<_> = missing_controls
                    .iter()
                    .map(|m| format!("{:?}: {}", m.control, m.reason))
                    .collect();
                format!(
                    "{}\nWARNING: partial sandbox — missing controls: {}. User approval required: {}",
                    self.user_message,
                    missing.join("; "),
                    user_approval_required
                )
            }
            IsolationOutcome::Denied { reason } => format!("Execution denied: {reason}"),
        }
    }
}

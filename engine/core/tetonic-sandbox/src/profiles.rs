//! Process-class sandbox profiles (M2-3).

use std::path::{Path, PathBuf};
use std::time::Duration;

use tetonic_domain::execution::ProcessClass;

use crate::types::{
    default_env_allowlist, EnvironmentPolicy, FilesystemPolicy, IsolationLevel, NetworkPolicy,
    PathScope, ProcessMode, ResourceLimits, SandboxRequest, StdinPolicy,
};

/// Strict when this platform can enforce OS network denial; otherwise Standard
/// so H1-2 warn-and-prompt remains usable (H1-3).
fn shell_verify_isolation() -> IsolationLevel {
    if crate::platform_backend().capabilities().network_denial {
        IsolationLevel::Strict
    } else {
        IsolationLevel::Standard
    }
}

/// Well-known coordinator / secret paths denied on all profiles.
pub fn default_denied_paths(workspace: &Path) -> Vec<PathScope> {
    let mut denied = vec![path_scope("\\\\.\\", false), path_scope("\\\\?\\", false)];
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let home = PathBuf::from(home);
        denied.push(path_scope(home.join(".ssh"), true));
        denied.push(path_scope(home.join(".aws"), true));
        denied.push(path_scope(home.join(".lokai"), true));
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        denied.push(path_scope(PathBuf::from(appdata).join("lokai"), true));
    }
    denied.push(path_scope(workspace.join("..").join("lokai.db"), false));
    denied
}

fn path_scope(path: impl Into<PathBuf>, recursive: bool) -> PathScope {
    PathScope {
        path: path.into(),
        recursive,
    }
}

fn workspace_roots(workspace: &Path) -> (Vec<PathScope>, Vec<PathScope>) {
    let ws = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    (
        vec![PathScope {
            path: ws.clone(),
            recursive: true,
        }],
        vec![PathScope {
            path: ws,
            recursive: true,
        }],
    )
}

pub fn profile_for_class(class: ProcessClass, workspace: &Path) -> SandboxRequest {
    let (read_roots, write_roots) = workspace_roots(workspace);
    let denied = default_denied_paths(workspace);
    let temp = std::env::temp_dir().join(format!("lokai-sandbox-{}", std::process::id()));

    match class {
        ProcessClass::InternalService => SandboxRequest {
            executable: String::new(),
            arguments: vec![],
            shell_identity: None,
            shell_script: None,
            working_directory: workspace.to_path_buf(),
            process_class: class,
            filesystem: FilesystemPolicy {
                read_roots: read_roots.clone(),
                write_roots: vec![PathScope {
                    path: temp.clone(),
                    recursive: true,
                }],
                denied_paths: denied,
                temporary_root: Some(temp),
            },
            network: NetworkPolicy::DenyAll,
            environment: EnvironmentPolicy {
                allowlist: default_env_allowlist(),
                extra_vars: vec![],
                strip_secrets: true,
                controlled_temp_dir: Some(std::env::temp_dir()),
                locale: Some("C.UTF-8".into()),
            },
            resources: ResourceLimits {
                max_memory_bytes: Some(768 * 1024 * 1024),
                max_cpu_percent: None,
                max_child_processes: Some(16),
                max_output_bytes_per_stream: 60_000,
                max_line_length: Some(16_384),
            },
            runtime_limit: Duration::from_secs(3600),
            child_process_limit: Some(16),
            stdin_policy: StdinPolicy::Pipe,
            workspace_version: None,
            isolation_level: IsolationLevel::Standard,
            mode: ProcessMode::LongLived,
            trace_context: Default::default(),
        },
        ProcessClass::RepositoryTool => SandboxRequest {
            executable: String::new(),
            arguments: vec![],
            shell_identity: None,
            shell_script: None,
            working_directory: workspace.to_path_buf(),
            process_class: class,
            filesystem: FilesystemPolicy {
                read_roots: read_roots.clone(),
                write_roots: write_roots.clone(),
                denied_paths: denied,
                temporary_root: Some(temp),
            },
            network: NetworkPolicy::DenyAll,
            environment: EnvironmentPolicy::default(),
            resources: ResourceLimits {
                max_memory_bytes: Some(512 * 1024 * 1024),
                max_child_processes: Some(24),
                ..ResourceLimits::default()
            },
            runtime_limit: Duration::from_secs(300),
            child_process_limit: Some(24),
            stdin_policy: StdinPolicy::Null,
            workspace_version: None,
            isolation_level: IsolationLevel::Standard,
            mode: ProcessMode::OneShot,
            trace_context: Default::default(),
        },
        ProcessClass::BuildVerification => SandboxRequest {
            executable: String::new(),
            arguments: vec![],
            shell_identity: None,
            shell_script: None,
            working_directory: workspace.to_path_buf(),
            process_class: class,
            filesystem: FilesystemPolicy {
                read_roots: read_roots.clone(),
                write_roots: write_roots.clone(),
                denied_paths: denied,
                temporary_root: Some(temp.clone()),
            },
            network: NetworkPolicy::DenyAll,
            environment: EnvironmentPolicy {
                controlled_temp_dir: Some(temp),
                ..EnvironmentPolicy::default()
            },
            resources: ResourceLimits {
                max_memory_bytes: Some(1024 * 1024 * 1024),
                max_child_processes: Some(48),
                max_output_bytes_per_stream: 60_000,
                ..ResourceLimits::default()
            },
            runtime_limit: Duration::from_secs(600),
            child_process_limit: Some(48),
            stdin_policy: StdinPolicy::Null,
            workspace_version: None,
            isolation_level: shell_verify_isolation(),
            mode: ProcessMode::OneShot,
            trace_context: Default::default(),
        },
        ProcessClass::ModelRequestedShell => SandboxRequest {
            executable: String::new(),
            arguments: vec![],
            shell_identity: None,
            shell_script: None,
            working_directory: workspace.to_path_buf(),
            process_class: class,
            filesystem: FilesystemPolicy {
                read_roots: read_roots.clone(),
                write_roots: write_roots.clone(),
                denied_paths: denied,
                temporary_root: Some(temp),
            },
            network: NetworkPolicy::DenyAll,
            environment: EnvironmentPolicy::default(),
            resources: ResourceLimits {
                max_memory_bytes: Some(256 * 1024 * 1024),
                max_child_processes: Some(8),
                max_output_bytes_per_stream: 60_000,
                ..ResourceLimits::default()
            },
            runtime_limit: Duration::from_secs(120),
            child_process_limit: Some(8),
            stdin_policy: StdinPolicy::Null,
            workspace_version: None,
            isolation_level: shell_verify_isolation(),
            mode: ProcessMode::OneShot,
            trace_context: Default::default(),
        },
        ProcessClass::HardwareProbe => SandboxRequest {
            executable: String::new(),
            arguments: vec![],
            shell_identity: None,
            shell_script: None,
            working_directory: workspace.to_path_buf(),
            process_class: class,
            filesystem: FilesystemPolicy {
                read_roots,
                write_roots: vec![],
                denied_paths: denied,
                temporary_root: None,
            },
            network: NetworkPolicy::DenyAll,
            environment: EnvironmentPolicy::default(),
            resources: ResourceLimits {
                max_memory_bytes: Some(128 * 1024 * 1024),
                max_child_processes: Some(1),
                ..ResourceLimits::default()
            },
            runtime_limit: Duration::from_secs(30),
            child_process_limit: Some(1),
            stdin_policy: StdinPolicy::Null,
            workspace_version: None,
            isolation_level: IsolationLevel::Standard,
            mode: ProcessMode::OneShot,
            trace_context: Default::default(),
        },
    }
}

pub fn apply_executable(mut req: SandboxRequest, exe: &str, args: &[String]) -> SandboxRequest {
    req.executable = exe.to_string();
    req.arguments = args.to_vec();
    req
}

pub fn apply_shell(mut req: SandboxRequest, shell: &str, script: &str) -> SandboxRequest {
    req.shell_identity = Some(shell.to_string());
    req.shell_script = Some(script.to_string());
    req
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_verification_denies_network() {
        let p = profile_for_class(ProcessClass::BuildVerification, Path::new("/ws"));
        assert!(matches!(p.network, NetworkPolicy::DenyAll));
    }
}

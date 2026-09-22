//! Architecture invariant checks (extended rules).

use crate::{collect_rs_files, is_allowed_subprocess, rel, Violation};
use std::path::Path;

pub fn check_lokaid_session_authority(root: &Path) -> Vec<Violation> {
    let mut out = Vec::new();
    let daemon = root.join("litho/lokaid/src/daemon.rs");
    let daemon_text = std::fs::read_to_string(&daemon).unwrap_or_default();
    if daemon_text.contains("sessions: HashMap") || daemon_text.contains("SessionState") {
        out.push(Violation {
            rule: "lokaid_session_authority",
            path: daemon,
            detail: "Daemon must not own a SessionState HashMap; live sessions live in lokai-app"
                .into(),
        });
    }
    let dir = root.join("litho/lokaid/src/daemon/handlers");
    for path in collect_rs_files(&dir) {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        for pat in ["SessionState {", "sessions.insert", "convo.borrow"] {
            if text.contains(pat) {
                out.push(Violation {
                    rule: "lokaid_session_authority",
                    path: path.clone(),
                    detail: format!(
                        "handlers must not mutate conversation/session slots directly (`{pat}`)"
                    ),
                });
            }
        }
    }
    out
}

/// H2-1: turn admission must not branch on `gates_ok` inside bins.
/// Display/logging in capacity CLI, node banner, and fabric mapping remain allowed.
pub fn check_no_gates_ok_turn_abort(root: &Path) -> Vec<Violation> {
    const ALLOW: &[&str] = &[
        "litho/lokai-cli/src/capacity.rs",
        "litho/lokaid/src/node.rs",
        "litho/lokaid/src/daemon/compute.rs",
        "litho/lokaid/src/daemon/helpers.rs",
        "litho/lokaid/src/daemon/handlers/capacity.rs",
    ];
    let mut out = Vec::new();
    for crate_rel in ["litho/lokai-cli/src", "litho/lokaid/src"] {
        let dir = root.join(crate_rel);
        for path in collect_rs_files(&dir) {
            let rel_path = rel(root, &path);
            if ALLOW.iter().any(|a| rel_path == *a) {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            if text.contains("gates_ok") {
                out.push(Violation {
                    rule: "no_gates_ok_turn_abort",
                    path,
                    detail: "turn-admission paths must not branch on `gates_ok`; use lokai-app::admit_chat_turn".into(),
                });
            }
        }
    }
    out
}

/// R4-1 / CAP-01: product composition must wire ContextCompiler; context provider attaches secret scanner.
pub fn check_context_compiler_wired(root: &Path) -> Vec<Violation> {
    let mut out = Vec::new();
    let assembly = crate::resolve_path(
        root,
        &[
            "core/tetonic-runtime/src/assembly.rs",
            "core/lokai-runtime/src/assembly.rs",
        ],
    );
    let text = std::fs::read_to_string(&assembly).unwrap_or_default();
    if !text.contains("with_context_compiler") {
        out.push(Violation {
            rule: "context_compiler_wired",
            path: assembly,
            detail: "EngineRuntime::assemble_agent must support with_context_compiler".into(),
        });
    }
    let turn = crate::resolve_path(
        root,
        &[
            "litho/tetonic-app/src/turn_execution.rs",
            "litho/lokai-app/src/turn_execution.rs",
        ],
    );
    let turn_text = std::fs::read_to_string(&turn).unwrap_or_default();
    if !turn_text.contains("assemble_agent")
        || !turn_text.contains("build_production_context_compiler")
    {
        out.push(Violation {
            rule: "context_compiler_wired",
            path: turn,
            detail: "turn_execution::build_agent must wire ContextCompiler through EngineRuntime"
                .into(),
        });
    }
    let provider = crate::resolve_path(
        root,
        &[
            "strata/tetonic-context/src/workspace.rs",
            "strata/lokai-context/src/workspace.rs",
        ],
    );
    let provider_text = std::fs::read_to_string(&provider).unwrap_or_default();
    if !provider_text.contains("with_secret_scanner") {
        out.push(Violation {
            rule: "context_compiler_wired",
            path: provider,
            detail:
                "build_production_context_compiler must attach ScannerEngine via with_secret_scanner"
                    .into(),
        });
    }
    out
}

/// H1-1: production Infer is assembled with ScannerEngine; lokai-secrets
/// must not depend on the unwired ContextCompiler crate.
pub fn check_outbound_secret_scanner(root: &Path) -> Vec<Violation> {
    let mut out = Vec::new();
    let secrets_cargo = crate::resolve_path(
        root,
        &[
            "core/tetonic-secrets/Cargo.toml",
            "core/lokai-secrets/Cargo.toml",
        ],
    );
    let secrets_text = std::fs::read_to_string(&secrets_cargo).unwrap_or_default();
    if secrets_text.contains("lokai-context") || secrets_text.contains("tetonic-context") {
        out.push(Violation {
            rule: "outbound_secret_scanner",
            path: secrets_cargo,
            detail: "secrets crate must not path-depend on context crate".into(),
        });
    }
    let plane = crate::resolve_path(
        root,
        &[
            "litho/tetonic-app/src/compute_plane.rs",
            "litho/lokai-app/src/compute_plane.rs",
        ],
    );
    let plane_text = std::fs::read_to_string(&plane).unwrap_or_default();
    if !plane_text.contains("with_outbound_scanner") {
        out.push(Violation {
            rule: "outbound_secret_scanner",
            path: plane,
            detail: "wrap_pooled_with_broker must attach ScannerEngine via with_outbound_scanner"
                .into(),
        });
    }
    let adapter = crate::resolve_path(
        root,
        &[
            "mantle/tetonic-broker/src/adapters/inference.rs",
            "mantle/lokai-broker/src/adapters/inference.rs",
        ],
    );
    let adapter_text = std::fs::read_to_string(&adapter).unwrap_or_default();
    if !adapter_text.contains("fn redact_outbound") || !adapter_text.contains("scan_outbound") {
        out.push(Violation {
            rule: "outbound_secret_scanner",
            path: adapter,
            detail: "BrokerInferenceProvider must scan Infer content at one chokepoint".into(),
        });
    }
    out
}

/// H3-1: resume cap lives in lokai-app. A copy in bins/ is the B3 divergence.
pub fn check_no_duplicate_resume_cap(root: &Path) -> Vec<Violation> {
    let mut out = Vec::new();
    for crate_rel in ["litho/lokai-cli/src", "litho/lokaid/src"] {
        let dir = root.join(crate_rel);
        for path in collect_rs_files(&dir) {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            if text.contains("RESUME_MESSAGE_CAP") {
                out.push(Violation {
                    rule: "no_duplicate_resume_cap",
                    path,
                    detail: "RESUME_MESSAGE_CAP must live only in lokai-app::resume".into(),
                });
            }
        }
    }
    out
}

/// R26: enrollment egress reload + coordinator key helpers live only in lokai-app.
pub fn check_no_duplicate_enrollment_helpers(root: &Path) -> Vec<Violation> {
    let mut out = Vec::new();
    let def_re = regex::Regex::new(
        r"(?m)^\s*(pub\s+)?fn\s+(reload_enrollment_egress|load_or_create_coordinator)\s*\(",
    )
    .expect("regex");
    for crate_rel in ["litho/lokai-cli/src", "litho/lokaid/src"] {
        let dir = root.join(crate_rel);
        for path in collect_rs_files(&dir) {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            if def_re.is_match(&text) {
                out.push(Violation {
                    rule: "no_duplicate_enrollment_helpers",
                    path,
                    detail: "reload_enrollment_egress / load_or_create_coordinator must live only in lokai-app::estate_enrollment"
                        .into(),
                });
            }
        }
    }
    out
}

/// M6-1 / R3-1: production inference must enter through ComputeBroker.
/// Shared builder lives in lokai-app; CLI and daemon both call it.
pub fn check_compute_broker_wiring(root: &Path) -> Vec<Violation> {
    let mut out = Vec::new();
    let cargo = crate::resolve_path(
        root,
        &["litho/tetonic-app/Cargo.toml", "litho/lokai-app/Cargo.toml"],
    );
    let cargo_text = std::fs::read_to_string(&cargo).unwrap_or_default();
    if !cargo_text.contains("lokai-broker") && !cargo_text.contains("tetonic-broker") {
        out.push(Violation {
            rule: "compute_broker_wiring",
            path: cargo,
            detail: "lokai-app must depend on broker crate".into(),
        });
    }
    let plane = crate::resolve_path(
        root,
        &[
            "litho/tetonic-app/src/compute_plane.rs",
            "litho/lokai-app/src/compute_plane.rs",
        ],
    );
    let plane_text = std::fs::read_to_string(&plane).unwrap_or_default();
    if !plane_text.contains("BrokerInferenceProvider")
        || !plane_text.contains("wrap_pooled_with_broker")
    {
        out.push(Violation {
            rule: "compute_broker_wiring",
            path: plane.clone(),
            detail: "shared compute plane must wrap PooledProvider with BrokerInferenceProvider"
                .into(),
        });
    }
    if !plane_text.contains("PolicyDispatchGuard") {
        out.push(Violation {
            rule: "compute_broker_wiring",
            path: plane,
            detail: "shared compute plane must attach PolicyDispatchGuard".into(),
        });
    }
    let daemon = crate::resolve_path(
        root,
        &[
            "litho/tetonic-app/src/daemon_bootstrap.rs",
            "litho/lokai-app/src/daemon_bootstrap.rs",
        ],
    );
    let daemon_text = std::fs::read_to_string(&daemon).unwrap_or_default();
    if !daemon_text.contains("build_compute_plane") {
        out.push(Violation {
            rule: "compute_broker_wiring",
            path: daemon,
            detail: "daemon bootstrap must call the shared lokai-app compute-plane builder".into(),
        });
    }
    let cli_boot = crate::resolve_path(
        root,
        &[
            "litho/tetonic-app/src/cli_bootstrap.rs",
            "litho/lokai-app/src/cli_bootstrap.rs",
        ],
    );
    let cli_boot_text = std::fs::read_to_string(&cli_boot).unwrap_or_default();
    if !cli_boot_text.contains("build_compute_plane") {
        out.push(Violation {
            rule: "compute_broker_wiring",
            path: cli_boot,
            detail: "cli bootstrap must call the shared compute-plane builder".into(),
        });
    }
    let session = root.join("litho/lokai-cli/src/session.rs");
    let session_text = std::fs::read_to_string(&session).unwrap_or_default();
    if session_text.contains("compute_broker: None") {
        out.push(Violation {
            rule: "compute_broker_wiring",
            path: session,
            detail: "CLI TurnExecutionHost must not hard-code compute_broker: None".into(),
        });
    }
    let broker_lib = crate::resolve_path(
        root,
        &[
            "mantle/tetonic-broker/src/lib.rs",
            "mantle/lokai-broker/src/lib.rs",
        ],
    );
    if !broker_lib.is_file() {
        out.push(Violation {
            rule: "compute_broker_wiring",
            path: broker_lib,
            detail: "broker crate missing".into(),
        });
    }
    out
}

/// R3-2: leftover CLI commands must be labeled infrastructure-only.
pub fn check_cli_infra_leftovers(root: &Path) -> Vec<Violation> {
    let path = root.join("litho/lokai-cli/src/offline.rs");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    if !text.contains("infrastructure-only") {
        vec![Violation {
            rule: "cli_infra_leftovers",
            path,
            detail: "offline.rs must document index/time-travel/history as infrastructure-only"
                .into(),
        }]
    } else {
        Vec::new()
    }
}

/// R3-2: TUI inspector must not spawn subprocesses.
pub fn check_cli_inspector_no_command(root: &Path) -> Vec<Violation> {
    let path = root.join("litho/lokai-cli/src/chat.rs");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    if text.contains("Command::new") {
        vec![Violation {
            rule: "cli_inspector_no_command",
            path,
            detail: "inspector must not call tokio::process::Command / std::process::Command"
                .into(),
        }]
    } else {
        Vec::new()
    }
}

/// R29: production LSP must not keep a raw Command allowlist escape in lokai-lsp.
pub fn check_lsp_via_process_broker(root: &Path) -> Vec<Violation> {
    let mut out = Vec::new();
    let launcher = crate::resolve_path(
        root,
        &[
            "litho/lokai-lsp/src/launcher.rs",
            "litho/tetonic-lsp/src/launcher.rs",
        ],
    );
    let text = std::fs::read_to_string(&launcher).unwrap_or_default();
    if text.contains("Command::new") {
        out.push(Violation {
            rule: "lsp_via_process_broker",
            path: launcher,
            detail: "lsp launcher must not spawn via Command::new (use SandboxLspLauncher)".into(),
        });
    }
    if is_allowed_subprocess("litho/lokai-lsp/src/launcher.rs") {
        out.push(Violation {
            rule: "lsp_via_process_broker",
            path: crate::resolve_path(
                root,
                &[
                    "tooling/tetonic-arch-gate/src/lib.rs",
                    "tooling/lokai-arch-gate/src/lib.rs",
                ],
            ),
            detail: "lsp launcher must not be on subprocess allowlist (R29)".into(),
        });
    }
    let tools_lsp = crate::resolve_path(
        root,
        &[
            "litho/tetonic-app/src/lsp_launcher.rs",
            "litho/lokai-app/src/lsp_launcher.rs",
        ],
    );
    let tools_text = std::fs::read_to_string(&tools_lsp).unwrap_or_default();
    if !tools_text.contains("SyncLongLivedService") || !tools_text.contains("SandboxLspLauncher") {
        out.push(Violation {
            rule: "lsp_via_process_broker",
            path: tools_lsp,
            detail: "production LSP spawn must use SandboxLspLauncher + SyncLongLivedService"
                .into(),
        });
    }
    out
}

/// R09: agent/tool git must not use raw `Command::new("git")` (sandbox/broker only).
pub fn check_git_via_process_broker(root: &Path) -> Vec<Violation> {
    let mut out = Vec::new();
    let pe = crate::resolve_path(
        root,
        &[
            "litho/lokai-tools/src/process_executor.rs",
            "litho/tetonic-tools/src/process_executor.rs",
        ],
    );
    let text = std::fs::read_to_string(&pe).unwrap_or_default();
    if text.contains("Command::new(\"git\")") || text.contains("Command::new('git')") {
        out.push(Violation {
            rule: "git_via_process_broker",
            path: pe,
            detail: "ProcessExecutor must not spawn git via raw Command::new (use sandbox_run_direct / broker)"
                .into(),
        });
    }
    let wt = crate::resolve_path(
        root,
        &[
            "litho/lokai-tools/src/worktree.rs",
            "litho/tetonic-tools/src/worktree.rs",
        ],
    );
    let wt_text = std::fs::read_to_string(&wt).unwrap_or_default();
    if wt_text.contains("Command::new") {
        out.push(Violation {
            rule: "git_via_process_broker",
            path: wt,
            detail: "worktree git must use ProcessExecutor::run_git / run_git_status".into(),
        });
    }
    out
}

/// R6-2: production Tools / worktree must use OS sandbox, not Constrained raw Command.
pub fn check_production_tools_sandboxed(root: &Path) -> Vec<Violation> {
    let mut out = Vec::new();
    let assembly = crate::resolve_path(
        root,
        &[
            "core/tetonic-runtime/src/assembly.rs",
            "core/lokai-runtime/src/assembly.rs",
        ],
    );
    let assembly_text = std::fs::read_to_string(&assembly).unwrap_or_default();
    if assembly_text.contains("tetonic_tools::")
        || assembly_text.contains("tetonic_tools::")
        || assembly_text.contains("EnforcementLevel")
    {
        out.push(Violation {
            rule: "production_tools_sandboxed",
            path: assembly.clone(),
            detail: "production runtime must not name tools / EnforcementLevel".into(),
        });
    }
    if assembly_text.contains("EnforcementLevel::Constrained") {
        out.push(Violation {
            rule: "production_tools_sandboxed",
            path: assembly,
            detail: "runtime assembly must not select Constrained".into(),
        });
    }
    let turn = crate::resolve_path(
        root,
        &[
            "litho/tetonic-app/src/turn_execution.rs",
            "litho/lokai-app/src/turn_execution.rs",
        ],
    );
    let turn_text = std::fs::read_to_string(&turn).unwrap_or_default();
    if !turn_text.contains("EnforcementLevel::Sandboxed") {
        out.push(Violation {
            rule: "production_tools_sandboxed",
            path: turn,
            detail: "app Tools builder must select EnforcementLevel::Sandboxed".into(),
        });
    }
    let tools_lib = crate::resolve_path(
        root,
        &[
            "litho/lokai-tools/src/lib.rs",
            "litho/tetonic-tools/src/lib.rs",
        ],
    );
    let tools_text = std::fs::read_to_string(&tools_lib).unwrap_or_default();
    if tools_text.contains("EnforcementLevel::Constrained") {
        out.push(Violation {
            rule: "production_tools_sandboxed",
            path: tools_lib,
            detail: "Tools::new must default to Sandboxed (R6-2); Constrained is test-only via with_enforcement_level".into(),
        });
    }
    let worktree = crate::resolve_path(
        root,
        &[
            "litho/lokai-tools/src/worktree.rs",
            "litho/tetonic-tools/src/worktree.rs",
        ],
    );
    let wt_text = std::fs::read_to_string(&worktree).unwrap_or_default();
    if wt_text.contains("EnforcementLevel::Constrained") {
        out.push(Violation {
            rule: "production_tools_sandboxed",
            path: worktree,
            detail: "session worktree git must use Sandboxed ProcessExecutor".into(),
        });
    }
    out
}

/// M5-1: fabric protocol crate must stay transport/enrollment/persistence free.
pub fn check_fabric_protocol_isolation(root: &Path) -> Vec<Violation> {
    let path = crate::resolve_path(
        root,
        &[
            "atmos/tetonic-fabric-protocol/Cargo.toml",
            "atmos/lokai-fabric-protocol/Cargo.toml",
        ],
    );
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let forbidden = [
        "lokai-enroll",
        "tetonic-enroll",
        "lokai-memory",
        "tetonic-memory",
        "lokai-egress",
        "tetonic-egress",
        "tokio",
        "reqwest",
        "hyper",
        "sqlx",
        "rusqlite",
    ];
    let mut out = Vec::new();
    for dep in forbidden {
        if text.contains(dep) {
            out.push(Violation {
                rule: "fabric_protocol_isolation",
                path: path.clone(),
                detail: format!("fabric-protocol must not depend on {dep}"),
            });
        }
    }
    out
}

/// M5-1: inference/scheduling must not depend on enrollment implementation.
pub fn check_inference_no_enroll(root: &Path) -> Vec<Violation> {
    let path = crate::resolve_path(
        root,
        &[
            "atmos/tetonic-inference/Cargo.toml",
            "atmos/lokai-inference/Cargo.toml",
        ],
    );
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    if text.contains("lokai-enroll") || text.contains("tetonic-enroll") {
        vec![Violation {
            rule: "inference_no_enroll",
            path,
            detail: "inference must not depend on enroll".into(),
        }]
    } else {
        vec![]
    }
}

/// M5-1: fabric client must use extracted protocol types.
pub fn check_fabric_client_uses_protocol(root: &Path) -> Vec<Violation> {
    let lib = crate::resolve_path(
        root,
        &[
            "atmos/tetonic-fabric-client/src/lib.rs",
            "atmos/lokai-fabric-client/src/lib.rs",
        ],
    );
    let protocol = crate::resolve_path(
        root,
        &[
            "atmos/tetonic-fabric-client/src/protocol.rs",
            "atmos/lokai-fabric-client/src/protocol.rs",
        ],
    );
    let mut out = Vec::new();
    let lib_text = std::fs::read_to_string(&lib).unwrap_or_default();
    if !lib_text.contains("mod protocol") {
        out.push(Violation {
            rule: "fabric_client_protocol",
            path: lib.clone(),
            detail: "fabric-client must expose protocol module".into(),
        });
    }
    let proto_text = std::fs::read_to_string(&protocol).unwrap_or_default();
    if !proto_text.contains("tetonic_fabric_protocol")
        && !proto_text.contains("tetonic_fabric_protocol")
    {
        out.push(Violation {
            rule: "fabric_client_protocol",
            path: protocol,
            detail: "fabric-client protocol module must use fabric-protocol".into(),
        });
    }
    out
}

/// M3-1: run/task/attempt state must mutate only through lokai-run supervisor.
pub fn check_run_state_mutations(root: &Path) -> Vec<Violation> {
    let forbidden = [
        "commit_run_command(",
        "UPDATE run_projections",
        "INSERT INTO run_events",
    ];
    let allow = [
        "mantle/lokai-run/",
        "mantle/tetonic-run/",
        "strata/lokai-memory/src/run_store.rs",
        "strata/tetonic-memory/src/run_store.rs",
        "tooling/lokai-arch-gate/",
        "tooling/tetonic-arch-gate/",
        "/tests/",
        "_tests.rs",
    ];
    let mut out = Vec::new();
    for path in collect_rs_files(root) {
        let rel_path = rel(root, &path);
        if allow.iter().any(|a| rel_path.contains(a)) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        for pat in forbidden {
            if text.contains(pat) {
                out.push(Violation {
                    rule: "run_state_mutation_bypass",
                    path: path.clone(),
                    detail: format!(
                        "direct run state mutation `{pat}` must route through RunSupervisor"
                    ),
                });
            }
        }
    }
    out
}

pub fn check_async_sync_calls(root: &Path) -> Vec<Violation> {
    let re_async = regex::Regex::new(r"async\s+fn\b").expect("regex");
    let re_sync = regex::Regex::new(r"\.(read_sync|write_sync)\b").expect("regex");
    let mut out = Vec::new();

    for path in collect_rs_files(root) {
        let rel_path = rel(root, &path);

        // Skip tests and specific crates that are not yet migrated
        if rel_path.contains("/tests/")
            || rel_path.ends_with("_tests.rs")
            || rel_path.contains("lokai-app")
            || rel_path.contains("tetonic-app")
            || rel_path.contains("lokai-cli")
            || rel_path.contains("tetonic-tools")
            || rel_path.contains("lokai-tools")
            || rel_path.contains("lokai-arch-gate")
            || rel_path.contains("tetonic-arch-gate")
        {
            continue;
        }

        let text = std::fs::read_to_string(&path).unwrap_or_default();
        if !re_sync.is_match(&text) || !re_async.is_match(&text) {
            continue;
        }

        let mut in_async_fn = false;
        let mut brace_depth = 0_isize;
        let mut looking_for_brace = false;

        for line in text.lines() {
            if re_async.is_match(line) {
                in_async_fn = true;
                looking_for_brace = true;
            }
            if in_async_fn {
                brace_depth += line.chars().filter(|&c| c == '{').count() as isize;
                brace_depth -= line.chars().filter(|&c| c == '}').count() as isize;
                if brace_depth > 0 {
                    looking_for_brace = false;
                }

                if re_sync.is_match(line) {
                    out.push(Violation {
                        rule: "async_sync_calls",
                        path: path.clone(),
                        detail: "Blocking .read_sync or .write_sync call inside async fn body"
                            .into(),
                    });
                }

                if brace_depth <= 0 && !looking_for_brace && line.contains('}') {
                    in_async_fn = false;
                    brace_depth = 0;
                }
            }
        }
    }
    out
}

/// H2-2: do not reintroduce `Mutex<Store>` / `Arc<Mutex<Store>>` as the shared
/// store handle. The read pool may still use `Mutex<Vec<Store>>`.
pub fn check_no_mutex_store(root: &Path) -> Vec<Violation> {
    let re = regex::Regex::new(r"Mutex\s*<\s*(?:(?:tetonic_memory|tetonic_memory)::)?Store\s*>")
        .expect("regex");
    let mut out = Vec::new();
    for path in collect_rs_files(root) {
        let rel_path = rel(root, &path);
        if rel_path.contains("/tests/")
            || rel_path.ends_with("_tests.rs")
            || rel_path.contains("lokai-arch-gate")
            || rel_path.contains("tetonic-arch-gate")
        {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        for (i, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") {
                continue;
            }
            if re.is_match(line) {
                out.push(Violation {
                    rule: "no_mutex_store",
                    path: path.clone(),
                    detail: format!(
                        "line {}: SharedStore must stay writer-actor + read pool; found Mutex wrapping Store",
                        i + 1
                    ),
                });
            }
        }
    }
    out
}

/// ARCH-V4-PORTAL-001: Portals (lokai-cli and lokaid) must depend only on lokai-app
/// (and rpc for lokaid stdio IPC). Direct imports and dependencies on core crates
/// (substrate, compute, capabilities, infrastructure, manager, or mantle/orchestrator)
/// are strictly forbidden.
pub fn check_portal_decoupling(root: &Path) -> Vec<Violation> {
    let mut out = Vec::new();
    let forbidden_crates = [
        ("lokai-core", "tetonic_core"),
        ("tetonic-core", "tetonic_core"),
        ("lokai-runtime", "tetonic_runtime"),
        ("tetonic-runtime", "tetonic_runtime"),
        ("lokai-inference", "tetonic_inference"),
        ("tetonic-inference", "tetonic_inference"),
        ("lokai-capacity", "tetonic_capacity"),
        ("tetonic-capacity", "tetonic_capacity"),
        ("lokai-broker", "tetonic_broker"),
        ("tetonic-broker", "tetonic_broker"),
        ("lokai-fabric-protocol", "tetonic_fabric_protocol"),
        ("tetonic-fabric-protocol", "tetonic_fabric_protocol"),
        ("lokai-fabric-client", "tetonic_fabric_client"),
        ("tetonic-fabric-client", "tetonic_fabric_client"),
        ("lokai-memory", "tetonic_memory"),
        ("tetonic-memory", "tetonic_memory"),
        ("lokai-index", "tetonic_index"),
        ("tetonic-index", "tetonic_index"),
        ("lokai-tools", "tetonic_tools"),
        ("tetonic-tools", "tetonic_tools"),
        ("lokai-artifact", "tetonic_artifact"),
        ("tetonic-artifact", "tetonic_artifact"),
        ("lokai-sandbox", "tetonic_sandbox"),
        ("tetonic-sandbox", "tetonic_sandbox"),
        ("lokai-context", "tetonic_context"),
        ("tetonic-context", "tetonic_context"),
        ("lokai-transaction", "tetonic_transaction"),
        ("tetonic-transaction", "tetonic_transaction"),
        ("lokai-policy", "tetonic_policy"),
        ("tetonic-policy", "tetonic_policy"),
        ("lokai-egress", "tetonic_egress"),
        ("tetonic-egress", "tetonic_egress"),
        ("lokai-secrets", "tetonic_secrets"),
        ("tetonic-secrets", "tetonic_secrets"),
        ("lokai-enroll", "tetonic_enroll"),
        ("tetonic-enroll", "tetonic_enroll"),
        ("lokai-node", "tetonic_node"),
        ("tetonic-node", "tetonic_node"),
        ("lokai-domain", "tetonic_domain"),
        ("tetonic-domain", "tetonic_domain"),
        ("lokai-orchestrator", "tetonic_orchestrator"),
        ("tetonic-orchestrator", "tetonic_orchestrator"),
        ("lokai-eval", "tetonic_eval"),
        ("tetonic-eval", "tetonic_eval"),
    ];

    let portals = [("litho/lokai-cli", false), ("litho/lokaid", true)];

    for (portal_rel, allows_rpc) in portals {
        let portal_dir = root.join(portal_rel);
        let cargo_toml = portal_dir.join("Cargo.toml");
        let cargo_text = std::fs::read_to_string(&cargo_toml).unwrap_or_default();

        for (krate_name, _) in &forbidden_crates {
            if cargo_text.contains(&format!("{krate_name} ="))
                || cargo_text.contains(&format!("\"{krate_name}\""))
            {
                out.push(Violation {
                    rule: "ARCH-V4-PORTAL-001",
                    path: cargo_toml.clone(),
                    detail: format!(
                        "Portal `{portal_rel}` must not depend on core crate `{krate_name}` in Cargo.toml"
                    ),
                });
            }
        }

        if !allows_rpc
            && (cargo_text.contains("lokai-rpc =")
                || cargo_text.contains("\"lokai-rpc\"")
                || cargo_text.contains("tetonic-rpc =")
                || cargo_text.contains("\"tetonic-rpc\""))
        {
            out.push(Violation {
                rule: "ARCH-V4-PORTAL-001",
                path: cargo_toml.clone(),
                detail: format!("Portal `{portal_rel}` must not depend on RPC crate in Cargo.toml"),
            });
        }

        let src_dir = portal_dir.join("src");
        for path in collect_rs_files(&src_dir) {
            let rel_path = rel(root, &path);
            if rel_path.contains("ui.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            for (_, krate_mod) in &forbidden_crates {
                let pat1 = format!("use {krate_mod}::");
                let pat2 = format!("{krate_mod}::");
                let has_violation = text.lines().any(|line| {
                    let trimmed = line.trim();
                    if trimmed.starts_with("//") {
                        return false;
                    }
                    line.contains(&pat1) || line.contains(&pat2)
                });
                if has_violation {
                    out.push(Violation {
                        rule: "ARCH-V4-PORTAL-001",
                        path: path.clone(),
                        detail: format!(
                            "Portal source `{rel_path}` must not import core crate `{krate_mod}`"
                        ),
                    });
                }
            }

            if !allows_rpc {
                let has_rpc = text.lines().any(|line| {
                    let trimmed = line.trim();
                    if trimmed.starts_with("//") {
                        return false;
                    }
                    line.contains("use tetonic_rpc::")
                        || line.contains("tetonic_rpc::")
                        || line.contains("use tetonic_rpc::")
                        || line.contains("tetonic_rpc::")
                });
                if has_rpc {
                    out.push(Violation {
                        rule: "ARCH-V4-PORTAL-001",
                        path: path.clone(),
                        detail: format!("Portal source `{rel_path}` must not import RPC crate"),
                    });
                }
            }
        }
    }

    out
}

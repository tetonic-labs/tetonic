//! Architecture invariant checks (AC2-9).

mod checks;
pub use checks::*;
mod freeze;
mod ids;
mod p0;
pub mod quality;
pub mod report;
pub mod v4_corpus;
mod v4_scan;
pub mod verify;

pub use ids::arch_id;
pub use report::Finding;

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Violation {
    pub rule: &'static str,
    pub path: PathBuf,
    pub detail: String,
}

pub fn engine_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("engine root")
}

pub fn resolve_path(root: &Path, candidates: &[&str]) -> PathBuf {
    for c in candidates {
        let p = root.join(c);
        if p.exists() {
            return p;
        }
    }
    root.join(candidates[0])
}

pub fn collect_rs_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk_rs(root, &mut out);
    out.sort();
    out
}

fn walk_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "target" || name == ".git" || name == "fixtures" {
                continue;
            }
            walk_rs(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn is_allowed_subprocess(rel_path: &str) -> bool {
    // Listing process_executor.rs is not "process is sandboxed".
    // Constrained / shell_command Command::new residual remains (M2 leftover; DEL-001 DELETED).
    const ALLOW: &[&str] = &[
        "lokai-tools/src/process_executor.rs",
        "lokai-tools/src/exec.rs",
        "lokai-sandbox/src/process_executor.rs",
        "lokai-sandbox/src/exec.rs",
        "core/lokai-sandbox/src/backend/windows.rs",
        "core/tetonic-sandbox/src/backend/windows.rs",
        "core/lokai-sandbox/src/backend/windows_net.rs",
        "core/tetonic-sandbox/src/backend/windows_net.rs",
        "core/lokai-sandbox/src/backend/linux.rs",
        "core/tetonic-sandbox/src/backend/linux.rs",
        "core/lokai-sandbox/src/backend/linux_net.rs",
        "core/tetonic-sandbox/src/backend/linux_net.rs",
        "core/lokai-sandbox/src/backend/macos.rs",
        "core/tetonic-sandbox/src/backend/macos.rs",
        "core/lokai-sandbox/src/backend/unix_common.rs",
        "core/tetonic-sandbox/src/backend/unix_common.rs",
        "core/lokai-sandbox/src/sync_service.rs",
        "core/tetonic-sandbox/src/sync_service.rs",
        "core/lokai-sandbox/bins/",
        "core/tetonic-sandbox/bins/",
        "mantle/lokai-capacity/src/detect.rs",
        "mantle/tetonic-capacity/src/detect.rs",
        "core/lokai-transaction/src/version.rs",
        "core/tetonic-transaction/src/version.rs",
        "core/lokai-transaction/src/lock.rs",
        "core/tetonic-transaction/src/lock.rs",
        "mantle/lokai-enroll/src/server.rs",
        "mantle/tetonic-enroll/src/server.rs",
        "tooling/lokai-arch-gate/",
        "tooling/tetonic-arch-gate/",
        "tooling/lokai-eval/",
        "tooling/tetonic-eval/",
        "litho/lokaid/src/supervise.rs",
    ];
    let norm = rel_path.replace("tetonic-", "lokai-");
    ALLOW
        .iter()
        .any(|a| rel_path.contains(a) || norm.contains(a))
}

pub fn check_subprocess_spawn(root: &Path) -> Vec<Violation> {
    let re = regex::Regex::new(r"Command::new\s*\(").expect("regex");
    let mut out = Vec::new();
    for path in collect_rs_files(root) {
        let rel_path = rel(root, &path);
        if rel_path.contains("/tests/")
            || rel_path.ends_with("_tests.rs")
            || rel_path.contains("/benches/")
            || rel_path.contains("security-fixtures")
        {
            continue;
        }
        if is_allowed_subprocess(&rel_path) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        if re.is_match(&text) {
            out.push(Violation {
                rule: "subprocess_spawn",
                path: path.clone(),
                detail: "Command::new outside ProcessExecutor allowlist".into(),
            });
        }
    }
    out
}

pub fn check_reqwest(root: &Path) -> Vec<Violation> {
    let re = regex::Regex::new(r"\breqwest::").expect("regex");
    let mut out = Vec::new();
    for path in collect_rs_files(root) {
        let rel_path = rel(root, &path);
        if rel_path.contains("atmos/lokai-egress/") || rel_path.contains("atmos/tetonic-egress/") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        if !re.is_match(&text) {
            continue;
        }
        // enroll integration tests use reqwest inside #[cfg(test)] modules only.
        if (rel_path.contains("mantle/lokai-enroll/")
            || rel_path.contains("mantle/tetonic-enroll/"))
            && text.contains("#[cfg(test)]")
        {
            continue;
        }
        out.push(Violation {
            rule: "reqwest_boundary",
            path: path.clone(),
            detail: "reqwest used outside egress crate".into(),
        });
    }
    out
}

pub fn check_production_runtime(root: &Path) -> Vec<Violation> {
    let re = regex::Regex::new(r"Agent::(new|with_tokenizer)\s*\(").expect("regex");
    let mut out = Vec::new();
    for bin in ["litho/lokaid/src", "litho/lokai-cli/src"] {
        let dir = root.join(bin);
        for path in collect_rs_files(&dir) {
            let rel_path = rel(root, &path);
            if rel_path.contains("/tests/") || rel_path.ends_with("_tests.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            if text.contains("MockProvider") || text.contains("mod tests") {
                // skip test helpers in same file: only flag if outside #[cfg(test)] blocks is hard;
                // lokaid production paths use assemble_agent only today.
            }
            if re.is_match(&text) && !text.contains("#[cfg(test)]") {
                // Heuristic: production files with Agent::new and no test module
                if !text.contains("mod tests {") {
                    out.push(Violation {
                        rule: "engine_runtime",
                        path: path.clone(),
                        detail: "production bin constructs Agent without EngineRuntime".into(),
                    });
                }
            }
        }
    }
    out
}

pub fn check_policy_deps(root: &Path) -> Vec<Violation> {
    let path = resolve_path(
        root,
        &[
            "core/tetonic-policy/Cargo.toml",
            "core/lokai-policy/Cargo.toml",
        ],
    );
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    if text.contains("lokai-inference") || text.contains("tetonic-inference") {
        vec![Violation {
            rule: "dependency_direction",
            path,
            detail: "policy crate must not depend on inference crate".into(),
        }]
    } else {
        vec![]
    }
}

/// Every method listed in `schema_bundle().methods` must be handled in lokaid dispatch.
pub fn check_schema_methods(root: &Path) -> Vec<Violation> {
    let daemon = root.join("litho/lokaid/src/daemon.rs");
    let main_rs = root.join("litho/lokaid/src/main.rs");
    let text = format!(
        "{}\n{}",
        std::fs::read_to_string(&daemon).unwrap_or_default(),
        std::fs::read_to_string(&main_rs).unwrap_or_default(),
    );
    let bundle = lokai_rpc::schema_bundle();
    let Some(methods) = bundle.get("methods").and_then(|m| m.as_object()) else {
        return vec![Violation {
            rule: "schema_methods",
            path: daemon.clone(),
            detail: "schema_bundle missing methods object".into(),
        }];
    };
    let mut out = Vec::new();
    for (const_name, wire) in methods {
        let wire = wire.as_str().unwrap_or_default();
        let pat = format!("methods::{const_name}");
        if !text.contains(&pat) && !text.contains(wire) {
            out.push(Violation {
                rule: "schema_methods",
                path: daemon.clone(),
                detail: format!("daemon dispatch missing schema method {const_name} ({wire})"),
            });
        }
    }
    out
}

/// Maximum lines allowed in a single Rust source file (lib.rs / main.rs / daemon.rs).
pub const MAX_RS_FILE_LINES: usize = 900;

/// Files temporarily above the limit while being split (remove as modules land).\
/// Allowlist entries require: named owner, documented reason, removal ticket.\
const FILE_SIZE_ALLOWLIST: &[&str] = &[
    "strata/lokai-memory/src/lib.rs",
    "strata/tetonic-memory/src/lib.rs",
    "atmos/lokai-inference/src/lib.rs",
    "atmos/tetonic-inference/src/lib.rs",
    "litho/lokai-cli/src/main.rs",
    "core/lokai-core/src/agent.rs",
    "core/tetonic-core/src/agent.rs",
    // Owner: M5-3. Reason: pooled dispatch tests cover trust downgrade,
    // revocation, failover, and capability freshness alongside private helpers.
    // Removal: extract pooled tests before M6-1 admission-control work.
    "atmos/lokai-inference/src/pooled.rs",
    "atmos/tetonic-inference/src/pooled.rs",
    // Owner: M5-3. Reason: additive worker-trust RPC schema pushed this legacy
    // protocol aggregate just over the limit.
    // Removal: extract fabric trust protocol types before M5-4.
    "atmos/lokai-rpc/src/protocol.rs",
    "atmos/tetonic-rpc/src/protocol.rs",
    // M2-1 adversarial tests added 5 large test functions inline.
    // Owner: M2-1. Reason: adversarial test suite required by ticket AC lives alongside the impl.
    // Removal: extract to litho/lokai-tools/tests/process_broker_adversarial.rs in M2-2 cleanup.
    "litho/lokai-tools/src/process_executor.rs",
    // Owner: M5-4 (R13). Reason: typed placement and redundant verification policy evaluation with inline tests.
    "atmos/lokai-inference/src/placement_engine.rs",
    "atmos/tetonic-inference/src/placement_engine.rs",
    // Owner: M5-1. Reason: fabric /v1/chat ingress handler with full streaming and cancellation state machine.
    "mantle/lokai-node/src/fabric_chat.rs",
    "mantle/tetonic-node/src/fabric_chat.rs",
    "atmos/lokai-fabric-client/src/legacy.rs",
    "atmos/tetonic-fabric-client/src/legacy.rs",
    // Owner: app turn path (pre-existing WIP). Reason: rustfmt expansion during M0 VERIFY
    // pushed this file over 900. Not an M0 freeze split.
    // Removal: extract event/redact helpers; delete this row.
    "litho/lokai-app/src/turn_execution.rs",
    // Owner: lokai-context. Reason: `src/tests.rs` is not skipped by `_tests.rs`/`/tests/`.
    // Removal: move to strata/lokai-context/tests/ or rename to *_tests.rs.
    "strata/lokai-context/src/tests.rs",
    "strata/tetonic-context/src/tests.rs",
];

pub fn check_file_sizes(root: &Path) -> Vec<Violation> {
    let mut out = Vec::new();
    for path in collect_rs_files(root) {
        let rel_path = rel(root, &path);
        let norm_path = rel_path.replace("tetonic-", "lokai-");
        if rel_path.contains("/tests/")
            || rel_path.ends_with("_tests.rs")
            || rel_path.contains("/benches/")
            || FILE_SIZE_ALLOWLIST
                .iter()
                .any(|a| rel_path.contains(a) || norm_path.contains(a))
        {
            continue;
        }
        let line_count = std::fs::read_to_string(&path)
            .map(|t| t.lines().count())
            .unwrap_or(0);
        if line_count > MAX_RS_FILE_LINES {
            out.push(Violation {
                rule: "file_size",
                path: path.clone(),
                detail: format!("{line_count} lines exceeds limit of {MAX_RS_FILE_LINES}"),
            });
        }
    }
    out
}

pub fn check_app_layer_deps(root: &Path) -> Vec<Violation> {
    let mut out = Vec::new();

    // Check Cargo.toml for lokai-app
    let cargo_path = root.join("litho/lokai-app/Cargo.toml");
    let cargo_text = std::fs::read_to_string(&cargo_path).unwrap_or_default();
    if cargo_text.contains("lokai-rpc")
        || cargo_text.contains("tetonic-rpc")
        || cargo_text.contains("lokaid")
        || cargo_text.contains("lokai-cli")
    {
        out.push(Violation {
            rule: "app_layer_deps",
            path: cargo_path,
            detail: "lokai-app must not depend on rpc or binary crates".into(),
        });
    }

    // Check lokai-app source files for print, eprint, rpc
    let app_root = root.join("litho/lokai-app/src");
    let print_re = regex::Regex::new(r"print(ln)?!\s*\(").unwrap();
    let eprint_re = regex::Regex::new(r"eprint(ln)?!\s*\(").unwrap();

    for path in collect_rs_files(&app_root) {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        if text.contains("lokai_rpc::") || text.contains("tetonic_rpc::") {
            out.push(Violation {
                rule: "app_layer_isolation",
                path: path.clone(),
                detail: "rpc types must not appear in application service interfaces".into(),
            });
        }
        if print_re.is_match(&text) || eprint_re.is_match(&text) {
            out.push(Violation {
                rule: "app_layer_io",
                path: path.clone(),
                detail: "Application services may not write directly to stdout or stderr".into(),
            });
        }
    }
    out
}

/// M1-2: migrated RPC handlers must delegate workflow decisions to `services.app`.
pub fn check_app_workflow_delegation(root: &Path) -> Vec<Violation> {
    const HANDLERS: &[&str] = &[
        "litho/lokaid/src/daemon/handlers/session.rs",
        "litho/lokaid/src/daemon/handlers/policy.rs",
        "litho/lokaid/src/daemon/handlers/misc.rs",
        "litho/lokaid/src/daemon/handlers/chat.rs",
        "litho/lokaid/src/daemon/handlers/agent.rs",
        "litho/lokaid/src/daemon/handlers/capacity.rs",
    ];
    let app_re = regex::Regex::new(r"services\s*\.\s*app|\.app\s*\.").expect("regex");
    let mut out = Vec::new();
    for rel in HANDLERS {
        let path = root.join(rel);
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        if !app_re.is_match(&text) {
            out.push(Violation {
                rule: "app_workflow_delegation",
                path: path.clone(),
                detail: "handler must delegate workflow decisions through Application services"
                    .into(),
            });
        }
    }
    out
}

/// M1-3: CLI agent path must delegate to lokai-app; no direct orchestration or agent assembly.
pub fn check_cli_workflow_delegation(root: &Path) -> Vec<Violation> {
    const FILES: &[&str] = &[
        "litho/lokai-cli/src/main.rs",
        "litho/lokai-cli/src/chat.rs",
        "litho/lokai-cli/src/estate.rs",
        "litho/lokai-cli/src/capacity.rs",
    ];
    let app_re = regex::Regex::new(
        r"lokai_app::|\.runs\.|\.sessions\.|\.estate\.|\.capacity\.|\.approvals\.",
    )
    .expect("regex");
    let mut out = Vec::new();
    for rel in FILES {
        let path = root.join(rel);
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        if !app_re.is_match(&text) {
            out.push(Violation {
                rule: "cli_workflow_delegation",
                path: path.clone(),
                detail: "CLI command handler must delegate through lokai-app Application services"
                    .into(),
            });
        }
    }
    out
}

pub fn check_lokaid_no_direct_orchestration(root: &Path) -> Vec<Violation> {
    let re = regex::Regex::new(
        r"run_orchestrated_turn\s*\(|run_spawned_specialist\s*\(|assemble_agent\s*\(|build_agent_standalone\s*\(",
    )
    .expect("regex");
    let mut out = Vec::new();
    let dir = root.join("litho/lokaid/src/daemon/handlers");
    for path in collect_rs_files(&dir) {
        let rel_path = rel(root, &path);
        if rel_path.contains("/tests/") || rel_path.ends_with("_tests.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        if re.is_match(&text) {
            out.push(Violation {
                rule: "lokaid_no_direct_orchestration",
                path: path.clone(),
                detail: "lokaid handlers must delegate orchestration to lokai-app RunService"
                    .into(),
            });
        }
    }
    out
}

pub fn check_lokaid_handlers_no_profile_store(root: &Path) -> Vec<Violation> {
    let re = regex::Regex::new(
        r"ProfileStore::new|(?:lokai_capacity|tetonic_capacity)::run_optimize\s*\(",
    )
    .expect("regex");
    let mut out = Vec::new();
    let dir = root.join("litho/lokaid/src/daemon/handlers");
    for path in collect_rs_files(&dir) {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        if re.is_match(&text) {
            out.push(Violation {
                rule: "lokaid_no_capacity_workflow",
                path: path.clone(),
                detail: "capacity workflow must delegate to lokai-app CapacityService".into(),
            });
        }
    }
    out
}

pub fn check_cli_no_direct_orchestration(root: &Path) -> Vec<Violation> {
    let re = regex::Regex::new(r"run_orchestrated_turn\s*\(|assemble_agent\s*\(").expect("regex");
    let mut out = Vec::new();
    let dir = root.join("litho/lokai-cli/src");
    for path in collect_rs_files(&dir) {
        let rel_path = rel(root, &path);
        if rel_path.contains("/tests/") || rel_path.ends_with("_tests.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        if re.is_match(&text) {
            out.push(Violation {
                rule: "cli_no_direct_orchestration",
                path: path.clone(),
                detail:
                    "lokai-cli must not invoke run_orchestrated_turn or assemble_agent directly"
                        .into(),
            });
        }
    }
    out
}

pub fn check_unguarded_remote_dispatch(root: &Path) -> Vec<Violation> {
    let remote_new = regex::Regex::new(r"RemoteNodeProvider::new\s*\(").expect("regex");
    let pooled_new = regex::Regex::new(r"PooledProvider::new_with_registry\s*\(").expect("regex");
    let allow_remote_new = &[
        "litho/lokai-app/src/compute_plane.rs",
        "litho/lokaid/src/daemon/compute.rs",
        "atmos/lokai-inference/src/pooled.rs",
        "atmos/tetonic-inference/src/pooled.rs",
    ];
    let mut out = Vec::new();
    for path in collect_rs_files(root) {
        let rel_path = rel(root, &path);
        if rel_path.contains("/tests/")
            || rel_path.ends_with("_tests.rs")
            || rel_path.contains("/benches/")
        {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        // Constructors in an inline `#[cfg(test)]` module are fixtures, not
        // production dispatch wiring.
        let production_text = text.split("#[cfg(test)]").next().unwrap_or(&text);
        if remote_new.is_match(production_text)
            && !allow_remote_new.iter().any(|a| rel_path.contains(a))
        {
            out.push(Violation {
                rule: "unguarded_remote_dispatch",
                path: path.clone(),
                detail: "RemoteNodeProvider::new outside authorized compute wiring".into(),
            });
        }
        if pooled_new.is_match(production_text)
            && !production_text.contains("with_dispatch_guard")
            && rel_path.contains("litho/lokaid/")
            && !rel_path.contains("/tests/")
        {
            out.push(Violation {
                rule: "unguarded_remote_dispatch",
                path: path.clone(),
                detail: "PooledProvider must be wired with with_dispatch_guard in production"
                    .into(),
            });
        }
    }
    out
}

/// M2-4: production write tools must route through lokai-transaction, not direct workspace I/O.
/// M3 A-12: CLI `offline.rs` restore must not regress to raw `std::fs::write`.
pub fn check_workspace_mutations(root: &Path) -> Vec<Violation> {
    let forbidden = [
        "write_bytes_nofollow(",
        "apply_edit_at_path(",
        "std::fs::write(",
        "std::fs::remove_file(",
    ];
    let allow = [
        "core/lokai-transaction/",
        "core/tetonic-transaction/",
        "litho/lokai-tools/src/workspace.rs",
        "litho/lokai-tools/src/mutation.rs",
        "/tests/",
        "_tests.rs",
        "/benches/",
    ];
    let mut out = Vec::new();
    for path in collect_rs_files(&root.join("litho/lokai-tools/src")) {
        let rel_path = rel(root, &path);
        if allow.iter().any(|a| rel_path.contains(a)) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        for pat in forbidden {
            if text.contains(pat) {
                out.push(Violation {
                    rule: "workspace_mutation_bypass",
                    path: path.clone(),
                    detail: format!(
                        "direct workspace mutation `{pat}` must route through lokai-transaction"
                    ),
                });
            }
        }
    }
    let cli_forbidden = [
        "apply_edit_at_path(",
        "std::fs::write(",
        "std::fs::remove_file(",
    ];
    let offline = root.join("litho/lokai-cli/src/offline.rs");
    if offline.exists() {
        let text = freeze::production_text(&std::fs::read_to_string(&offline).unwrap_or_default());
        for pat in cli_forbidden {
            if text.contains(pat) {
                out.push(Violation {
                    rule: "workspace_mutation_bypass",
                    path: offline.clone(),
                    detail: format!(
                        "CLI restore `{pat}` must use jailed lokai-tools helpers (DEL-023 / M3)"
                    ),
                });
            }
        }
    }
    out
}

pub fn run_all(root: &Path) -> Vec<Violation> {
    let mut v = Vec::new();
    v.extend(check_subprocess_spawn(root));
    v.extend(check_reqwest(root));
    v.extend(check_production_runtime(root));
    v.extend(check_policy_deps(root));
    v.extend(check_schema_methods(root));
    v.extend(check_file_sizes(root));
    v.extend(check_app_layer_deps(root));
    v.extend(check_app_workflow_delegation(root));
    v.extend(check_cli_workflow_delegation(root));
    v.extend(check_cli_no_direct_orchestration(root));
    v.extend(check_lokaid_no_direct_orchestration(root));
    v.extend(check_lokaid_handlers_no_profile_store(root));
    v.extend(check_unguarded_remote_dispatch(root));
    v.extend(check_workspace_mutations(root));
    v.extend(check_run_state_mutations(root));
    v.extend(check_fabric_protocol_isolation(root));
    v.extend(check_inference_no_enroll(root));
    v.extend(check_fabric_client_uses_protocol(root));
    v.extend(check_compute_broker_wiring(root));
    v.extend(check_cli_inspector_no_command(root));
    v.extend(check_production_tools_sandboxed(root));
    v.extend(check_git_via_process_broker(root));
    v.extend(check_lsp_via_process_broker(root));
    v.extend(check_cli_infra_leftovers(root));
    v.extend(check_lokaid_session_authority(root));
    v.extend(check_no_gates_ok_turn_abort(root));
    v.extend(check_no_duplicate_resume_cap(root));
    v.extend(check_no_duplicate_enrollment_helpers(root));
    v.extend(check_outbound_secret_scanner(root));
    v.extend(check_context_compiler_wired(root));
    v.extend(check_async_sync_calls(root));
    v.extend(check_no_mutex_store(root));
    v.extend(check_portal_decoupling(root));
    v.extend(freeze::run_freeze(root));
    v.extend(v4_scan::check_v4_promoted(root));
    v
}

#[cfg(test)]
mod tests;

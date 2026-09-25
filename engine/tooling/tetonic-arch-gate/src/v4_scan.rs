//! GATE-01 promoted ARCH-V4 production-failing detectors.
//! Adding these scanners is not ESTABLISH. ARCH-V4-GATE-001 is not satisfied.

use std::path::Path;

use crate::{collect_rs_files, Violation};

#[path = "test_source.rs"]
mod test_source;
pub use test_source::strip_cfg_test_blocks;

pub fn production_prefix(src: &str) -> String {
    strip_cfg_test_blocks(src)
}

fn cargo_prod_deps(toml: &str) -> &str {
    let start = toml.find("[dependencies]").unwrap_or(0);
    let rest = &toml[start..];
    rest.split("\n[").next().unwrap_or(rest)
}

fn skip_cfg_test_path(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str() == "tests")
}

pub fn check_v4_promoted(root: &Path) -> Vec<Violation> {
    let mut out = Vec::new();
    out.extend(check_sub_002(root));
    out.extend(check_manager_owner(root));
    out.extend(check_tool_001(root));
    out.extend(check_dep_001(root));
    out.extend(check_iface_001(root));
    out.extend(check_cmp_001(root));
    out.extend(check_iface_002(root));
    out.extend(check_obs_001(root));
    out.extend(check_cap_001(root));
    out
}

fn check_sub_002(root: &Path) -> Vec<Violation> {
    let path = crate::resolve_path(
        root,
        &["core/tetonic-core/Cargo.toml", "core/lokai-core/Cargo.toml"],
    );
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let deps = cargo_prod_deps(&text);
    if deps.contains("lokai-tools") || deps.contains("tetonic-tools") {
        vec![Violation {
            rule: "ARCH-V4-SUB-002",
            path,
            detail: "core [dependencies] must not list tools crate".into(),
        }]
    } else {
        vec![]
    }
}

fn check_tool_001(root: &Path) -> Vec<Violation> {
    let path = crate::resolve_path(
        root,
        &[
            "core/tetonic-domain/src/tool_host.rs",
            "core/lokai-domain/src/tool_host.rs",
        ],
    );
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let prod = production_prefix(&text);
    let mut out = Vec::new();
    for needle in ["commit_staged", "run_verify", "abort_staged"] {
        if prod.contains(needle) {
            out.push(Violation {
                rule: "ARCH-V4-TOOL-001",
                path: path.clone(),
                detail: format!("production tool_host.rs must not contain `{needle}`"),
            });
        }
    }
    out
}

fn check_dep_001(root: &Path) -> Vec<Violation> {
    let mut out = check_sub_002(root);
    let path = crate::resolve_path(
        root,
        &[
            "core/tetonic-runtime/Cargo.toml",
            "core/lokai-runtime/Cargo.toml",
        ],
    );
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let deps = cargo_prod_deps(&text);
    if deps.contains("lokai-tools") || deps.contains("tetonic-tools") {
        out.push(Violation {
            rule: "ARCH-V4-DEP-001",
            path: path.clone(),
            detail: "runtime [dependencies] must not list tools crate".into(),
        });
    }
    if deps.contains("lokai-transaction") || deps.contains("tetonic-transaction") {
        out.push(Violation {
            rule: "ARCH-V4-DEP-001",
            path,
            detail: "runtime [dependencies] must not list transaction crate".into(),
        });
    }
    out
}

fn check_iface_001(root: &Path) -> Vec<Violation> {
    let needles = [
        "take_conversation(",
        "restore_conversation(",
        ".run_turn(",
        "build_supervisor",
        "use tetonic_run::RunSupervisor",
    ];
    let mut out = Vec::new();
    for rel in [
        "litho/tetonic-cli/src",
        "litho/tetonicd/src",
        "tooling/lokai-eval/src",
        "tooling/tetonic-eval/src",
    ] {
        let p = root.join(rel);
        if !p.exists() {
            continue;
        }
        for path in collect_rs_files(&p) {
            if skip_cfg_test_path(&path) {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let prod = production_prefix(&text);
            for needle in needles {
                if prod.contains(needle) {
                    out.push(Violation {
                        rule: "ARCH-V4-IFACE-001",
                        path: path.clone(),
                        detail: format!("portal production src must not contain `{needle}`"),
                    });
                }
            }
        }
    }
    out
}

fn check_cmp_001(root: &Path) -> Vec<Violation> {
    let needles = [
        "RunCommand::CompleteAttempt",
        "pub fn apply_verified_remote_patch",
        "pub fn apply_authorized_remote_patch",
    ];
    let mut out = Vec::new();
    let client_src = crate::resolve_path(
        root,
        &[
            "atmos/tetonic-fabric-client/src",
            "atmos/lokai-fabric-client/src",
        ],
    );
    for path in collect_rs_files(&client_src) {
        if skip_cfg_test_path(&path) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let prod = production_prefix(&text);
        for needle in needles {
            if prod.contains(needle) {
                out.push(Violation {
                    rule: "ARCH-V4-CMP-001",
                    path: path.clone(),
                    detail: format!("production fabric-client src must not contain `{needle}`"),
                });
            }
        }
    }
    out
}

fn check_iface_002(root: &Path) -> Vec<Violation> {
    let mut out = Vec::new();
    let paths = [
        crate::resolve_path(
            root,
            &[
                "core/tetonic-runtime/src/assembly.rs",
                "core/lokai-runtime/src/assembly.rs",
            ],
        ),
        crate::resolve_path(
            root,
            &[
                "core/tetonic-runtime/src/action_broker.rs",
                "core/lokai-runtime/src/action_broker.rs",
            ],
        ),
        crate::resolve_path(
            root,
            &[
                "litho/tetonic-app/src/turn_execution.rs",
                "litho/lokai-app/src/turn_execution.rs",
            ],
        ),
    ];
    for path in paths {
        if !path.exists() {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let prod = production_prefix(&text);
        if prod.contains("set_session_approval_hook") || prod.contains("set_approval_hook(") {
            out.push(Violation {
                rule: "ARCH-V4-IFACE-002",
                path: path.clone(),
                detail: "shared runtime must not hold singleton session approval hook".into(),
            });
        }
    }
    out
}

fn check_obs_001(root: &Path) -> Vec<Violation> {
    let mut out = Vec::new();
    let job_path = root.join("litho/lokai-app/src/identity_job.rs");
    if job_path.exists() {
        let text = std::fs::read_to_string(&job_path).unwrap_or_default();
        let prod = production_prefix(&text);
        if prod.contains("&mut |_| {}") || prod.contains("&mut |_step| {}") {
            out.push(Violation {
                rule: "ARCH-V4-OBS-001",
                path: job_path.clone(),
                detail:
                    "sessionless execution must not pass silent no-op sink to execute_bound_attempt"
                        .into(),
            });
        }
    }
    let events_path = root.join("litho/lokai-app/src/events.rs");
    if events_path.exists() {
        let text = std::fs::read_to_string(&events_path).unwrap_or_default();
        let prod = production_prefix(&text);
        for variant in [
            "ModelToken {",
            "ThoughtToken {",
            "ToolCall {",
            "ToolResult {",
            "TurnCompleted {",
        ] {
            if let Some(pos) = prod.find(variant) {
                let end = prod[pos..].find('}').unwrap_or(prod[pos..].len());
                let block = &prod[pos..pos + end];
                if !block.contains("attempt_id") {
                    out.push(Violation {
                        rule: "ARCH-V4-OBS-001",
                        path: events_path.clone(),
                        detail: format!("{variant} missing execution envelope attempt_id"),
                    });
                }
            }
        }
    }
    out
}

fn check_cap_001(root: &Path) -> Vec<Violation> {
    let mut out = Vec::new();
    let assembly_path = crate::resolve_path(
        root,
        &[
            "core/tetonic-runtime/src/assembly.rs",
            "core/lokai-runtime/src/assembly.rs",
        ],
    );
    let assembly_text = std::fs::read_to_string(&assembly_path).unwrap_or_default();
    let assembly_prod = production_prefix(&assembly_text);
    if assembly_prod.contains("build_production_context_compiler") {
        out.push(Violation {
            rule: "ARCH-V4-CAP-001",
            path: assembly_path.clone(),
            detail: "runtime assembly.rs must not contain `build_production_context_compiler`"
                .into(),
        });
    }
    if assembly_prod.contains("current_dir()") {
        out.push(Violation {
            rule: "ARCH-V4-CAP-001",
            path: assembly_path,
            detail: "runtime assembly.rs must not contain `current_dir()`".into(),
        });
    }

    let toml_path = crate::resolve_path(
        root,
        &[
            "core/tetonic-runtime/Cargo.toml",
            "core/lokai-runtime/Cargo.toml",
        ],
    );
    let toml_text = std::fs::read_to_string(&toml_path).unwrap_or_default();
    if cargo_prod_deps(&toml_text).contains("ignore") {
        out.push(Violation {
            rule: "ARCH-V4-CAP-001",
            path: toml_path,
            detail: "runtime [dependencies] must not list `ignore`".into(),
        });
    }

    let runtime_src = crate::resolve_path(
        root,
        &["core/tetonic-runtime/src", "core/lokai-runtime/src"],
    );
    for path in collect_rs_files(&runtime_src) {
        if skip_cfg_test_path(&path) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let prod = production_prefix(&text);
        if prod.contains("WalkBuilder") {
            out.push(Violation {
                rule: "ARCH-V4-CAP-001",
                path: path.clone(),
                detail: format!(
                    "production runtime file {} must not contain `WalkBuilder`",
                    path.display()
                ),
            });
        }
        if prod.contains("WorkspaceContextProvider") {
            out.push(Violation {
                rule: "ARCH-V4-CAP-001",
                path: path.clone(),
                detail: format!(
                    "production runtime file {} must not contain `WorkspaceContextProvider`",
                    path.display()
                ),
            });
        }
    }
    out
}

#[cfg(test)]
#[path = "v4_scan_tests.rs"]
mod tests;

/// The product adapter must not reacquire lifecycle authority after migration.
fn check_manager_owner(root: &Path) -> Vec<Violation> {
    let mut out = Vec::new();
    for module in [
        "run_service.rs",
        "identity_job.rs",
        "turn_finalization.rs",
        "attempt_completion.rs",
    ] {
        let path = root.join("litho/lokai-app/src").join(module);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let source = production_prefix(&text);
        for marker in [
            "HashMap<AttemptId, ActiveTurnRun>",
            "tokio::task::spawn_local",
            "RunCommand::CreateAttempt",
            "RunCommand::LeaseAttempt",
            "RunCommand::ClaimExecution",
            "RunCommand::ClaimFinalization",
            "RunCommand::CompleteAttempt",
            "RunCommand::FailAttempt",
            "RunCommand::CancelRun",
            "RunCommand::FinishRun",
        ] {
            if source.contains(marker) {
                out.push(Violation { rule: "ARCH-V4-WORK-002", path: path.clone(), detail: format!("product adapter owns lifecycle primitive `{marker}`; delegate to lokai-run") });
            }
        }
    }
    out
}

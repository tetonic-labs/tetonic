//! WORK-02 Attempt-keyed execution / Infer admission / Agent::run pins.

use std::fs;
use std::path::{Path, PathBuf};

fn production_prefix(src: &str) -> &str {
    src.split("#[cfg(test)]").next().unwrap_or(src)
}

#[test]
fn work02_active_is_attempt_keyed() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Registry);
}

#[test]
fn work02_heartbeat_uses_attempt() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Heartbeat);
}

#[test]
fn work02_root_start_uses_local_executor() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Execution);
}

#[test]
fn work02_orchestrator_leftover_turn_remains() {
    let src = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../mantle/tetonic-orchestrator/src/turn.rs"),
    )
    .expect("turn.rs");
    let prod = src.split("#[cfg(test)]").next().unwrap_or(&src);
    assert!(prod.contains("root_execute"));
    assert!(prod.contains("trait ChildJob"));
    assert!(!prod.contains(".turn("));
}

#[test]
fn work02_one_local_executor_impl() {
    let engine_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut impls = Vec::new();
    let layers = [
        "core",
        "strata",
        "mantle",
        "atmos",
        "litho",
        "portals",
        "product",
        "manager",
        "substrate",
        "compute",
        "capabilities",
        "infrastructure",
        "tooling",
        "bins",
        "crates",
    ];
    for layer in layers {
        walk_production_rs(&engine_root.join(layer), &mut |path, src| {
            if src.contains("AgentAttemptExecutor for") {
                impls.push(path.display().to_string());
            }
        });
    }
    assert_eq!(impls.len(), 1, "found {impls:?}");
    assert!(
        impls[0]
            .replace('\\', "/")
            .ends_with("tetonic-runtime/src/executor.rs")
            || impls[0]
                .replace('\\', "/")
                .ends_with("lokai-runtime/src/executor.rs")
    );
}

#[test]
fn work02_agent_run_deleted() {
    let src = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../core/tetonic-core/src/agent.rs"),
    )
    .expect("agent.rs");
    assert!(!src.contains("pub async fn run<F>"));
}

#[test]
fn work02_broker_has_no_runcommand_lifecycle() {
    let lease = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../mantle/tetonic-broker/src/scheduler/attempt_lease.rs"),
    )
    .expect("attempt_lease.rs");
    let prod = production_prefix(&lease);
    for needle in [
        "RunCommand::CreateRun",
        "RunCommand::StartRun",
        "RunCommand::AddTask",
        "RunCommand::FailAttempt",
        "RunCommand::CreateAttempt",
        "RunCommand::LeaseAttempt",
        "RunCommand::CancelTask",
    ] {
        assert!(
            !prod.contains(needle),
            "production broker still has {needle}"
        );
    }
    let broker = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../mantle/tetonic-broker/src/broker.rs"),
    )
    .expect("broker.rs");
    assert!(!production_prefix(&broker).contains("RunCommand::CancelTask"));
}

#[test]
fn work02_infer_hop_still_no_job_spec() {
    let src = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../mantle/tetonic-run/src/infer_admission.rs"),
    )
    .expect("infer_admission.rs");
    assert!(src.contains("job_spec: None"));
    assert!(!src.contains("job_spec: Some"));
    assert!(tetonic_run::hop_job_spec_must_be_none(None));
}

#[test]
fn work02_cmp001_not_established_fabric_complete_remains() {
    let src = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/fabric_run_bridge.rs"),
    )
    .expect("fabric_run_bridge.rs");
    assert!(!src.contains("apply_complete_attempt"));
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tooling/tetonic-arch-gate/fixtures/v4/ARCH-V4-CMP-001.v4fix");
    assert!(
        fixture.exists(),
        "ARCH-V4-CMP-001 stays planted inventory; not ESTABLISHED"
    );
}

fn walk_production_rs(root: &Path, visit: &mut impl FnMut(&Path, &str)) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if matches!(name, "target" | "tests") {
                    continue;
                }
                stack.push(path);
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                if let Ok(src) = fs::read_to_string(&path) {
                    visit(&path, &src);
                }
            }
        }
    }
}

#[path = "support/lifecycle_contract.rs"]
mod lifecycle_contract;

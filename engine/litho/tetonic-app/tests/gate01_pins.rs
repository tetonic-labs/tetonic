//! GATE-01 IMPLEMENT source pins. Close zero `DEL-V4-*` rows.
//! Passing these tests does not ESTABLISH GATE/ID/APP/IFACE/DEP/FIN/CMP.

use std::fs;
use std::path::PathBuf;

fn crate_src(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    if path.exists() {
        return fs::read_to_string(&path).expect(rel);
    }
    let alt_rel = rel.replace("lokai-", "tetonic-");
    let alt_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(&alt_rel);
    if alt_path.exists() {
        return fs::read_to_string(&alt_path).expect(&alt_rel);
    }
    fs::read_to_string(&path).expect(rel)
}

fn cargo_prod_deps(toml: &str) -> &str {
    let start = toml.find("[dependencies]").unwrap_or(0);
    let rest = &toml[start..];
    rest.split("\n[").next().unwrap_or(rest)
}

fn fn_body<'a>(src: &'a str, sig: &str) -> &'a str {
    let start = src.rfind(sig).expect(sig);
    let after = &src[start..];
    let brace = after.find('{').expect("fn body");
    let bytes = &after.as_bytes()[brace..];
    let mut depth = 0i32;
    for (i, ch) in bytes.iter().enumerate() {
        match *ch {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &after[brace..=brace + i];
                }
            }
            _ => {}
        }
    }
    panic!("unclosed {sig}");
}

#[test]
fn gate01_start_identity_job_sessionless() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Sessionless);
}

#[test]
fn gate01_create_run_persist_only() {
    let src = crate_src("src/execution/service.rs");
    let body = fn_body(&src, "async fn create_run(");
    assert!(!body.contains("StartRun"));
    assert!(!body.contains("executor"));
    assert!(body.contains("RunCommand::CreateRun"));
}

#[test]
fn gate01_no_invariant_established() {
    let corpus = crate_src("../../tooling/tetonic-arch-gate/src/v4_corpus.rs");
    assert!(corpus.contains("not ARCH-V4-GATE-001 satisfied"));
    assert!(!corpus.contains("INV-V4-ID-001 ESTABLISHED"));
    assert!(!corpus.contains("INV-V4-APP-002 ESTABLISHED"));
}

#[test]
fn gate01_no_with_completion_check() {
    let src = crate_src("../../core/tetonic-core/src/agent.rs");
    assert!(!src.contains("with_completion_check"));
    assert!(!src.contains("completion_check"));
    assert!(!src.contains("CompletionDecision::Reject"));
}

#[test]
fn gate01_patch_pipeline_not_in_src() {
    let lib = crate_src("../../atmos/tetonic-fabric-client/src/lib.rs");
    assert!(!lib.contains("pub mod patch_pipeline"));
    assert!(!lib.contains("pub use patch_pipeline"));
    let src_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../atmos/tetonic-fabric-client/src");
    assert!(!src_dir.join("patch_pipeline.rs").exists());
    let helper = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../atmos/tetonic-fabric-client/tests/common/patch_pipeline.rs");
    assert!(helper.exists());
    let helper_src = fs::read_to_string(&helper).expect("helper");
    assert!(helper_src.contains("tetonic_fabric_client::validate_patch_acceptance"));
    assert!(helper_src.contains("tetonic_fabric_client::FabricClientError"));
    assert!(!helper_src.contains("crate::result_accept"));
    assert!(!PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../atmos/tetonic-fabric-client/tests/patch_pipeline.rs")
        .exists());
}

#[test]
fn gate01_promoted_detectors_live() {
    let lib = crate_src("../../tooling/tetonic-arch-gate/src/lib.rs");
    assert!(lib.contains("v4_scan::check_v4_promoted"));
    let scan = crate_src("../../tooling/tetonic-arch-gate/src/v4_scan.rs");
    for rule in [
        "ARCH-V4-SUB-002",
        "ARCH-V4-TOOL-001",
        "ARCH-V4-DEP-001",
        "ARCH-V4-IFACE-001",
        "ARCH-V4-CMP-001",
    ] {
        assert!(scan.contains(rule), "scanner missing {rule}");
    }
}

#[test]
fn gate01_sub001_fin_work_gate001_still_inventory() {
    let corpus = crate_src("../../tooling/tetonic-arch-gate/src/v4_corpus.rs");
    assert!(corpus.contains("Other ARCH-V4-* stay"));
    for name in [
        "ARCH-V4-SUB-001.v4fix",
        "ARCH-V4-FIN-001.v4fix",
        "ARCH-V4-WORK-001.v4fix",
    ] {
        let fix = crate_src(&format!(
            "../../tooling/tetonic-arch-gate/fixtures/v4/{name}"
        ));
        assert!(
            fix.contains("not scanned as production"),
            "{name} must stay planted inventory"
        );
    }
    let gate =
        crate_src("../../tooling/tetonic-arch-gate/fixtures/v4/ARCH-V4-GATE-001-coding.v4fix");
    assert!(gate.contains("not satisfied"));
    assert!(gate.contains("must not mark ARCH-V4-GATE-001 satisfied"));
}

#[test]
fn gate01_explain_floor_left_the_loop() {
    let src = crate_src("../../core/tetonic-core/src/agent.rs");
    assert!(!src.contains("summary.len() < 20"));
}

#[test]
fn gate01_start_identity_job_still_none() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Sessionless);
}

#[test]
fn gate01_create_run_still_persist_only() {
    let src = crate_src("src/execution/service.rs");
    let body = fn_body(&src, "async fn create_run(");
    assert!(!body.contains("StartRun"));
}

#[test]
fn gate01_destination_lokai_tools_remain() {
    let toml = crate_src("Cargo.toml");
    assert!(
        cargo_prod_deps(&toml).contains("tetonic-tools")
            || cargo_prod_deps(&toml).contains("lokai-tools"),
        "Cargo.toml must keep lokai-tools"
    );
    let orch_toml = crate_src("../../mantle/tetonic-orchestrator/Cargo.toml");
    assert!(
        !cargo_prod_deps(&orch_toml).contains("tetonic-tools")
            && !cargo_prod_deps(&orch_toml).contains("lokai-tools"),
        "mantle/tetonic-orchestrator must not have production dependency on tetonic-tools"
    );
}

#[path = "support/lifecycle_contract.rs"]
mod lifecycle_contract;

//! COMP-01 IMPLEMENT source pins. Close `006`/`031`/`008`/`021` at CONVERGE only.

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

fn production_prefix(src: &str) -> &str {
    src.split("#[cfg(test)]").next().unwrap_or(src)
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
    &after[brace..]
}

fn trait_run_service(src: &str) -> &str {
    let start = src.find("pub trait RunService").expect("RunService trait");
    fn_body(&src[start..], "pub trait RunService")
}

#[test]
fn comp01_runtime_cargo_has_no_lokai_tools() {
    let toml = crate_src("../../core/tetonic-runtime/Cargo.toml");
    assert!(
        !cargo_prod_deps(&toml).contains("tetonic-tools")
            && !cargo_prod_deps(&toml).contains("lokai-tools")
    );
}

#[test]
fn comp01_runtime_cargo_has_no_lokai_transaction() {
    let toml = crate_src("../../core/tetonic-runtime/Cargo.toml");
    assert!(
        !cargo_prod_deps(&toml).contains("tetonic-transaction")
            && !cargo_prod_deps(&toml).contains("lokai-transaction")
    );
}

#[test]
fn comp01_runtime_has_no_lokai_tools_import() {
    for rel in [
        "../../core/tetonic-runtime/src/assembly.rs",
        "../../core/tetonic-runtime/src/build.rs",
        "../../core/tetonic-runtime/src/lib.rs",
    ] {
        let owned = crate_src(rel);
        let src = production_prefix(&owned);
        assert!(
            !src.contains("tetonic_tools::") && !src.contains("Tools::new"),
            "{rel} production src must not name tetonic_tools / Tools::new"
        );
        assert!(
            !src.contains("fn enforcement_level"),
            "{rel} must not keep enforcement_level"
        );
    }
}

#[test]
fn comp01_runtime_has_no_lokai_transaction_import() {
    for rel in [
        "../../core/tetonic-runtime/src/assembly.rs",
        "../../core/tetonic-runtime/src/build.rs",
        "../../core/tetonic-runtime/src/lib.rs",
    ] {
        let owned = crate_src(rel);
        let src = production_prefix(&owned);
        assert!(
            !src.contains("tetonic_transaction::"),
            "{rel} production src must not name tetonic_transaction"
        );
    }
}

#[test]
fn comp01_tools_crate_still_depends_on_transaction() {
    let toml = crate_src("../../litho/tetonic-tools/Cargo.toml");
    assert!(
        cargo_prod_deps(&toml).contains("tetonic-transaction")
            || cargo_prod_deps(&toml).contains("lokai-transaction")
    );
}

#[test]
fn comp01_fabric_client_transaction_untouched() {
    let toml = crate_src("../../atmos/tetonic-fabric-client/Cargo.toml");
    assert!(
        cargo_prod_deps(&toml).contains("tetonic-transaction")
            || cargo_prod_deps(&toml).contains("lokai-transaction")
    );
}

#[test]
fn comp01_public_run_service_has_no_turn_execution_host() {
    let src = crate_src("src/execution/service.rs");
    let trait_body = trait_run_service(&src);
    assert!(!trait_body.contains("TurnExecutionHost"));
    assert!(!trait_body.contains("async fn run_turn("));
    assert!(!trait_body.contains("async fn spawn_agent("));
}

#[test]
fn comp01_build_supervisor_not_pub() {
    let src = crate_src("src/host/composition.rs");
    assert!(src.contains("pub(crate) fn build_supervisor("));
    assert!(!src.contains("pub fn build_supervisor("));
}

#[test]
fn comp01_application_supervisor_not_pub() {
    let src = crate_src("src/lib.rs");
    assert!(src.contains("pub(crate) supervisor: Arc<dyn tetonic_run::RunSupervisor>"));
    assert!(!src.contains("pub supervisor: Arc<dyn tetonic_run::RunSupervisor>"));
}

#[test]
fn comp01_local_ui_uses_application_owned_bootstrap() {
    let src = crate_src("../../litho/tetonic-cli/src/local_ui.rs");
    assert!(!src.contains("build_supervisor"));
    assert!(!src.contains("from_bootstrap_with_supervisor"));
    assert!(!src.contains("app.supervisor"));
    assert!(!src.contains("RunSupervisor"));
    assert!(!src.contains("SupervisorRunBridge"));
    assert!(src.contains("LocalWorkspace::open_with_configuration("));
    let bootstrap = crate_src("src/local_workspace/bootstrap.rs");
    assert!(bootstrap.contains("prepare_launch_with_control("));
    assert!(bootstrap.contains("RegisteredLaunchHost"));
}

#[test]
fn comp01_compute_plane_request_has_no_supervisor_field() {
    let src = crate_src("src/compute_plane.rs");
    let prod = production_prefix(&src);
    assert!(!prod.contains("pub supervisor"));
    assert!(prod.contains("pub(crate) fn wrap_pooled_with_broker("));
}

#[test]
fn comp01_inspect_run_remains() {
    let src = crate_src("src/execution/service.rs");
    assert!(src.contains("async fn inspect_run("));
    assert!(src.contains("async fn resume_events("));
}

#[test]
fn comp01_internal_run_service_still_has_supervisor() {
    let src = crate_src("src/execution/service.rs");
    assert!(src.contains("supervisor: Arc<dyn RunSupervisor>"));
}

#[test]
fn comp01_start_identity_job_stays_sessionless() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Sessionless);
}

#[test]
fn comp01_create_run_stays_persist_only() {
    let src = crate_src("src/execution/service.rs");
    let body = fn_body(&src, "async fn create_run(");
    assert!(!body.contains("execute_turn"));
    assert!(!body.contains("submit_chat_turn"));
}

#[test]
fn comp01_bh_id_session_stays_defect() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tooling/tetonic-arch-gate/fixtures/v4/ARCH-V4-IFACE-001.v4fix");
    assert!(fixture.is_file(), "ARCH-V4-IFACE-001 stays planted");
}

#[path = "support/lifecycle_contract.rs"]
mod lifecycle_contract;

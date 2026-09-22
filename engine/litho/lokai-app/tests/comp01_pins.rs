//! COMP-01 IMPLEMENT source pins. Close `006`/`031`/`008`/`021` at CONVERGE only.

use std::fs;
use std::path::PathBuf;

fn crate_src(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
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
    let toml = crate_src("../../core/lokai-runtime/Cargo.toml");
    assert!(!cargo_prod_deps(&toml).contains("lokai-tools"));
}

#[test]
fn comp01_runtime_cargo_has_no_lokai_transaction() {
    let toml = crate_src("../../core/lokai-runtime/Cargo.toml");
    assert!(!cargo_prod_deps(&toml).contains("lokai-transaction"));
}

#[test]
fn comp01_runtime_has_no_lokai_tools_import() {
    for rel in [
        "../../core/lokai-runtime/src/assembly.rs",
        "../../core/lokai-runtime/src/build.rs",
        "../../core/lokai-runtime/src/lib.rs",
    ] {
        let owned = crate_src(rel);
        let src = production_prefix(&owned);
        assert!(
            !src.contains("lokai_tools::") && !src.contains("Tools::new"),
            "{rel} production src must not name lokai_tools / Tools::new"
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
        "../../core/lokai-runtime/src/assembly.rs",
        "../../core/lokai-runtime/src/build.rs",
        "../../core/lokai-runtime/src/lib.rs",
    ] {
        let owned = crate_src(rel);
        let src = production_prefix(&owned);
        assert!(
            !src.contains("lokai_transaction::"),
            "{rel} production src must not name lokai_transaction"
        );
    }
}

#[test]
fn comp01_tools_crate_still_depends_on_transaction() {
    let toml = crate_src("../../litho/lokai-tools/Cargo.toml");
    assert!(cargo_prod_deps(&toml).contains("lokai-transaction"));
}

#[test]
fn comp01_fabric_client_transaction_untouched() {
    let toml = crate_src("../../atmos/lokai-fabric-client/Cargo.toml");
    assert!(cargo_prod_deps(&toml).contains("lokai-transaction"));
}

#[test]
fn comp01_assemble_agent_stays_without_tools() {
    let src = crate_src("../../core/lokai-runtime/src/assembly.rs");
    let prod = production_prefix(&src);
    assert!(prod.contains("pub fn assemble_agent("));
    assert!(prod.contains("with_context_compiler"));
    assert!(
        !prod.contains("build_production_context_compiler"),
        "CAP-01: runtime assembly must not contain build_production_context_compiler"
    );
    assert!(!prod.contains("Agent::with_tokenizer("));
    assert!(!prod.contains("tools: Tools"));
    let turn = crate_src("src/turn_execution.rs");
    assert!(turn.contains("assemble_agent"));
    assert!(turn.contains("Agent::with_tokenizer"));
    assert!(turn.contains("EnforcementLevel::Sandboxed"));
    assert!(turn.contains("build_production_context_compiler"));
}

#[test]
fn comp01_turn_execution_host_not_pub() {
    let src = crate_src("src/turn_execution.rs");
    assert!(src.contains("pub(crate) struct TurnExecutionHost"));
    assert!(!src.contains("pub struct TurnExecutionHost"));
}

#[test]
fn comp01_build_session_host_not_pub() {
    let src = crate_src("src/product_submit.rs");
    assert!(src.contains("pub(crate) fn build_session_host"));
    assert!(!src.contains("pub fn build_session_host"));
}

#[test]
fn comp01_execute_spawn_symbol_remains() {
    let src = crate_src("src/turn_execution.rs");
    assert!(src.contains("async fn execute_spawn("));
    assert!(src.contains("pub(crate) async fn execute_spawn("));
}

#[test]
fn comp01_run_turn_symbol_remains() {
    let src = crate_src("src/run_service.rs");
    assert!(src.contains("async fn run_turn("));
}

#[test]
fn comp01_public_run_service_has_no_turn_execution_host() {
    let src = crate_src("src/run_service.rs");
    let trait_body = trait_run_service(&src);
    assert!(!trait_body.contains("TurnExecutionHost"));
    assert!(!trait_body.contains("async fn run_turn("));
    assert!(!trait_body.contains("async fn spawn_agent("));
}

#[test]
fn comp01_cli_lokaid_eval_have_no_turn_execution_host() {
    for rel in [
        "../../litho/lokai-cli/src/main.rs",
        "../../litho/lokaid/src/daemon/handlers/initialize.rs",
        "../../tooling/lokai-eval/src/kernel.rs",
    ] {
        let src = crate_src(rel);
        assert!(
            !src.contains("TurnExecutionHost"),
            "{rel} must not name TurnExecutionHost"
        );
    }
    let parity = crate_src("../../litho/lokaid/src/daemon/tests/parity.rs");
    assert!(!parity.contains("TurnExecutionHost"));
}

#[test]
fn comp01_build_supervisor_not_pub() {
    let src = crate_src("src/lib.rs");
    assert!(src.contains("pub(crate) fn build_supervisor("));
    assert!(!src.contains("pub fn build_supervisor("));
}

#[test]
fn comp01_application_supervisor_not_pub() {
    let src = crate_src("src/lib.rs");
    assert!(src.contains("pub(crate) supervisor: Arc<dyn lokai_run::RunSupervisor>"));
    assert!(!src.contains("pub supervisor: Arc<dyn lokai_run::RunSupervisor>"));
}

#[test]
fn comp01_cli_no_supervisor_clone() {
    let src = crate_src("../../litho/lokai-cli/src/main.rs");
    assert!(!src.contains("app.supervisor"));
    assert!(!src.contains("build_supervisor"));
    assert!(!src.contains("RunSupervisor"));
    assert!(!src.contains("SupervisorRunBridge"));
    let cli_boot = crate_src("src/cli_bootstrap.rs");
    assert!(cli_boot.contains("install_compute_services"));
    assert!(crate_src("src/product_submit.rs").contains("self.install_compute_plane("));
}

#[test]
fn comp01_daemon_no_build_supervisor() {
    let src = crate_src("../../litho/lokaid/src/daemon/handlers/initialize.rs");
    assert!(!src.contains("build_supervisor"));
    assert!(!src.contains("from_bootstrap_with_supervisor"));
    assert!(!src.contains("app.supervisor"));
    assert!(!src.contains("RunSupervisor"));
    assert!(!src.contains("SupervisorRunBridge"));
    let daemon_boot = crate_src("src/daemon_bootstrap.rs");
    assert!(daemon_boot.contains("from_bootstrap("));
    assert!(daemon_boot.contains("install_compute_plane"));
}

#[test]
fn comp01_daemon_compute_no_run_supervisor() {
    let daemon_boot = crate_src("src/daemon_bootstrap.rs");
    assert!(!daemon_boot.contains("RunSupervisor"));
    assert!(!daemon_boot.contains("lokai_run::"));
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
    let src = crate_src("src/run_service.rs");
    assert!(src.contains("async fn inspect_run("));
    assert!(src.contains("async fn resume_events("));
}

#[test]
fn comp01_internal_run_service_still_has_supervisor() {
    let src = crate_src("src/run_service.rs");
    assert!(src.contains("supervisor: Arc<dyn RunSupervisor>"));
}

#[test]
fn comp01_daemon_index_reservation_after_install() {
    let src = crate_src("src/daemon_bootstrap.rs");
    let install = src.find("install_compute_plane").expect("install");
    let reserve = src
        .find("with_index_shard_reservation")
        .expect("reservation");
    assert!(
        install < reserve,
        "install_compute_plane must precede with_index_shard_reservation"
    );
}

#[test]
fn comp01_approot_execute_stays_bound() {
    let src = crate_src("src/turn_execution.rs");
    assert!(src.contains("impl RootExecute for AppRootExecute"));
    assert!(src.contains(".execute_attempt("));
}

#[test]
fn comp01_start_identity_job_stays_sessionless() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Sessionless);
}

#[test]
fn comp01_create_run_stays_persist_only() {
    let src = crate_src("src/run_service.rs");
    let body = fn_body(&src, "async fn create_run(");
    assert!(!body.contains("execute_turn"));
    assert!(!body.contains("submit_chat_turn"));
}

#[test]
fn comp01_live_session_and_catalogue_remain() {
    let live = crate_src("src/session_live.rs");
    assert!(live.contains("pub struct LiveSession"));
    let init = crate_src("src/daemon_bootstrap.rs");
    assert!(init.contains("Tools::new"));
}

#[test]
fn comp01_kernel_unread() {
    let src = crate_src("../../tooling/lokai-eval/src/kernel.rs");
    assert!(src.contains("verify_cmd: None"));
}

#[test]
fn comp01_bh_id_session_stays_defect() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tooling/lokai-arch-gate/fixtures/v4/ARCH-V4-IFACE-001.v4fix");
    assert!(fixture.is_file(), "ARCH-V4-IFACE-001 stays planted");
}

#[test]
fn comp01_host_construction_lives_in_app() {
    let submit = crate_src("src/product_submit.rs");
    assert!(submit.contains("build_session_host"));
    assert!(submit.contains("TurnExecutionHost"));
    let parity = crate_src("../../litho/lokaid/src/daemon/tests/parity.rs");
    assert!(!parity.contains("TurnExecutionHost"));
}

#[path = "support/lifecycle_contract.rs"]
mod lifecycle_contract;

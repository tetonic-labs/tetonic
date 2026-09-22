use super::*;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

fn write(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let mut f = fs::File::create(path).unwrap();
    f.write_all(body.as_bytes()).unwrap();
}

fn temp_engine() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    write(
        &root.join("core/lokai-core/Cargo.toml"),
        "[dependencies]\nlokai-domain = { path = \"../lokai-domain\" }\n\n[dev-dependencies]\nlokai-tools = { path = \"../lokai-tools\" }\n",
    );
    write(
        &root.join("core/lokai-runtime/Cargo.toml"),
        "[dependencies]\nlokai-core = { path = \"../lokai-core\" }\n\n[dev-dependencies]\nlokai-tools = { path = \"../lokai-tools\" }\nlokai-transaction = { path = \"../lokai-transaction\" }\n",
    );
    write(
        &root.join("core/lokai-domain/src/tool_host.rs"),
        "pub trait ToolHost {}\n",
    );
    write(&root.join("litho/lokai-cli/src/main.rs"), "fn main() {}\n");
    write(&root.join("litho/lokaid/src/main.rs"), "fn main() {}\n");
    write(
        &root.join("tooling/lokai-eval/src/main.rs"),
        "fn main() {}\n",
    );
    write(
        &root.join("atmos/lokai-fabric-client/src/lib.rs"),
        "pub mod result_accept;\n",
    );
    write(
        &root.join("atmos/lokai-fabric-client/src/result_accept.rs"),
        "pub struct CompleteAttempt;\n",
    );
    (dir, root)
}

#[test]
fn clean_tree_passes() {
    let (_keep, root) = temp_engine();
    assert!(check_v4_promoted(&root).is_empty());
}

#[test]
fn sub_002_trips_prod_lokai_tools() {
    let (_keep, root) = temp_engine();
    write(
        &root.join("core/lokai-core/Cargo.toml"),
        "[dependencies]\nlokai-tools = { path = \"../lokai-tools\" }\n",
    );
    let v = check_sub_002(&root);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].rule, "ARCH-V4-SUB-002");
}

#[test]
fn tool_001_trips_commit_staged() {
    let (_keep, root) = temp_engine();
    write(
        &root.join("core/lokai-domain/src/tool_host.rs"),
        "fn commit_staged() {}\n",
    );
    let v = check_tool_001(&root);
    assert!(v.iter().any(|x| x.rule == "ARCH-V4-TOOL-001"));
}

#[test]
fn dep_001_trips_runtime_transaction() {
    let (_keep, root) = temp_engine();
    write(
        &root.join("core/lokai-runtime/Cargo.toml"),
        "[dependencies]\nlokai-transaction = { path = \"../lokai-transaction\" }\n",
    );
    let v = check_dep_001(&root);
    assert!(v.iter().any(|x| x.rule == "ARCH-V4-DEP-001"));
}

#[test]
fn iface_001_trips_take_conversation() {
    let (_keep, root) = temp_engine();
    write(
        &root.join("litho/lokai-cli/src/chat.rs"),
        "fn go() { take_conversation(); }\n",
    );
    let v = check_iface_001(&root);
    assert!(v.iter().any(|x| x.rule == "ARCH-V4-IFACE-001"));
}

#[test]
fn iface_001_ignores_bare_run_supervisor() {
    let (_keep, root) = temp_engine();
    write(
        &root.join("litho/lokai-cli/src/failure.rs"),
        "const E: &str = \"provider: RunSupervisor snapshot\";\n",
    );
    assert!(check_iface_001(&root).is_empty());
}

#[test]
fn cmp_001_trips_pub_fn_and_allows_type_only() {
    let (_keep, root) = temp_engine();
    write(
        &root.join("atmos/lokai-fabric-client/src/patch.rs"),
        "pub fn apply_verified_remote_patch() {}\n",
    );
    let v = check_cmp_001(&root);
    assert!(v.iter().any(|x| x.rule == "ARCH-V4-CMP-001"));
    write(
        &root.join("atmos/lokai-fabric-client/src/patch.rs"),
        "pub struct CompleteAttempt;\n",
    );
    assert!(check_cmp_001(&root).is_empty());
}

#[test]
fn tool_001_skips_cfg_test_suffix() {
    let (_keep, root) = temp_engine();
    write(
        &root.join("core/lokai-domain/src/tool_host.rs"),
        "pub trait ToolHost {}\n#[cfg(test)]\nfn commit_staged() {}\n",
    );
    assert!(check_tool_001(&root).is_empty());
}

fn function_body<'a>(src: &'a str, name: &str) -> Option<&'a str> {
    let sig = format!("fn {name}(");
    let start = src.find(&sig)?;
    let after = &src[start..];
    let brace = after.find('{')?;
    let bytes = &after.as_bytes()[brace..];
    let mut depth = 0i32;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&after[brace..=brace + i]);
                }
            }
            _ => {}
        }
    }
    None
}

fn detect_unclassified_fail_hop(src: &str) -> bool {
    let Some(body) = function_body(src, "fail_hop") else {
        return false;
    };
    body.contains("RunCommand::FailAttempt")
        && !body.contains("hop_run_classified")
        && !body.contains("hop_job_spec_must_be_none")
}

#[test]
fn cmp_001_hop_fail_mutant_trips() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/v4/ARCH-V4-CMP-001-hop-fail.v4fix");
    let src = fs::read_to_string(&fixture).expect("planted hop-fail mutant");
    assert!(
        detect_unclassified_fail_hop(&src),
        "planted unclassified fail_hop must trip"
    );
}

#[test]
fn cmp_001_hop_fail_classified_fail_hop_clean() {
    let classified = r#"
pub async fn fail_hop(
    supervisor: &dyn RunSupervisor,
    run_id: RunId,
    attempt_id: AttemptId,
    expected_sequence: u64,
    reason: String,
) -> Result<u64, RunSupervisorError> {
    let snap = supervisor.snapshot(run_id.clone()).await?;
    if !hop_run_classified(&snap) {
        return Err(RunSupervisorError::InvalidTransition("refused".into()));
    }
    let result = supervisor
        .handle(RunCommand::FailAttempt(FailAttempt {
            run_id,
            attempt_id,
            reason,
        }))
        .await?;
    Ok(result.sequence)
}
"#;
    assert!(!detect_unclassified_fail_hop(classified));

    let production = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../mantle/lokai-run/src/infer_admission.rs");
    let src = fs::read_to_string(&production).expect("production fail_hop");
    assert!(
        !detect_unclassified_fail_hop(&src),
        "classified production fail_hop must be clean"
    );
}

fn detect_loop_coding_policy(src: &str) -> bool {
    let prod = production_prefix(src);
    prod.contains("\"read_file\"")
        || prod.contains("\"search_code\"")
        || prod.contains("summary.len() < 20")
        || prod.contains("\"spawn_agent\"")
        || prod.contains("\"expand_context\"")
        || prod.contains("std::fs::metadata")
}

#[test]
fn sub_001_loop_policy_mutant_trips() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/v4/ARCH-V4-SUB-001-loop-policy.v4fix");
    let src = fs::read_to_string(&fixture).expect("planted loop-policy mutant");
    assert!(
        detect_loop_coding_policy(&src),
        "planted loop policy mutant must trip"
    );
}

#[test]
fn sub_001_loop_policy_production_clean() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../core/lokai-core/src");
    for rel in ["agent.rs", "monitor.rs", "demuxer.rs"] {
        let path = root.join(rel);
        let src = fs::read_to_string(&path).unwrap_or_else(|_| panic!("read {rel}"));
        assert!(
            !detect_loop_coding_policy(&src),
            "production {rel} must not contain coding literals or fs::metadata"
        );
    }
}

fn detect_manager_finalization_contract_violations(src: &str) -> bool {
    let prod = production_prefix(src);
    prod.contains("CodingAgentDefinition")
        || prod.contains("commit_staged_if_any")
        || prod.contains("explain_turn")
        || prod.contains("lokai_tools::")
}

#[test]
fn fin_001_contracts_mutant_trips() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/v4/ARCH-V4-FIN-001.v4fix");
    let src = fs::read_to_string(&fixture).expect("planted fin-001 mutant");
    assert!(
        detect_manager_finalization_contract_violations(&src),
        "planted fin-001 mutant must trip"
    );
}

#[test]
fn fin_001_contracts_production_clean() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../litho/lokai-app/src");
    for rel in ["run_service.rs", "turn_finalization.rs"] {
        let path = root.join(rel);
        let src = fs::read_to_string(&path).unwrap_or_else(|_| panic!("read {rel}"));
        assert!(
            !detect_manager_finalization_contract_violations(&src),
            "production {rel} must not contain forbidden finalization symbols"
        );
    }
}

fn detect_product_spawn_local_attempt_violations(src: &str) -> bool {
    let prod = production_prefix(src);
    prod.contains("spawn_local")
}

#[test]
fn work_004_product_spawn_local_mutant_trips() {
    let mutant = "tokio::task::spawn_local(async move { let res = execute_turn(); });";
    assert!(
        detect_product_spawn_local_attempt_violations(mutant),
        "spawn_local in product submit must trip detector"
    );
}

#[test]
fn work_004_product_spawn_local_production_clean() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../litho/lokai-app/src");
    let path = root.join("product_submit.rs");
    let src = fs::read_to_string(&path).expect("read product_submit.rs");
    assert!(
        !detect_product_spawn_local_attempt_violations(&src),
        "production product_submit.rs must not contain spawn_local for attempt execution"
    );
}

fn detect_unbound_identity_job_violations(src: &str) -> bool {
    let prod = production_prefix(src);
    if prod.contains("pub struct LocalAgentAttemptExecutor")
        && (prod.contains("_ctx: AttemptExecutionContext") || !prod.contains("ctx.attempt_id"))
    {
        return true;
    }
    if prod.contains("async fn start_identity_job(")
        && (!prod.contains("cmd.job_spec.identity_id")
            || !prod.contains("job_input_digest")
            || !prod.contains("self.validate_start(&cmd"))
    {
        return true;
    }
    false
}

#[test]
fn work_005_binding_mutant_trips() {
    let mutant_ctx = r#"
pub struct LocalAgentAttemptExecutor;
impl LocalAgentAttemptExecutor {
    async fn execute(
        &mut self,
        invocation: AgentInvocation,
        _ctx: AttemptExecutionContext,
    ) -> CandidateOutcome {
        self.agent
            .turn(self.conversation, invocation, &mut self.on_step)
            .await
    }
}
"#;
    assert!(
        detect_unbound_identity_job_violations(mutant_ctx),
        "_ctx in LocalAgentAttemptExecutor must trip detector"
    );

    let mutant_job = r#"
    pub(super) async fn start_identity_job(
        &self,
        cmd: StartIdentityJobCommand,
        agent: &mut lokai_core::Agent,
    ) -> Result<StartIdentityJobResult, AppError> {
        let active = self.begin_job_run(None, &cmd.identity, cmd.job_spec).await?;
        Ok(StartIdentityJobResult { run_id: active.run_id, task_id: active.task_id, attempt_id: active.attempt_id, outcome: CandidateOutcome::ok() })
    }
"#;
    assert!(
        detect_unbound_identity_job_violations(mutant_job),
        "unvalidated start_identity_job must trip detector"
    );
}

#[test]
fn work_005_binding_production_clean() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let executor_path = root.join("core/lokai-runtime/src/executor.rs");
    let executor_src = fs::read_to_string(&executor_path).expect("read executor.rs");
    assert!(
        !detect_unbound_identity_job_violations(&executor_src),
        "production executor.rs must bind ctx.attempt_id"
    );

    let identity_job_path = root.join("mantle/lokai-run/src/managed/execution.rs");
    let identity_job_src = fs::read_to_string(&identity_job_path).expect("read identity_job.rs");
    assert!(
        !detect_unbound_identity_job_violations(&identity_job_src),
        "production identity_job.rs must validate identity_id and input_digest"
    );
}

fn detect_session_authority_violations(src: &str) -> bool {
    let prod = production_prefix(src);
    if let Some(body) = function_body(&prod, "find_parent_active") {
        if body.contains("live.current_run_id()") || body.contains("live.root_task_id()") {
            return true;
        }
    }
    if let Some(body) = function_body(&prod, "cancel_session") {
        if body.contains("supervisor.handle") || body.contains("RunCommand::CancelRun") {
            return true;
        }
    }
    false
}

#[test]
fn work_006_session_authority_mutant_trips() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/v4/ARCH-V4-WORK-006-session-authority.v4fix");
    let src = fs::read_to_string(&fixture).expect("planted session-authority mutant");
    assert!(
        detect_session_authority_violations(&src),
        "planted session authority mutant must trip detector"
    );
}

#[test]
fn work_006_session_authority_production_clean() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../litho/lokai-app/src");
    for rel in ["run_service.rs", "services.rs"] {
        let path = root.join(rel);
        let src = fs::read_to_string(&path).unwrap_or_else(|_| panic!("read {rel}"));
        assert!(
            !detect_session_authority_violations(&src),
            "production {rel} must not contain session execution authority violations"
        );
    }
}

fn detect_shared_runtime_approval_violations(src: &str) -> bool {
    let prod = production_prefix(src);
    prod.contains("set_session_approval_hook")
        || prod.contains("set_approval_hook(")
        || (prod.contains("struct RuntimeActionBroker") && prod.contains("approval_hook:"))
}

#[test]
fn iface_002_approval_singleton_mutant_trips() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/v4/ARCH-V4-IFACE-002-approval-singleton.v4fix");
    let src = fs::read_to_string(&fixture).expect("planted approval singleton mutant");
    assert!(
        detect_shared_runtime_approval_violations(&src),
        "planted approval singleton mutant must trip detector"
    );
}

#[test]
fn iface_002_approval_singleton_production_clean() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let paths = [
        root.join("core/lokai-runtime/src/assembly.rs"),
        root.join("core/lokai-runtime/src/action_broker.rs"),
        root.join("litho/lokai-app/src/turn_execution.rs"),
    ];
    for path in paths {
        let src = fs::read_to_string(&path).unwrap_or_else(|_| panic!("read {:?}", path));
        assert!(
            !detect_shared_runtime_approval_violations(&src),
            "production {:?} must not contain singleton approval hook",
            path
        );
    }
}

fn detect_sessionless_noop_violations(src: &str) -> bool {
    let prod = production_prefix(src);
    prod.contains("&mut |_| {}")
        || prod.contains("&mut |_step| {}")
        || (prod.contains("execute_bound_attempt") && prod.contains("&mut |_"))
}

#[test]
fn obs_001_sessionless_noop_mutant_trips() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/v4/ARCH-V4-OBS-001-sessionless-noop.v4fix");
    let src = fs::read_to_string(&fixture).expect("planted sessionless noop mutant");
    assert!(
        detect_sessionless_noop_violations(&src),
        "planted sessionless no-op mutant must trip detector"
    );
}

#[test]
fn obs_001_sessionless_noop_production_clean() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../litho/lokai-app/src");
    let job_path = root.join("identity_job.rs");
    let job_src = fs::read_to_string(&job_path).expect("read identity_job.rs");
    assert!(
        !detect_sessionless_noop_violations(&job_src),
        "production identity_job.rs must not contain sessionless no-op sink"
    );

    let events_path = root.join("events.rs");
    let events_src = fs::read_to_string(&events_path).expect("read events.rs");
    assert!(
        !detect_sessionless_noop_violations(&events_src),
        "production events.rs must contain attempt_id envelope fields"
    );
}

#[test]
fn cap_001_trips_compiler_or_heuristics_in_runtime() {
    let (_keep, root) = temp_engine();
    assert!(check_cap_001(&root).is_empty());

    write(
        &root.join("core/lokai-runtime/src/assembly.rs"),
        "fn make() { build_production_context_compiler(); }\n",
    );
    let v = check_cap_001(&root);
    assert!(v.iter().any(|x| x.rule == "ARCH-V4-CAP-001"));

    let (_keep2, root2) = temp_engine();
    write(
        &root2.join("core/lokai-runtime/src/assembly.rs"),
        "fn make() { std::env::current_dir(); }\n",
    );
    let v2 = check_cap_001(&root2);
    assert!(v2.iter().any(|x| x.rule == "ARCH-V4-CAP-001"));

    let (_keep3, root3) = temp_engine();
    write(
        &root3.join("core/lokai-runtime/Cargo.toml"),
        "[dependencies]\nignore = \"0.4\"\n",
    );
    let v3 = check_cap_001(&root3);
    assert!(v3.iter().any(|x| x.rule == "ARCH-V4-CAP-001"));

    let (_keep4, root4) = temp_engine();
    write(
        &root4.join("core/lokai-runtime/src/helper.rs"),
        "fn scan() { let _ = WalkBuilder::new(\".\"); }\n",
    );
    let v4 = check_cap_001(&root4);
    assert!(v4.iter().any(|x| x.rule == "ARCH-V4-CAP-001"));

    let (_keep5, root5) = temp_engine();
    write(
        &root5.join("core/lokai-runtime/src/helper.rs"),
        "struct WorkspaceContextProvider;\n",
    );
    let v5 = check_cap_001(&root5);
    assert!(v5.iter().any(|x| x.rule == "ARCH-V4-CAP-001"));
}

#[test]
fn cap_001_production_clean() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let violations = check_cap_001(&root);
    assert!(
        violations.is_empty(),
        "production tree violates CAP-001: {:?}",
        violations
    );
}

#[test]
fn test_strip_cfg_test_blocks_preserves_trailing_code() {
    let src = r#"
pub fn before() -> i32 { 1 }

#[cfg(test)]
mod tests {
    #[test]
    fn t1() {
        let s = "{ nested brace }";
        assert_eq!(1, 1);
    }
}

pub fn after() -> &'static str {
    "CompleteAttempt"
}
"#;
    let prod = strip_cfg_test_blocks(src);
    assert!(prod.contains("pub fn before"));
    assert!(!prod.contains("fn t1"));
    assert!(!prod.contains("nested brace"));
    assert!(prod.contains("pub fn after"));
    assert!(prod.contains("CompleteAttempt"));
}

#[test]
fn test_scanner_detects_mutant_after_mid_file_test_module() {
    let mutant_src = r#"
pub fn helper() -> bool { true }

#[cfg(test)]
mod tests {
    #[test]
    fn dummy() {}
}

pub fn evil_finish() {
    let _ = "read_file";
}
"#;
    assert!(
        detect_loop_coding_policy(mutant_src),
        "scanner must detect forbidden policy trailing mid-file test module"
    );
}

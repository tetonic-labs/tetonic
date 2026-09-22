//! WORK-FIN-02 S6/S7 pins. Close `017`/`018`/`027`/`029`/`032`/`033` at CONVERGE
//! only. Passing these tests does not ESTABLISH INV-V4-FIN-* / CMP-001.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use tetonic_domain::{
    AgentJobSpec, AttemptId, AttemptLease, AttemptRecord, AttemptState, ExecutionTargetId,
    IdentityId, LeaseId, RunId, RunSnapshot, RunState, SessionId, TaskId, TaskRecord, TaskState,
};
use tetonic_run::detect_recovery_required;

fn crate_src(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    let mut src = fs::read_to_string(&path).expect(rel);
    if rel == "src/run_service.rs" {
        src.push_str(
            &fs::read_to_string(path.with_file_name("turn_finalization.rs"))
                .expect("turn_finalization.rs"),
        );
    }
    src
}

fn production_prefix(src: &str) -> &str {
    src.split("#[cfg(test)]").next().unwrap_or(src)
}

fn fn_body<'a>(src: &'a str, sig: &str) -> &'a str {
    let start = src.find(sig).expect(sig);
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

fn source_order(haystack: &str, start_at: &str, earlier: &str, later: &str) {
    let start = haystack
        .find(start_at)
        .unwrap_or_else(|| panic!("missing `{start_at}`"));
    let rest = &haystack[start..];
    let a = rest
        .find(earlier)
        .unwrap_or_else(|| panic!("missing `{earlier}`"));
    let b = rest
        .find(later)
        .unwrap_or_else(|| panic!("missing `{later}`"));
    assert!(
        a < b,
        "expected `{earlier}` before `{later}` after `{start_at}`"
    );
}

fn empty_snapshot(state: RunState, job_spec: Option<AgentJobSpec>) -> RunSnapshot {
    RunSnapshot {
        run_id: RunId::new("run_wf02"),
        session_id: Some(SessionId::new("sess_wf02")),
        state,
        sequence: 1,
        workspace_version: None,
        tasks: BTreeMap::new(),
        attempts: BTreeMap::new(),
        dependencies: Default::default(),
        events: Vec::new(),
        delivery_index: Default::default(),
        side_effect_commits: Default::default(),
        deadlines: Default::default(),
        cancellation: Default::default(),
        speculation: Default::default(),
        next_lease_epoch: 0,
        job_spec,
    }
}

fn job_spec() -> AgentJobSpec {
    AgentJobSpec {
        identity_id: IdentityId::new("id_wf02"),
        definition_digest: "def".into(),
        input_digest: "in".into(),
        capability_bindings: Vec::new(),
        artifact_bindings: Vec::new(),
        recovery_id: "rec".into(),
    }
}

fn task(claim: Option<AttemptId>) -> TaskRecord {
    TaskRecord {
        task_id: TaskId::new("task_wf02"),
        state: TaskState::Running,
        binding: Default::default(),
        accepted_artifact: None,
        active_attempt: Some(AttemptId::new("att_wf02")),
        winning_attempt: None,
        finalization_claim: claim,
        completed_version: None,
        retry: Default::default(),
        side_effect_keys: Vec::new(),
    }
}

fn attempt(state: AttemptState) -> AttemptRecord {
    let lease = matches!(
        state,
        AttemptState::Leased | AttemptState::Starting | AttemptState::Running
    )
    .then(|| AttemptLease {
        lease_id: LeaseId::new("lease_wf02"),
        attempt_id: AttemptId::new("att_wf02"),
        lease_epoch: 0,
        holder: ExecutionTargetId::local(),
        issued_at: 0,
        expires_at: 10_000,
        heartbeat_interval_secs: 30,
        last_heartbeat_sequence: 0,
    });
    AttemptRecord {
        execution_claimed: false,
        attempt_id: AttemptId::new("att_wf02"),
        task_id: TaskId::new("task_wf02"),
        state,
        task_version: 1,
        workspace_version: None,
        input_digest: "in".into(),
        result_digest: None,
        delivery_key: None,
        lease,
        failure_class: None,
        failure_reason: None,
        attempt_number: 1,
    }
}

#[test]
fn workfin02_finish_does_not_remove_before_terminal() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Cleanup);
}

#[test]
fn workfin02_failed_uses_fail_attempt_not_complete() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Failures);
}

#[test]
fn workfin02_heartbeat_fail_still_finishes() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::ClaimFence);
}

#[test]
fn workfin02_claim_fail_still_finishes() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::ClaimFence);
}

#[test]
fn workfin02_canceled_skips_complete_and_accept() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Cancellation);
}

#[test]
fn workfin02_success_order_complete_accept_finish() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::FinalizationOrder);
}

#[test]
fn workfin02_seal_fail_after_commit_does_not_fail_attempt() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Failures);
}

#[test]
fn workfin02_detect_recovery_required_claimed_unfinished() {
    let mut snap = empty_snapshot(RunState::Active, None);
    snap.tasks.insert(
        TaskId::new("task_wf02"),
        task(Some(AttemptId::new("att_wf02"))),
    );
    snap.attempts
        .insert(AttemptId::new("att_wf02"), attempt(AttemptState::Running));
    assert!(detect_recovery_required(&snap, 0));
}

#[test]
fn workfin02_detect_recovery_required_complete_without_finish() {
    let mut snap = empty_snapshot(RunState::Active, None);
    snap.tasks.insert(TaskId::new("task_wf02"), task(None));
    snap.attempts
        .insert(AttemptId::new("att_wf02"), attempt(AttemptState::Succeeded));
    assert!(detect_recovery_required(&snap, 0));
}

#[test]
fn workfin02_detect_recovery_required_claim_plus_active_even_if_attempt_failed() {
    let mut snap = empty_snapshot(RunState::Active, Some(job_spec()));
    snap.tasks.insert(
        TaskId::new("task_wf02"),
        task(Some(AttemptId::new("att_wf02"))),
    );
    snap.attempts
        .insert(AttemptId::new("att_wf02"), attempt(AttemptState::Failed));
    assert!(detect_recovery_required(&snap, 0));
}

#[test]
fn workfin02_detect_recovery_required_job_spec_active_terminal_failed() {
    let mut snap = empty_snapshot(RunState::Active, Some(job_spec()));
    snap.tasks.insert(TaskId::new("task_wf02"), task(None));
    snap.attempts
        .insert(AttemptId::new("att_wf02"), attempt(AttemptState::Failed));
    assert!(detect_recovery_required(&snap, 0));
}

#[test]
fn workfin02_start_run_without_attempt_is_not_d() {
    let snap = empty_snapshot(RunState::Active, Some(job_spec()));
    assert!(!detect_recovery_required(&snap, 0));
}

#[test]
fn workfin02_created_attempt_is_not_d() {
    let mut snap = empty_snapshot(RunState::Active, Some(job_spec()));
    snap.tasks.insert(TaskId::new("task_wf02"), {
        let mut t = task(None);
        t.state = TaskState::Ready;
        t.active_attempt = None;
        t
    });
    snap.attempts
        .insert(AttemptId::new("att_wf02"), attempt(AttemptState::Created));
    assert!(!detect_recovery_required(&snap, 0));
}

#[test]
fn workfin02_preclaim_fail_attempt_before_finish_run() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Failures);
}

#[test]
fn workfin02_fail_hop_without_job_spec_is_not_d() {
    let infer = crate_src("../../mantle/tetonic-run/src/infer_admission.rs");
    assert!(infer.contains("job_spec: None"));
    let mut snap = empty_snapshot(RunState::Active, None);
    snap.tasks.insert(TaskId::new("task_wf02"), task(None));
    snap.attempts
        .insert(AttemptId::new("att_wf02"), attempt(AttemptState::Failed));
    assert!(!detect_recovery_required(&snap, 0));
}

#[test]
fn workfin02_recover_at_startup_is_sole_recovery_required_writer() {
    let recovery = crate_src("../../mantle/tetonic-run/src/recovery.rs");
    assert!(!recovery.contains("fn mark_recovery_required"));
    let service = crate_src("../../mantle/tetonic-run/src/service.rs");
    let body = fn_body(&service, "fn recover_at_startup(");
    assert!(body.contains("snap.state = RunState::RecoveryRequired"));
    let prod = production_prefix(&service);
    let writers = prod
        .matches("snap.state = RunState::RecoveryRequired")
        .count();
    assert_eq!(
        writers, 1,
        "recover_at_startup must be the only RecoveryRequired writer"
    );
}

#[test]
fn workfin02_mid_terminal_failure_leaves_derivable_condition() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Failures);
}

#[test]
fn workfin02_effectful_commit_records_side_effect() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::SideEffects);
}

#[test]
fn workfin02_response_only_skips_side_effect() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::SideEffects);
}

#[test]
fn workfin02_txn_artifact_binds_task_and_attempt() {
    let txn = crate_src("../../core/tetonic-transaction/src/service.rs");
    assert!(txn.contains("pub task_id: Option<TaskId>"));
    assert!(txn.contains("pub attempt_id: Option<AttemptId>"));
    let commit = fn_body(&txn, "pub fn commit(");
    assert!(commit.contains("task_id: self.task_id.clone()"));
    assert!(commit.contains("attempt_id: self.attempt_id.clone()"));
}

#[test]
fn workfin02_bind_effect_identity_sets_txn_fields() {
    let tools = crate_src("../../litho/tetonic-tools/src/lib.rs");
    assert!(tools.contains("pub fn bind_effect_identity"));
    let mutation = crate_src("../../litho/tetonic-tools/src/mutation.rs");
    let body = fn_body(&mutation, "pub fn bind_effect_identity(");
    assert!(body.contains("with_active_if_any"));
    assert!(body.contains("txn.task_id = Some(task_id.clone())"));
    assert!(body.contains("txn.attempt_id = Some(attempt_id.clone())"));
}

#[test]
fn workfin02_sessionless_does_not_bind_effect_identity() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Sessionless);
}

#[test]
fn workfin02_journal_persist_fsyncs() {
    let src = crate_src("../../core/tetonic-transaction/src/journal.rs");
    let body = fn_body(&src, "pub fn persist(");
    assert!(body.contains("sync_all"));
    source_order(body, "rename", "OpenOptions::new()", "sync_all");
}

#[test]
fn workfin02_journal_intent_already_before_first_apply() {
    let src = crate_src("../../core/tetonic-transaction/src/service.rs");
    let body = fn_body(&src, "pub fn commit(");
    source_order(body, "journal.persist", "apply_journal", "Committed");
    let before_apply = &body[..body.find("apply_journal").expect("apply")];
    assert_eq!(
        before_apply.matches("journal.persist").count(),
        1,
        "do not persist Committing intent a second time"
    );
}

#[test]
fn workfin02_apply_without_durable_marker_is_txn_recovery_required() {
    let src = crate_src("../../core/tetonic-transaction/src/service.rs");
    let body = fn_body(&src, "pub fn commit(");
    source_order(
        body,
        "Err(e) =>",
        "journal.state = TransactionState::RecoveryRequired",
        "self.transition(TransactionState::RecoveryRequired)",
    );
}

#[test]
fn workfin02_result_accept_does_not_build_complete_attempt() {
    let src = crate_src("../../atmos/tetonic-fabric-client/src/result_accept.rs");
    assert!(!src.contains("fn build_complete_attempt"));
    assert!(!production_prefix(&src).contains("try_accept_completion"));
}

#[test]
fn workfin02_legacy_result_does_not_apply_complete_attempt() {
    let src = crate_src("../../atmos/tetonic-fabric-client/src/legacy_result.rs");
    assert!(!production_prefix(&src).contains("apply_complete_attempt"));
}

#[test]
fn workfin02_bridge_does_not_submit_complete_attempt() {
    let trait_src = crate_src("../../atmos/tetonic-fabric-client/src/run_bridge.rs");
    assert!(!trait_src.contains("apply_complete_attempt"));
    let bridge = crate_src("src/fabric_run_bridge.rs");
    let prod = production_prefix(&bridge);
    assert!(!prod.contains("apply_complete_attempt"));
    assert!(!prod.contains("RunCommand::CompleteAttempt"));
}

#[test]
fn workfin02_no_public_remote_patch_commit() {
    let app = crate_src("src/lib.rs");
    assert!(!production_prefix(&app).contains("apply_authorized_remote_patch"));
    let fabric = crate_src("../../atmos/tetonic-fabric-client/src/legacy_result.rs");
    assert!(!production_prefix(&fabric).contains("apply_authorized_remote_patch"));
    let fin = crate_src("src/run_service.rs");
    assert!(!fin.contains("apply_verified_remote_patch"));
}

#[test]
fn workfin02_fin002_not_established_run_turn_and_leftover_turn_remain() {
    let run = crate_src("src/run_service.rs");
    assert!(run.contains("async fn run_turn("));
    let exec = crate_src("src/turn_execution.rs");
    assert!(exec.contains("async fn execute_spawn"));
    let live = crate_src("src/session_live.rs");
    assert!(live.contains("pub struct LiveSession"));
}

#[test]
fn workfin02_cmp001_not_established_until_gate() {
    let fix = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tooling/tetonic-arch-gate/fixtures/v4/ARCH-V4-CMP-001.v4fix");
    assert!(fix.exists(), "ARCH-V4-CMP-001.v4fix must stay planted");
    let text = fs::read_to_string(&fix).unwrap();
    assert!(text.contains("production-failing detector live"));
    assert!(text.contains("Not ESTABLISHED"));
}

#[test]
fn workfin02_id001_not_established_session_chat_remains() {
    let live = crate_src("src/session_live.rs");
    assert!(live.contains("pub struct LiveSession"));
}

#[test]
fn workfin02_arch_fin_002_still_planted() {
    let fix = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tooling/tetonic-arch-gate/fixtures/v4/ARCH-V4-FIN-002.v4fix");
    assert!(fix.exists(), "ARCH-V4-FIN-002.v4fix must stay planted");
    let corpus = crate_src("../../tooling/tetonic-arch-gate/src/v4_corpus.rs");
    assert!(corpus.contains("ARCH-V4-FIN-002.v4fix"));
}

#[path = "support/lifecycle_contract.rs"]
mod lifecycle_contract;

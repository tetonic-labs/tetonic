//! Source contracts follow the production owner after manager consolidation.
#![allow(dead_code)]
use std::path::PathBuf;
fn source(module: &str) -> String {
    std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../mantle/lokai-run/src/managed")
            .join(format!("{module}.rs")),
    )
    .unwrap()
}
fn product(module: &str) -> String {
    std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join(format!("{module}.rs")),
    )
    .unwrap()
}
fn compact(s: &str) -> String {
    s.split_whitespace().collect()
}
fn before(s: &str, first: &str, second: &str) {
    assert!(
        s.find(first).unwrap_or_else(|| panic!("missing {first}"))
            < s.find(second).unwrap_or_else(|| panic!("missing {second}"))
    );
}
#[derive(Clone, Copy)]
pub enum Contract {
    RootKeys,
    Sessionless,
    ChildAdmission,
    Registry,
    Execution,
    Cleanup,
    Heartbeat,
    Cancellation,
    FinalizationOrder,
    ClaimFence,
    Failures,
    SideEffects,
    ProductDispatch,
}
pub fn assert_contract(contract: Contract) {
    use Contract::*;
    match contract {
        RootKeys => {
            let s = source("admission");
            assert!(s.contains("format!(\"task_root_{}\", run_id)"));
            assert!(s.contains("format!(\"turn:{}:{}\", run_id, attempt_id)"));
            assert!(!s.contains("task_root_{session}"));
        }
        Sessionless => {
            let adapter = compact(&product("identity_job"));
            assert!(adapter.contains("self.managed.start_identity_job(cmd,agent).await"));
            let s = source("execution");
            assert!(s.contains("AdmitJob"));
            assert!(s.contains("parent_attempt: None"));
            assert!(s.contains("policy: None"));
            assert!(s.contains("finish_run: true"));
            assert!(!s.contains("CodingAgentDefinition"));
            assert!(!s.contains("Tools"));
            let admission = source("admission");
            assert!(admission.contains("AdmissionContext::default()"));
        }
        ChildAdmission => {
            let s = source("admission");
            let child = s.split("if let Some(parent_attempt)").nth(1).unwrap();
            before(child, "RunCommand::AddTask", "RunCommand::CreateAttempt");
            before(
                child,
                "RunCommand::CreateAttempt",
                "RunCommand::LeaseAttempt",
            );
            before(
                child,
                "RunCommand::LeaseAttempt",
                "RunCommand::StartAttempt",
            );
            assert!(!s.contains("AddDependency"));
            assert!(product("identity_job").contains("admit_with_context"));
        }
        Registry => {
            assert!(source("service").contains("HashMap<AttemptId, ActiveAttempt>"));
            assert!(source("admission").contains("attempt_id.clone(),"));
            assert!(!product("run_service").contains("HashMap<AttemptId, ActiveTurnRun>"));
            assert!(!product("run_service").contains("attempt_joins:"));
        }
        Execution => {
            let s = source("execution");
            before(&s, "ClaimExecution", "LocalAgentAttemptExecutor");
            assert!(s.contains("stamp_managed_run"));
            assert!(compact(&product("identity_job")).contains("self.managed.execute_attempt("));
        }
        Cleanup => {
            let s = source("finalization");
            before(
                &s,
                "RunCommand::AcceptArtifact",
                "self.deliver_terminal(&active, final_outcome.clone())",
            );
            assert!(
                s.contains("self.active.lock_recover().remove(&active.binding.attempt_id)")
                    || compact(&s)
                        .contains("self.active.lock_recover().remove(&active.binding.attempt_id)")
            );
            assert!(product("turn_finalization").contains("live.clear_current_run()"));
            assert!(
                compact(&product("run_service")).contains("self.managed.binding(&cmd.attempt_id)")
            );
        }
        Heartbeat => {
            let s = source("service");
            assert!(s.contains("spawn_heartbeat_driver"));
            assert!(s.contains("this.heartbeat(&attempt_id).await"));
            assert!(s.contains("this.cancel_dispatch(&id)"));
            let life = source("lifetime");
            assert!(life.contains("RunCommand::RecordHeartbeat"));
            assert!(life.contains("heartbeat_cancel: Arc<AtomicBool>"));
            assert!(life.contains("task_handle: Option<AbortHandle>"));
        }
        Cancellation => {
            let s = source("service");
            assert!(s.contains("self.cancel_dispatch(&id)?"));
            let lifetime = source("lifetime");
            let cancel = lifetime
                .split("pub fn cancel_dispatch(")
                .nth(1)
                .unwrap()
                .split("pub fn is_canceled(")
                .next()
                .unwrap();
            assert!(cancel.contains("*canceled = true"));
            assert!(
                !cancel.contains("task.abort()"),
                "cancellation must let execution own cleanup"
            );
            assert!(source("execution").contains("tokio::select!"));
            let f = source("finalization");
            let canceled = f
                .split("CandidateOutcome::Canceled { reason: _ } =>")
                .nth(1)
                .unwrap()
                .split("CandidateOutcome::Failed { message }")
                .next()
                .unwrap();
            assert!(canceled.contains("if !job.finish_run"));
            assert!(canceled.contains("self.fail_and_finish"));
            assert!(!canceled.contains("CompleteAttempt"));
            assert!(!canceled.contains("AcceptArtifact"));
        }
        FinalizationOrder => {
            let s = source("finalization");
            before(&s, "RunCommand::ClaimFinalization", "driver.run_verify");
            before(&s, "driver.run_verify", "driver.commit_workspace");
            before(
                &s,
                "driver.commit_workspace",
                "seal_output_set(&self.artifacts",
            );
            before(
                &s,
                "RunCommand::CompleteAttempt",
                "RunCommand::AcceptArtifact",
            );
            before(&s, "RunCommand::AcceptArtifact", "RunCommand::FinishRun");
            assert!(s.contains("encode_candidate_bytes(&job.outcome)?"));
        }
        ClaimFence => {
            let s = source("finalization");
            before(
                &s,
                "self.heartbeat(&attempt_id).await?",
                "RunCommand::ClaimFinalization",
            );
            before(&s, "if claim.idempotent_replay", "driver.commit_workspace");
            assert!(s.contains("if matches!(job.outcome, CandidateOutcome::Completed"));
            assert!(s.contains("finalization already claimed"));
            assert!(!s.contains("unwrap_or(seq)"));
        }
        Failures => {
            let s = source("finalization");
            assert!(s.contains(".map_err(|e| ManagedRunError::PersistenceFailed(e.to_string()))?"));
            assert!(s.contains("FailureClass::VerificationFailed"));
            let fail = s.split("async fn fail_and_finish").nth(1).unwrap();
            before(fail, "RunCommand::FailAttempt", "RunCommand::FinishRun");
            assert!(!fail.contains("CompleteAttempt"));
            assert!(s.contains(".await?;")); // sealing errors propagate without success cleanup
        }
        SideEffects => {
            let s = source("finalization");
            before(
                &s,
                "driver.commit_workspace",
                "RunCommand::RecordSideEffectCommit",
            );
            before(
                &s,
                "RunCommand::RecordSideEffectCommit",
                "RunCommand::CompleteAttempt",
            );
            assert!(s.contains("if let Some(commit) = committed"));
            assert!(s.contains("if let Some(driver) = &policy.effect_driver"));
        }
        ProductDispatch => {
            assert!(!product("attempt_completion").contains("spawn_local"));
            assert!(compact(&product("attempt_completion")).contains("self.managed.spawn_dispatch"));
            assert!(source("lifetime").contains("tokio::task::spawn_local(future)"));
            assert!(source("lifetime").contains("self.attach_task(id, task.abort_handle())"));
        }
    }
}

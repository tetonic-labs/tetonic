//! CODE-02 IMPLEMENT source pins. Distinctive `009` pins are run-shaped keys.
//! Distinctive `025` pins are the `turn.is_some()` incremental wrap.

use std::fs;
use std::path::PathBuf;

fn crate_src(rel: &str) -> String {
    fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)).expect(rel)
}

fn turn_src() -> String {
    crate_src("src/turn_execution.rs")
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
    panic!("unclosed {sig}");
}

#[test]
fn code02_begin_job_run_task_id_is_run_not_session() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::RootKeys);
}

#[test]
fn code02_begin_job_run_delivery_key_is_run_not_session() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::RootKeys);
}

#[test]
fn code02_sessionless_still_task_root_run() {
    lifecycle_contract::assert_contract(lifecycle_contract::Contract::Sessionless);
}

#[test]
fn code02_build_agent_wraps_only_when_turn_some() {
    let owned = turn_src();
    let src = production_prefix(&owned);
    let body = fn_body(src, "fn build_agent(");
    assert!(body.contains("if turn.is_some()"));
    assert!(body.contains("ExecutionProjectionAudit"));
}

#[test]
fn code02_wrap_persists_message_incrementally() {
    let owned = turn_src();
    let src = production_prefix(&owned);
    assert!(src.contains("impl lokai_core::AuditSink for ExecutionProjectionAudit"));
    let message = fn_body(
        src,
        "fn message(&self, role: &str, content: &str, tool_calls_json: Option<&str>)",
    );
    assert!(message.contains("self.persist("));
    assert!(!message.contains("self.inner.message("));
}

#[test]
fn code02_wrap_skips_plan_user_once() {
    let owned = turn_src();
    let src = production_prefix(&owned);
    let message = fn_body(
        src,
        "fn message(&self, role: &str, content: &str, tool_calls_json: Option<&str>)",
    );
    assert!(message.contains("role == \"user\""));
    assert!(message.contains("content == self.plan_user"));
    assert!(message.contains("skipped_plan_user.swap(true, Ordering::SeqCst)"));
}

#[test]
fn code02_wrap_persists_nudges_and_system() {
    let owned = turn_src();
    let src = production_prefix(&owned);
    let message = fn_body(
        src,
        "fn message(&self, role: &str, content: &str, tool_calls_json: Option<&str>)",
    );
    assert!(!message.contains("role == \"system\""));
    assert!(message.contains("self.persist(role, content, tool_calls_json, None, None)"));
}

#[test]
fn code02_wrap_uses_write_sync_append_message_with() {
    let owned = turn_src();
    let src = production_prefix(&owned);
    let persist = fn_body(src, "fn persist(");
    assert!(persist.contains("write_sync"));
    assert!(persist.contains("append_message_with("));
    assert!(!persist.contains(".write("));
}

#[test]
fn code02_wrap_skip_flag_is_arc_atomic_bool() {
    let owned = turn_src();
    let src = production_prefix(&owned);
    assert!(src.contains("skipped_plan_user: Arc<AtomicBool>"));
    assert!(!src.contains("Rc<Cell<bool>>"));
}

#[test]
fn code02_wrap_persists_build_agent_id() {
    let owned = turn_src();
    let src = production_prefix(&owned);
    let persist = fn_body(src, "fn persist(");
    assert!(persist.contains("&agent_id"));
    let body = fn_body(src, "fn build_agent(");
    assert!(body.contains("agent_id: agent_id.clone()"));
    assert!(!persist.contains("\"\""));
}

#[test]
fn code02_run_turn_body_has_no_checkpoint_suffix_persist() {
    let owned = turn_src();
    let src = production_prefix(&owned);
    let body = fn_body(src, "async fn run_turn_body(");
    assert!(!body.contains("append_message"));
    assert!(!body.contains("checkpoint"));
    assert!(!body.contains("conversation.messages"));
}

#[test]
fn code02_execute_spawn_keeps_inner_audit() {
    let owned = turn_src();
    let src = production_prefix(&owned);
    let body = fn_body(src, "async fn execute_spawn(");
    assert!(body.contains("build_agent("));
    assert!(body.contains("None,"));
}

#[test]
fn code02_complete_turn_has_no_conversation() {
    let src = crate_src("src/run_service.rs");
    let start = src
        .rfind("async fn complete_turn(")
        .expect("complete_turn impl");
    let sig_end = src[start..].find('{').expect("sig");
    let sig = &src[start..start + sig_end];
    assert!(!sig.contains("Conversation"));
}

#[test]
fn code02_eval_kernel_untouched() {
    let src = crate_src("../../tooling/tetonic-eval/src/kernel.rs");
    assert!(src.contains("verify_cmd: None"));
    assert!(!src.contains("struct StoreAudit"));
    assert!(!src.contains("fn session_audit"));
}

#[path = "support/lifecycle_contract.rs"]
mod lifecycle_contract;

//! Product inner `AuditFactory`: store tool-projection (eval StoreAudit / CLI Audit shape).
//! Not the CODE-02 `ExecutionProjectionAudit` wrap.

use lokai_core::AuditSink;
use lokai_memory::SharedStore;
use lokai_runtime::NullAudit;
use std::sync::Arc;

use crate::turn_execution::AuditFactory;

pub struct StoreAuditFactory {
    store: SharedStore,
}

impl StoreAuditFactory {
    pub fn new(store: SharedStore) -> Arc<Self> {
        Arc::new(Self { store })
    }
}

impl AuditFactory for StoreAuditFactory {
    fn session_audit(&self, session_id: &str, _agent_id: &str) -> Box<dyn AuditSink> {
        Box::new(StoreAudit {
            store: self.store.clone(),
            session: session_id.to_string(),
        })
    }
}

pub fn product_audit_factory(store: &Option<SharedStore>) -> Option<Arc<dyn AuditFactory>> {
    store
        .as_ref()
        .map(|s| StoreAuditFactory::new(s.clone()) as Arc<dyn AuditFactory>)
}

struct StoreAudit {
    store: SharedStore,
    session: String,
}

impl AuditSink for StoreAudit {
    fn message(&self, role: &str, content: &str, tool_calls_json: Option<&str>) {
        let session = self.session.clone();
        let role = role.to_string();
        let content = content.to_string();
        let tool_calls_json = tool_calls_json.map(|s| s.to_string());
        let _ = self.store.write_sync(move |db| {
            db.append_message_with(
                &session,
                &role,
                "",
                &content,
                tool_calls_json.as_deref(),
                None,
                None,
            )
        });
    }

    fn tool_call(
        &self,
        id: &str,
        tool: &str,
        args_json: &str,
        ok: bool,
        summary: &str,
        error_kind: Option<&str>,
    ) {
        let id = id.to_string();
        let session = self.session.clone();
        let tool = tool.to_string();
        let args_json = args_json.to_string();
        let summary = summary.to_string();
        let error_kind = error_kind.map(|s| s.to_string());
        let _ = self.store.write_sync(move |db| {
            db.record_tool_call(
                &id,
                &session,
                &tool,
                &args_json,
                ok,
                &summary,
                error_kind.as_deref(),
            )
        });
    }

    fn file_change(
        &self,
        tool_call_id: &str,
        path: &str,
        kind: &str,
        before: Option<&str>,
        after: Option<&str>,
    ) {
        let tool_call_id = tool_call_id.to_string();
        let session = self.session.clone();
        let path = path.to_string();
        let kind = kind.to_string();
        let before = before.map(|s| s.to_string());
        let after = after.map(|s| s.to_string());
        let _ = self.store.write_sync(move |db| {
            db.record_file_change(
                &tool_call_id,
                &session,
                &path,
                &kind,
                before.as_deref(),
                after.as_deref(),
            )
        });
    }

    fn note(&self, text: &str) {
        let payload = serde_json::json!({ "text": text }).to_string();
        let session = self.session.clone();
        let _ = self
            .store
            .write_sync(move |db| db.append_event(&session, "note", "system", &payload));
    }

    fn audit_persists(&self) -> bool {
        true
    }
}

pub fn null_if_absent(store: &Option<SharedStore>) -> Option<Arc<dyn AuditFactory>> {
    if store.is_some() {
        product_audit_factory(store)
    } else {
        None
    }
}

#[allow(dead_code)]
fn _null_audit_symbol() -> Box<dyn AuditSink> {
    Box::new(NullAudit)
}

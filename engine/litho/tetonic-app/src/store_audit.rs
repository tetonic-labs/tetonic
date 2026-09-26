//! Product inner `AuditFactory`: store tool-projection (eval StoreAudit / CLI Audit shape).
//! Not the CODE-02 `ExecutionProjectionAudit` wrap.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tetonic_core::AuditSink;
use tetonic_memory::SharedStore;
use tetonic_runtime::NullAudit;

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
    fn session_audit(&self, session_id: &str, agent_id: &str) -> Box<dyn AuditSink> {
        Box::new(StoreAudit {
            store: self.store.clone(),
            session: session_id.to_string(),
            agent_id: agent_id.to_string(),
            scoped: None,
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
    agent_id: String,
    scoped: Option<(String, Arc<AtomicBool>)>,
}

/// Reuse the product audit writer with an immutable context and a failure latch.
/// The execution authority checks the latch before further work/finalization.
pub(crate) fn scoped_execution_audit(
    store: SharedStore,
    context: String,
    session: String,
    agent_id: String,
    failed: Arc<AtomicBool>,
) -> Box<dyn AuditSink> {
    Box::new(StoreAudit {
        store,
        session,
        agent_id,
        scoped: Some((context, failed)),
    })
}

impl StoreAudit {
    fn call_id(&self, id: &str) -> String {
        if self.scoped.is_some() {
            format!("{}:{id}", self.session)
        } else {
            id.to_owned()
        }
    }

    fn write<R: Send + 'static>(
        &self,
        write: impl FnOnce(&mut tetonic_memory::Store) -> Result<R, tetonic_memory::StoreError>
            + Send
            + 'static,
    ) {
        let context = self.scoped.as_ref().map(|(context, _)| context.clone());
        let session = self.session.clone();
        let result = self.store.write_sync(move |db| {
            if let Some(context) = context {
                db.require_execution_audit_history(&context, &session)?;
            }
            write(db)
        });
        if !matches!(result, Ok(Ok(_))) {
            if let Some((_, failed)) = &self.scoped {
                failed.store(true, Ordering::SeqCst);
            }
            tracing::error!("execution audit write failed");
        }
    }
}

impl AuditSink for StoreAudit {
    fn message(&self, role: &str, content: &str, tool_calls_json: Option<&str>) {
        let session = self.session.clone();
        let role = role.to_string();
        let agent = self.agent_id.clone();
        let content = content.to_string();
        let tool_calls_json = tool_calls_json.map(|s| s.to_string());
        self.write(move |db| {
            db.append_message_with(
                &session,
                &role,
                &agent,
                &content,
                tool_calls_json.as_deref(),
                None,
                None,
            )
        });
    }

    fn tool_message(&self, name: &str, tool_call_id: &str, content: &str) {
        let session = self.session.clone();
        let agent = self.agent_id.clone();
        let name = name.to_owned();
        let id = self.call_id(tool_call_id);
        let content = content.to_owned();
        self.write(move |db| {
            db.append_message_with(
                &session,
                "tool",
                &agent,
                &content,
                None,
                Some(&name),
                Some(&id),
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
        let id = self.call_id(id);
        let session = self.session.clone();
        let tool = tool.to_string();
        let args_json = args_json.to_string();
        let summary = summary.to_string();
        let error_kind = error_kind.map(|s| s.to_string());
        self.write(move |db| {
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
        let tool_call_id = self.call_id(tool_call_id);
        let session = self.session.clone();
        let path = path.to_string();
        let kind = kind.to_string();
        let before = before.map(|s| s.to_string());
        let after = after.map(|s| s.to_string());
        self.write(move |db| {
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
        self.write(move |db| db.append_event(&session, "note", "system", &payload));
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scoped_audit_namespaces_call_ids_and_latches_boundary_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.db");
        let store = SharedStore::open(&path, 1).unwrap();
        store
            .write_sync(|db| {
                db.bootstrap_control("admin", "org", "Org").unwrap();
                for context in ["one", "two"] {
                    db.create_information_context(
                        "admin",
                        context,
                        &tetonic_memory::ContextOwner::Private {
                            org_id: "org".into(),
                        },
                    )
                    .unwrap();
                    db.create_execution_audit_history("admin", context, context)
                        .unwrap();
                }
            })
            .unwrap();
        for context in ["one", "two"] {
            let failed = Arc::new(AtomicBool::new(false));
            let audit = scoped_execution_audit(
                store.clone(),
                context.into(),
                context.into(),
                "agent".into(),
                failed.clone(),
            );
            audit.tool_call("same-call", "read_file", "{}", true, context, None);
            audit.tool_message("read_file", "same-call", context);
            assert!(!failed.load(Ordering::SeqCst));
        }
        let raw = rusqlite::Connection::open(&path).unwrap();
        for context in ["one", "two"] {
            let summary: String = raw
                .query_row(
                    "SELECT result_summary FROM tool_calls WHERE id=?1 AND session_id=?2",
                    [format!("{context}:same-call"), context.into()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(summary, context);
        }
        let failed = Arc::new(AtomicBool::new(false));
        let invalid = scoped_execution_audit(
            store,
            "two".into(),
            "one".into(),
            "agent".into(),
            failed.clone(),
        );
        invalid.message("assistant", "WRONGSCOPE", None);
        assert!(failed.load(Ordering::SeqCst));
        let count: i64 = raw
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE content='WRONGSCOPE'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }
}

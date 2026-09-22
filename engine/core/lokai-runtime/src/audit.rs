//! Best-effort audit sink when no session store is attached (CLI one-shot paths).

use lokai_core::AuditSink;

/// Discards audit events; satisfies production assembly when persistence is unavailable.
pub struct NullAudit;

impl AuditSink for NullAudit {
    fn message(&self, _role: &str, _content: &str, _tool_calls_json: Option<&str>) {}

    fn tool_call(
        &self,
        _id: &str,
        _tool: &str,
        _args_json: &str,
        _ok: bool,
        _summary: &str,
        _error_kind: Option<&str>,
    ) {
    }

    fn file_change(
        &self,
        _tool_call_id: &str,
        _path: &str,
        _kind: &str,
        _before: Option<&str>,
        _after: Option<&str>,
    ) {
    }

    fn note(&self, _text: &str) {}

    fn audit_persists(&self) -> bool {
        false
    }
}

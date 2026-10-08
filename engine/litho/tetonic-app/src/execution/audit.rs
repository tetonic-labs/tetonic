//! Audit writer construction for registered harness assembly.
/// Builds session-scoped audit sinks for agent assembly.
pub trait AuditFactory: Send + Sync {
    fn session_audit(&self, session_id: &str, agent_id: &str) -> Box<dyn tetonic_core::AuditSink>;
}

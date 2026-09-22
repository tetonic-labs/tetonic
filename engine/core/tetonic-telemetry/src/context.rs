use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable trace context injected across application boundaries.
///
/// Business IDs (`session_id`, `run_id`, …) are plain strings so they can carry
/// live Lokai ids (`sess_…`, `run_…`) without inventing fake UUIDs (M0-3 / R02).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceContext {
    pub session_id: Option<String>,
    pub run_id: Option<String>,
    pub task_id: Option<String>,
    pub attempt_id: Option<String>,
    pub agent_id: Option<String>,
    pub worker_id: Option<String>,
    pub workspace_version: Option<u64>,
    /// ComputeBroker / scheduler decision correlation (M6-3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduler_decision_id: Option<String>,
    /// Resource reservation id when admitted (M6-3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reservation_id: Option<String>,
    pub trace_id: Uuid,
    pub parent_span_id: Option<Uuid>,
    pub span_id: Uuid,
}

impl Default for TraceContext {
    fn default() -> Self {
        Self {
            session_id: None,
            run_id: None,
            task_id: None,
            attempt_id: None,
            agent_id: None,
            worker_id: None,
            workspace_version: None,
            scheduler_decision_id: None,
            reservation_id: None,
            trace_id: Uuid::new_v4(),
            parent_span_id: None,
            span_id: Uuid::new_v4(),
        }
    }
}

impl TraceContext {
    /// Creates a new child context from the current context.
    pub fn child(&self) -> Self {
        Self {
            parent_span_id: Some(self.span_id),
            span_id: Uuid::new_v4(),
            ..self.clone()
        }
    }

    pub fn with_scheduler_decision_id(mut self, id: impl Into<String>) -> Self {
        self.scheduler_decision_id = Some(id.into());
        self
    }

    pub fn with_reservation_id(mut self, id: impl Into<String>) -> Self {
        self.reservation_id = Some(id.into());
        self
    }

    /// Bind live session identity (R02). Preserves trace/span lineage.
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Bind live turn identities from the durable run plan (R02).
    pub fn with_turn_ids(
        mut self,
        session_id: impl Into<String>,
        run_id: impl Into<String>,
        task_id: Option<impl Into<String>>,
    ) -> Self {
        self.session_id = Some(session_id.into());
        self.run_id = Some(run_id.into());
        self.task_id = task_id.map(|t| t.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_preserves_scheduler_and_reservation_ids() {
        let parent = TraceContext::default()
            .with_scheduler_decision_id("sched_1")
            .with_reservation_id("res_1");
        let child = parent.child();
        assert_eq!(child.scheduler_decision_id.as_deref(), Some("sched_1"));
        assert_eq!(child.reservation_id.as_deref(), Some("res_1"));
        assert_eq!(child.parent_span_id, Some(parent.span_id));
        assert_ne!(child.span_id, parent.span_id);
        assert_eq!(child.trace_id, parent.trace_id);
    }

    #[test]
    fn with_turn_ids_sets_real_string_ids() {
        let ctx = TraceContext::default().with_turn_ids("sess_abc", "run_xyz", Some("task_1"));
        assert_eq!(ctx.session_id.as_deref(), Some("sess_abc"));
        assert_eq!(ctx.run_id.as_deref(), Some("run_xyz"));
        assert_eq!(ctx.task_id.as_deref(), Some("task_1"));
    }
}

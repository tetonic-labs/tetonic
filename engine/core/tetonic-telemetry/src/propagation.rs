use std::cell::RefCell;

use crate::TraceContext;
use tracing::Span;

tokio::task_local! {
    static TASK_CONTEXT: RefCell<Option<TraceContext>>;
}

/// Scope correlation state to this future, independently of executor threads.
pub async fn scope_context<F: std::future::Future>(context: TraceContext, future: F) -> F::Output {
    TASK_CONTEXT
        .scope(RefCell::new(Some(context)), future)
        .await
}

/// Injects the given TraceContext into the task scope, or an explicitly active synchronous span.
///
/// Safe to call repeatedly on the same span: span extensions are **replaced**
/// (tracing-subscriber `insert` panics if the type is already present).
pub fn inject_context(context: TraceContext) {
    if TASK_CONTEXT
        .try_with(|c| *c.borrow_mut() = Some(context.clone()))
        .is_ok()
    {
        return;
    }
    let span = Span::current();
    span.with_subscriber(|(id, dispatch)| {
        if let Some(registry) = dispatch.downcast_ref::<tracing_subscriber::Registry>() {
            use tracing_subscriber::registry::LookupSpan;
            if let Some(span_ref) = registry.span(id) {
                let mut extensions = span_ref.extensions_mut();
                let _ = extensions.replace(context);
            }
        }
    });
}

/// Extracts a TraceContext from the task scope, or the active synchronous span.
pub fn extract_context() -> Option<TraceContext> {
    if let Ok(context) = TASK_CONTEXT.try_with(|c| c.borrow().clone()) {
        return context;
    }
    let span = Span::current();
    let mut found = None;
    span.with_subscriber(|(id, dispatch)| {
        if let Some(registry) = dispatch.downcast_ref::<tracing_subscriber::Registry>() {
            use tracing_subscriber::registry::LookupSpan;
            if let Some(span_ref) = registry.span(id) {
                for current_span in span_ref.scope() {
                    if let Some(ctx) = current_span.extensions().get::<TraceContext>() {
                        found = Some(ctx.clone());
                        break;
                    }
                }
            }
        }
    });
    found
}

/// After session start: root context carries the live `session_id` (R02).
pub fn inject_session_context(session_id: &str) {
    let ctx = extract_context()
        .unwrap_or_default()
        .with_session_id(session_id);
    inject_context(ctx);
}

/// After turn begin: root context carries live session/run/(task) ids (R02).
pub fn inject_turn_context(session_id: &str, run_id: &str, task_id: Option<&str>) {
    let ctx = extract_context()
        .unwrap_or_default()
        .with_turn_ids(session_id, run_id, task_id);
    inject_context(ctx);
}

/// Enter a child stage, inheriting business ids from the root turn context (R03).
///
/// Emits a debug event with only correlation ids (no payloads/secrets).
pub fn enter_stage_child(stage: &'static str) -> TraceContext {
    let child = extract_context().unwrap_or_default().child();
    inject_context(child.clone());
    tracing::debug!(
        target: "lokai.trace.stage",
        stage,
        session_id = child.session_id.as_deref().unwrap_or(""),
        run_id = child.run_id.as_deref().unwrap_or(""),
        task_id = child.task_id.as_deref().unwrap_or(""),
        "stage"
    );
    child
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn inject_turn_context_populates_task_scope() {
        scope_context(Default::default(), async {
            inject_turn_context("sess_1", "run_1", Some("task_1"));
            let ctx = extract_context().expect("context");
            assert_eq!(ctx.session_id.as_deref(), Some("sess_1"));
            assert_eq!(ctx.run_id.as_deref(), Some("run_1"));
            assert_eq!(ctx.task_id.as_deref(), Some("task_1"));
        })
        .await;
    }

    #[tokio::test]
    async fn stage_child_inherits_turn_ids() {
        scope_context(Default::default(), async {
            inject_turn_context("sess_a", "run_b", Some("task_c"));
            let infer = enter_stage_child("infer");
            assert_eq!(infer.session_id.as_deref(), Some("sess_a"));
            assert_eq!(infer.run_id.as_deref(), Some("run_b"));
            let tool = enter_stage_child("tool");
            assert_eq!(tool.session_id.as_deref(), Some("sess_a"));
            assert_eq!(tool.run_id.as_deref(), Some("run_b"));
            assert_ne!(tool.span_id, infer.span_id);
        })
        .await;
    }

    #[test]
    fn reinject_on_same_span_does_not_panic() {
        use tracing_subscriber::prelude::*;

        let subscriber = tracing_subscriber::registry()
            .with(tracing_subscriber::fmt::layer().with_writer(std::io::sink));
        let _guard = tracing::subscriber::set_default(subscriber);
        let span = tracing::info_span!("reinject_test");
        let _enter = span.enter();

        inject_context(TraceContext::default());
        inject_session_context("sess_re");
        inject_turn_context("sess_re", "run_re", Some("task_re"));
        let ctx = extract_context().expect("ctx");
        assert_eq!(ctx.session_id.as_deref(), Some("sess_re"));
        assert_eq!(ctx.run_id.as_deref(), Some("run_re"));
    }
}

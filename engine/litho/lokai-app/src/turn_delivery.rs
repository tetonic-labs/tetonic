//! Publish completion once, after the product releases its conversation lease.
use crate::events::{ApplicationEvent, ApplicationEventSink, EventEnvelope};
use lokai_memory::RecoverMutex;
use std::sync::{Arc, Mutex};

pub(crate) struct TurnDelivery {
    session_id: String,
    sink: Arc<dyn ApplicationEventSink>,
    state: Mutex<DeliveryState>,
}

#[derive(Default)]
struct DeliveryState {
    terminal: Option<ApplicationEvent>,
    envelope: EventEnvelope,
    finished: bool,
}

impl TurnDelivery {
    pub(crate) fn new(session_id: String, sink: Arc<dyn ApplicationEventSink>) -> Self {
        Self {
            session_id,
            sink,
            state: Mutex::new(DeliveryState::default()),
        }
    }

    pub(crate) fn finish(&self, canceled: bool, error: Option<String>) {
        let event = {
            let mut state = self.state.lock_recover();
            if state.finished {
                return;
            }
            state.finished = true;
            let mut terminal = state.terminal.take();
            // Persistence can fail after finalization prepared a successful event.
            if let Some(ApplicationEvent::TurnCompleted {
                status,
                error: reported,
                ..
            }) = &mut terminal
            {
                if status == "ok" && (canceled || error.is_some()) {
                    *status = if canceled { "canceled" } else { "error" }.into();
                    *reported = error.clone();
                }
                if status != "ok" && reported.as_deref().is_none_or(|s| s.trim().is_empty()) {
                    *reported = error.clone();
                }
            }
            terminal.unwrap_or_else(|| {
                ApplicationEvent::turn_completed(
                    self.session_id.clone(),
                    if canceled {
                        "canceled"
                    } else if error.is_some() {
                        "error"
                    } else {
                        "ok"
                    }
                    .into(),
                    error,
                    &state.envelope,
                )
            })
        };
        self.sink.send(event);
    }
}

impl ApplicationEventSink for TurnDelivery {
    fn send(&self, event: ApplicationEvent) {
        {
            let mut state = self.state.lock_recover();
            if state.finished {
                return;
            }
            if matches!(&event, ApplicationEvent::TurnCompleted { session_id, .. } if session_id == &self.session_id)
            {
                state.terminal = Some(event);
                return;
            }
            if let ApplicationEvent::RunStatus {
                run_id,
                task_id,
                attempt_id,
                identity_id,
                ..
            } = &event
            {
                state.envelope = EventEnvelope {
                    run_id: Some(run_id.clone()),
                    task_id: task_id.clone(),
                    attempt_id: attempt_id.clone(),
                    identity_id: identity_id.clone(),
                };
            }
        }
        self.sink.send(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::RecordingEventSink;

    #[test]
    fn completion_preserves_missing_failure_reason() {
        let (sink, events) = RecordingEventSink::new();
        let delivery = TurnDelivery::new("session".into(), sink);
        delivery.send(ApplicationEvent::turn_completed(
            "session".into(),
            "error".into(),
            None,
            &EventEnvelope::default(),
        ));
        delivery.finish(false, Some("effort cap reached (8 steps)".into()));
        assert!(matches!(&events.lock().unwrap()[0],
            ApplicationEvent::TurnCompleted { error: Some(reason), .. }
                if reason == "effort cap reached (8 steps)"));
    }

    #[test]
    fn completion_is_buffered_and_published_once() {
        let (sink, events) = RecordingEventSink::new();
        let delivery = TurnDelivery::new("session".into(), sink);
        delivery.send(ApplicationEvent::turn_completed(
            "session".into(),
            "ok".into(),
            None,
            &EventEnvelope::default(),
        ));
        assert!(events.lock().unwrap().is_empty());
        delivery.finish(false, None);
        delivery.finish(false, Some("dropped".into()));
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], ApplicationEvent::TurnCompleted { status, .. } if status == "ok")
        );
    }

    #[test]
    fn early_failure_has_a_terminal_event_without_an_invented_run() {
        let (sink, events) = RecordingEventSink::new();
        let delivery = TurnDelivery::new("session".into(), sink);
        delivery.finish(false, Some("planning failed".into()));
        assert!(
            matches!(&events.lock().unwrap()[0], ApplicationEvent::TurnCompleted { status, error: Some(error), run_id: None, .. } if status == "error" && error == "planning failed")
        );
    }
}

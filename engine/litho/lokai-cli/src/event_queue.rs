//! Bounded synchronous bridge from application events to the terminal portal.
//! Overflow is sticky and observable by the consumer, even if a sender ignores
//! its error. The portal must stop and close the session instead of presenting
//! an incomplete event stream as successful.

use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use lokai_app::events::ApplicationEvent;

const MAX_EVENTS: usize = 4096;
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_EVENT_BYTES: usize = 4 * 1024 * 1024;
const OVERFLOW: &str =
    "TUI output exceeded its queue limit; session stopped to avoid losing output or approvals";
const POISONED: &str = "TUI event queue failed; session stopped";

#[derive(Default)]
struct State {
    events: VecDeque<(ApplicationEvent, usize)>,
    bytes: usize,
    failure: Option<&'static str>,
    closed: bool,
}

#[derive(Clone)]
pub struct Sender(Arc<Mutex<State>>);
pub struct Receiver(Arc<Mutex<State>>);

pub fn channel() -> (Sender, Receiver) {
    let state = Arc::new(Mutex::new(State::default()));
    (Sender(state.clone()), Receiver(state))
}

impl Sender {
    pub fn send(&self, event: ApplicationEvent) -> Result<(), &'static str> {
        // Count serialized payload without allocating a second payload buffer.
        // The count limit separately bounds per-event/container overhead.
        let mut counter = PayloadCounter(0);
        let oversized = serde_json::to_writer(&mut counter, &event).is_err();
        let mut state = self.0.lock().map_err(|_| POISONED)?;
        if let Some(error) = state.failure {
            return Err(error);
        }
        if state.closed {
            return Err("TUI event receiver closed");
        }
        if oversized || state.events.len() >= MAX_EVENTS || counter.0 > MAX_BYTES - state.bytes {
            state.failure = Some(OVERFLOW);
            state.events.clear();
            state.bytes = 0;
            return Err(OVERFLOW);
        }
        state.bytes += counter.0;
        state.events.push_back((event, counter.0));
        Ok(())
    }
}

impl Receiver {
    pub fn check_health(&self) -> anyhow::Result<()> {
        let state = self.0.lock().map_err(|_| anyhow::anyhow!(POISONED))?;
        if let Some(error) = state.failure {
            anyhow::bail!(error);
        }
        Ok(())
    }

    pub fn try_recv(&mut self) -> Option<ApplicationEvent> {
        let mut state = self.0.lock().ok()?;
        let (event, bytes) = state.events.pop_front()?;
        state.bytes -= bytes;
        Some(event)
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.lock() {
            state.closed = true;
            state.events.clear();
            state.bytes = 0;
        }
    }
}

struct PayloadCounter(usize);

impl Write for PayloadCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_EVENT_BYTES - self.0 {
            return Err(io::Error::other("event payload limit exceeded"));
        }
        self.0 += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(text: &str) -> ApplicationEvent {
        ApplicationEvent::InspectorUpdate { text: text.into() }
    }

    #[test]
    fn preserves_order_and_releases_payload_budget_on_consumption() {
        let (tx, mut rx) = channel();
        tx.send(output("first")).unwrap();
        tx.send(output("second")).unwrap();
        assert!(rx.0.lock().unwrap().bytes > 0);
        assert!(
            matches!(rx.try_recv(), Some(ApplicationEvent::InspectorUpdate { text }) if text == "first")
        );
        assert!(
            matches!(rx.try_recv(), Some(ApplicationEvent::InspectorUpdate { text }) if text == "second")
        );
        assert_eq!(rx.0.lock().unwrap().bytes, 0);
        rx.check_health().unwrap();
    }

    #[test]
    fn ignored_overflow_is_sticky_and_releases_queued_payloads() {
        let (tx, mut rx) = channel();
        for _ in 0..MAX_EVENTS {
            tx.send(ApplicationEvent::InspectorClear).unwrap();
        }
        let _ = tx.send(output("cannot silently lose this"));
        assert!(rx.check_health().is_err());
        assert!(rx.try_recv().is_none());
        assert_eq!(rx.0.lock().unwrap().bytes, 0);
        assert!(tx.send(ApplicationEvent::InspectorClear).is_err());
        assert!(rx.check_health().is_err());
    }

    #[test]
    fn payload_limits_apply_before_event_count_limit() {
        let (tx, rx) = channel();
        let text = "x".repeat(MAX_EVENT_BYTES / 2);
        for _ in 0..7 {
            tx.send(output(&text)).unwrap();
        }
        assert!(tx.send(output(&text)).is_err());
        assert!(rx.check_health().is_err());
        let (tx, rx) = channel();
        assert!(tx.send(output(&"x".repeat(MAX_EVENT_BYTES))).is_err());
        assert!(rx.check_health().is_err());
    }

    #[test]
    fn dropped_receiver_releases_queue_and_rejects_senders() {
        let (tx, rx) = channel();
        tx.send(output("queued")).unwrap();
        drop(rx);
        assert!(tx.0.lock().unwrap().events.is_empty());
        assert!(tx.send(output("late")).is_err());
    }

    #[test]
    fn concurrent_producers_cannot_exceed_count_limit() {
        let (tx, rx) = channel();
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let tx = tx.clone();
                scope.spawn(move || {
                    for _ in 0..MAX_EVENTS {
                        if tx.send(ApplicationEvent::InspectorClear).is_err() {
                            break;
                        }
                    }
                });
            }
        });
        assert!(rx.check_health().is_err());
        assert!(rx.0.lock().unwrap().events.len() <= MAX_EVENTS);
    }
}

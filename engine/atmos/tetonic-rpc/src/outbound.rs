//! Semantic outbound queue policy (AC2-10).
//!
//! Classifies RPC frames so a slow stdout reader cannot grow coordinator memory
//! without bound: tokens coalesce, progress replaces, debug drops when full.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use crate::protocol::events;

/// `Ok(Some(redacted))` replaces the frame. `Ok(None)` keeps the original.
/// `Err` drops the frame (never plaintext).
pub type OutboundRedactor = Arc<dyn Fn(&str) -> Result<Option<String>, String> + Send + Sync>;

/// Default bounded queue depth for daemon stdout.
pub const DEFAULT_OUTBOUND_CAPACITY: usize = 512;

/// How an outbound frame is treated when the writer queue is under pressure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboundClass {
    /// JSON-RPC responses and terminal run status — disconnect on overflow.
    Terminal,
    /// Approval prompts — disconnect on overflow.
    Critical,
    /// Streaming tokens — keep latest per `(session_id, agent_id)`.
    Coalesce,
    /// Capacity/progress — keep latest per session.
    Replace,
    /// Debug/context — drop when the queue is full.
    Lossy,
    /// Tool/diff/egress events — standard priority (disconnect on overflow).
    Standard,
}

pub fn classify_outbound(method: &str, is_response: bool) -> OutboundClass {
    if is_response {
        return OutboundClass::Terminal;
    }
    match method {
        events::RUN_STATUS => OutboundClass::Terminal,
        events::APPROVAL_REQUEST => OutboundClass::Critical,
        events::TOKEN => OutboundClass::Coalesce,
        events::CAPACITY_PROGRESS => OutboundClass::Replace,
        events::LOG | events::CONTEXT => OutboundClass::Lossy,
        _ => OutboundClass::Standard,
    }
}

type CoalesceKey = (String, String);

/// Bounded outbound queue with in-memory coalesce/replace slots.
#[derive(Clone)]
pub struct OutboundQueue {
    pending: Arc<Mutex<VecDeque<String>>>,
    wake_tx: mpsc::Sender<()>,
    capacity: usize,
    enqueue_gate: Arc<Mutex<()>>,
    failed: Arc<std::sync::atomic::AtomicBool>,
    pending_tokens: Arc<Mutex<HashMap<CoalesceKey, String>>>,
    pending_progress: Arc<Mutex<HashMap<String, String>>>,
    redactor: Option<OutboundRedactor>,
}

impl OutboundQueue {
    /// Returns `(queue, wake_rx)`. Pass `wake_rx` to [`crate::writer_task`].
    pub fn new(capacity: usize) -> (Self, mpsc::Receiver<()>) {
        let (wake_tx, wake_rx) = mpsc::channel(1);
        (
            Self {
                pending: Arc::new(Mutex::new(VecDeque::new())),
                wake_tx,
                capacity: capacity.max(1),
                enqueue_gate: Arc::new(Mutex::new(())),
                failed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                pending_tokens: Arc::new(Mutex::new(HashMap::new())),
                pending_progress: Arc::new(Mutex::new(HashMap::new())),
                redactor: None,
            },
            wake_rx,
        )
    }

    pub fn with_redactor(mut self, redactor: OutboundRedactor) -> Self {
        self.redactor = Some(redactor);
        self
    }

    pub fn drain_ready(&self) -> Vec<String> {
        let mut q = self.pending.lock().expect("outbound pending queue");
        q.drain(..).collect()
    }

    pub fn flush_pending(&self) {
        let tokens: Vec<String> = self
            .pending_tokens
            .lock()
            .expect("token coalesce map")
            .drain()
            .map(|(_, v)| v)
            .collect();
        for frame in tokens {
            self.push_frame(frame);
        }
        let progress: Vec<String> = self
            .pending_progress
            .lock()
            .expect("progress replace map")
            .drain()
            .map(|(_, v)| v)
            .collect();
        for frame in progress {
            self.push_frame(frame);
        }
    }

    pub fn enqueue(
        &self,
        class: OutboundClass,
        mut frame: String,
        coalesce_key: Option<CoalesceKey>,
        progress_session: Option<&str>,
    ) -> bool {
        if let Some(redactor) = &self.redactor {
            match redactor(&frame) {
                Ok(Some(redacted)) => frame = redacted,
                Ok(None) => {}
                Err(_) => return true,
            }
        }

        let _gate = self.enqueue_gate.lock().expect("enqueue gate");
        if self.is_failed() {
            return false;
        }
        let existing = match class {
            OutboundClass::Coalesce => coalesce_key
                .as_ref()
                .and_then(|k| self.pending_tokens.lock().unwrap().get(k).cloned()),
            OutboundClass::Replace => {
                progress_session.and_then(|k| self.pending_progress.lock().unwrap().get(k).cloned())
            }
            _ => None,
        };
        if class == OutboundClass::Coalesce {
            if let Some(old) = &existing {
                if let (Ok(previous), Ok(mut next)) = (
                    serde_json::from_str::<serde_json::Value>(old),
                    serde_json::from_str::<serde_json::Value>(&frame),
                ) {
                    if let (Some(a), Some(b)) = (
                        previous["params"]["delta"].as_str(),
                        next["params"]["delta"].as_str(),
                    ) {
                        next["params"]["delta"] = serde_json::Value::String(format!("{a}{b}"));
                        frame = next.to_string();
                    } else {
                        self.failed
                            .store(true, std::sync::atomic::Ordering::Release);
                        self.signal_wake();
                        return false;
                    }
                } else {
                    self.failed
                        .store(true, std::sync::atomic::Ordering::Release);
                    self.signal_wake();
                    return false;
                }
            }
        }
        let q = self.pending.lock().unwrap();
        let tokens = self.pending_tokens.lock().unwrap();
        let progress = self.pending_progress.lock().unwrap();
        let count = q.len() + tokens.len() + progress.len();
        let bytes: usize = q
            .iter()
            .chain(tokens.values())
            .chain(progress.values())
            .map(String::len)
            .sum();
        let overflow = count + usize::from(existing.is_none()) > self.capacity
            || bytes
                .saturating_sub(existing.as_ref().map_or(0, String::len))
                .saturating_add(frame.len())
                > 32 * 1024 * 1024;
        drop(progress);
        drop(tokens);
        drop(q);
        if overflow {
            if class != OutboundClass::Lossy {
                self.failed
                    .store(true, std::sync::atomic::Ordering::Release);
                self.signal_wake();
            }
            return false;
        }

        match class {
            OutboundClass::Coalesce => {
                if let Some(key) = coalesce_key {
                    self.pending_tokens
                        .lock()
                        .expect("token coalesce map")
                        .insert(key, frame);
                    self.signal_wake();
                    return true;
                }
                self.push_frame(frame)
            }
            OutboundClass::Replace => {
                if let Some(sid) = progress_session {
                    self.pending_progress
                        .lock()
                        .expect("progress replace map")
                        .insert(sid.to_string(), frame);
                    self.signal_wake();
                    return true;
                }
                self.push_frame(frame)
            }
            OutboundClass::Lossy => self.try_push_frame(frame),
            OutboundClass::Terminal | OutboundClass::Critical | OutboundClass::Standard => {
                self.flush_pending();
                self.push_frame(frame)
            }
        }
    }

    fn push_frame(&self, frame: String) -> bool {
        let mut q = self.pending.lock().expect("outbound pending queue");
        q.push_back(frame);
        self.signal_wake();
        true
    }

    fn try_push_frame(&self, frame: String) -> bool {
        let mut q = self.pending.lock().expect("outbound pending queue");
        if q.len() >= self.capacity {
            return false;
        }
        q.push_back(frame);
        self.signal_wake();
        true
    }

    fn signal_wake(&self) {
        let _ = self.wake_tx.try_send(());
    }

    pub fn is_failed(&self) -> bool {
        self.failed.load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn pending_token_count(&self) -> usize {
        self.pending_tokens
            .lock()
            .expect("token coalesce map")
            .len()
    }

    pub fn pending_progress_count(&self) -> usize {
        self.pending_progress
            .lock()
            .expect("progress replace map")
            .len()
    }

    pub fn queued_count(&self) -> usize {
        self.pending.lock().expect("outbound pending queue").len()
    }

    /// Move coalesced token frames into the pending queue for the writer task.
    pub fn promote_coalesce_frames(&self) {
        let tokens: Vec<String> = self
            .pending_tokens
            .lock()
            .expect("token coalesce map")
            .drain()
            .map(|(_, v)| v)
            .collect();
        for frame in tokens {
            self.push_frame(frame);
        }
    }

    /// Move replaced progress frames into the pending queue for the writer task.
    pub fn promote_progress_frames(&self) {
        let progress: Vec<String> = self
            .pending_progress
            .lock()
            .expect("progress replace map")
            .drain()
            .map(|(_, v)| v)
            .collect();
        for frame in progress {
            self.push_frame(frame);
        }
    }

    /// Promote side slots then drain the pending queue (used by the stdout writer).
    pub fn drain_for_write(&self) -> Vec<String> {
        let _gate = self.enqueue_gate.lock().expect("enqueue gate");
        self.promote_coalesce_frames();
        self.promote_progress_frames();
        self.drain_ready()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn token_frame(session: &str, agent: &str, delta: &str) -> String {
        json!({
            "jsonrpc": "2.0",
            "method": events::TOKEN,
            "params": {
                "session_id": session,
                "agent_id": agent,
                "seq": 1,
                "delta": delta,
                "role": "assistant"
            }
        })
        .to_string()
    }

    #[test]
    fn classify_outbound_maps_events() {
        assert_eq!(
            classify_outbound(events::TOKEN, false),
            OutboundClass::Coalesce
        );
        assert_eq!(
            classify_outbound(events::CAPACITY_PROGRESS, false),
            OutboundClass::Replace
        );
        assert_eq!(classify_outbound(events::LOG, false), OutboundClass::Lossy);
        assert_eq!(
            classify_outbound("chat/send", true),
            OutboundClass::Terminal
        );
    }

    #[test]
    fn token_coalesce_keeps_one_pending_per_session_agent() {
        let (q, _wake) = OutboundQueue::new(8);
        let key = ("s1".into(), "a0".into());
        q.enqueue(
            OutboundClass::Coalesce,
            token_frame("s1", "a0", "a"),
            Some(key.clone()),
            None,
        );
        q.enqueue(
            OutboundClass::Coalesce,
            token_frame("s1", "a0", "b"),
            Some(key),
            None,
        );
        assert_eq!(q.pending_token_count(), 1);
    }

    #[test]
    fn lossy_drops_when_queue_full() {
        let (q, _wake) = OutboundQueue::new(1);
        let frame = json!({"jsonrpc":"2.0","method":events::LOG,"params":{}}).to_string();
        assert!(q.enqueue(OutboundClass::Lossy, frame.clone(), None, None));
        assert!(!q.enqueue(OutboundClass::Lossy, frame, None, None));
    }

    #[test]
    fn terminal_flush_drains_coalesced_tokens() {
        let (q, _wake) = OutboundQueue::new(8);
        let key = ("s1".into(), "a0".into());
        q.enqueue(
            OutboundClass::Coalesce,
            token_frame("s1", "a0", "only"),
            Some(key),
            None,
        );
        assert_eq!(q.pending_token_count(), 1);
        q.enqueue(
            OutboundClass::Terminal,
            json!({"jsonrpc":"2.0","id":1,"result":{}}).to_string(),
            None,
            None,
        );
        assert_eq!(q.pending_token_count(), 0);
        let drained = q.drain_ready();
        assert_eq!(drained.len(), 2);
        assert!(drained[0].contains("only"));
    }

    #[test]
    fn flood_tokens_stays_bounded_in_coalesce_slots() {
        let (q, _wake) = OutboundQueue::new(8);
        let key = ("s1".into(), "a0".into());
        for i in 0..10_000 {
            q.enqueue(
                OutboundClass::Coalesce,
                token_frame("s1", "a0", &format!("t{i}")),
                Some(key.clone()),
                None,
            );
        }
        assert_eq!(q.pending_token_count(), 1);
        assert_eq!(q.queued_count(), 0);
    }

    #[test]
    fn queue_redactor_err_does_not_emit_original_frame() {
        let secret = "AKIAIOSFODNN7EXAMPLE";
        let (q, _wake) = OutboundQueue::new(8);
        let q = q.with_redactor(Arc::new(move |text| {
            if text.contains(secret) {
                Err("scan failed".into())
            } else {
                Ok(None)
            }
        }));
        let frame = format!(r#"{{"method":"event/log","params":{{"message":"{secret}"}}}}"#);
        assert!(q.enqueue(OutboundClass::Standard, frame, None, None));
        let drained = q.drain_ready();
        assert!(
            drained.iter().all(|f| !f.contains(secret)),
            "Err redactor must not emit original frame: {drained:?}"
        );
        assert!(drained.is_empty(), "Err drops the frame");
    }

    #[test]
    fn queue_redactor_ok_none_keeps_frame() {
        let (q, _wake) = OutboundQueue::new(8);
        let q = q.with_redactor(Arc::new(|_text| Ok(None)));
        let frame = r#"{"method":"event/log","params":{"message":"ok"}}"#.to_string();
        assert!(q.enqueue(OutboundClass::Standard, frame.clone(), None, None));
        let drained = q.drain_ready();
        assert_eq!(drained, vec![frame]);
    }
}

//! Server-side transport helpers: a single stdout writer task fed by an
//! [`OutboundQueue`], and a cloneable [`Notifier`] that stamps every
//! notification with the emitting `(session_id, agent_id)` and a per-session
//! monotonic `seq`.
//!
//! The actual request/dispatch loop lives in the daemon (`tetonicd`) because it
//! needs to spawn per-run tasks on a single-threaded `LocalSet` — keeping that
//! out of this crate lets `lokai-rpc` stay engine-free and trivially testable.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::framing;
use crate::outbound::{classify_outbound, OutboundClass, OutboundQueue, DEFAULT_OUTBOUND_CAPACITY};

/// Drains serialized JSON frames from `queue` and writes them to `w` (typically
/// the daemon's stdout). Runs until the wake channel closes or a write fails.
pub async fn writer_task<W: AsyncWrite + Unpin>(
    queue: OutboundQueue,
    mut wake_rx: mpsc::Receiver<()>,
    mut w: W,
) {
    loop {
        if queue.is_failed() {
            return;
        }
        let mut wrote = false;
        for s in queue.drain_for_write() {
            wrote = true;
            if !matches!(
                tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    framing::write_frame(&mut w, s.as_bytes())
                )
                .await,
                Ok(Ok(()))
            ) {
                return;
            }
        }
        if wrote
            && !matches!(
                tokio::time::timeout(std::time::Duration::from_secs(5), w.flush()).await,
                Ok(Ok(()))
            )
        {
            return;
        }
        if wake_rx.recv().await.is_none() {
            return;
        }
    }
}

/// Spawns a background task that forwards the bounded outbound queue to an
/// unbounded channel. Used by integration tests that assert on emitted frames.
pub fn channel_pair(capacity: usize) -> (Notifier, mpsc::UnboundedReceiver<String>) {
    let (queue, mut wake_rx) = OutboundQueue::new(capacity);
    let (tx, rx) = mpsc::unbounded_channel();
    let notifier = Notifier::new(queue.clone());
    tokio::spawn(async move {
        loop {
            for s in queue.drain_for_write() {
                if tx.send(s).is_err() {
                    return;
                }
            }
            if wake_rx.recv().await.is_none() {
                return;
            }
        }
    });
    (notifier, rx)
}

/// Emits JSON-RPC responses and notifications onto the shared outbound queue.
/// Cloneable and cheap to pass into every run/audit closure.
#[derive(Clone)]
pub struct Notifier {
    queue: OutboundQueue,
    /// Per-session monotonic notification counter so the editor can order/replay.
    seqs: Arc<Mutex<HashMap<String, u64>>>,
}

impl Notifier {
    pub fn new(queue: OutboundQueue) -> Self {
        Self {
            queue,
            seqs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_default_queue() -> Self {
        let (queue, _wake_rx) = OutboundQueue::new(DEFAULT_OUTBOUND_CAPACITY);
        Self::new(queue)
    }

    pub fn queue(&self) -> OutboundQueue {
        self.queue.clone()
    }

    /// Send a completed JSON-RPC response (result or error envelope).
    pub fn respond(&self, response: &crate::protocol::Response) {
        let Ok(s) = serde_json::to_string_pretty(response) else {
            return;
        };
        let _ = self.queue.enqueue(OutboundClass::Terminal, s, None, None);
    }

    /// Emit a notification for `(session_id, agent_id)`. The envelope injects
    /// `session_id`, `agent_id`, and the next per-session `seq` into `params`
    /// (which must be a JSON object). `agent_id` is `"a0"` for the root agent;
    /// the field exists from v1 so sub-agents (Phase C) need no protocol bump.
    pub fn notify(&self, session_id: &str, agent_id: &str, method: &str, params: Value) -> bool {
        tetonic_telemetry::fault::inject_fault("during_notification_delivery");
        let seq = self.next_seq(session_id);
        let mut params = params;
        let Value::Object(map) = &mut params else {
            return false;
        };
        map.insert("session_id".into(), json!(session_id));
        map.insert("agent_id".into(), json!(agent_id));
        map.insert("seq".into(), json!(seq));
        let note = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        let class = classify_outbound(method, false);
        let coalesce_key = if class == OutboundClass::Coalesce {
            Some((session_id.to_string(), agent_id.to_string()))
        } else {
            None
        };
        let progress_session = if class == OutboundClass::Replace {
            Some(session_id)
        } else {
            None
        };
        let payload = serde_json::to_string_pretty(&note).unwrap_or_else(|_| note.to_string());
        self.queue
            .enqueue(class, payload, coalesce_key, progress_session)
    }

    fn next_seq(&self, session_id: &str) -> u64 {
        let mut seqs = self.seqs.lock().expect("notifier seq map poisoned");
        let e = seqs.entry(session_id.to_string()).or_insert(0);
        *e += 1;
        *e
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::events;

    #[tokio::test]
    async fn notify_injects_envelope_and_increments_seq() {
        let (n, mut rx) = channel_pair(8);
        n.notify(
            "s1",
            "a0",
            events::TOKEN,
            json!({ "delta": "hi", "role": "assistant" }),
        );
        n.notify("s1", "a0", events::RUN_STATUS, json!({ "status": "ok" }));

        let first: Value = serde_json::from_str(&rx.recv().await.unwrap()).unwrap();
        assert_eq!(first["method"], events::TOKEN);
        assert_eq!(first["params"]["session_id"], "s1");
        assert_eq!(first["params"]["agent_id"], "a0");
        assert_eq!(first["params"]["seq"], 1);
        assert_eq!(first["params"]["delta"], "hi");

        let second: Value = serde_json::from_str(&rx.recv().await.unwrap()).unwrap();
        assert_eq!(second["params"]["seq"], 2);
    }

    #[tokio::test]
    async fn seq_is_per_session() {
        let (n, mut rx) = channel_pair(8);
        n.notify("s1", "a0", events::TOKEN, json!({}));
        n.notify("s2", "a0", events::TOKEN, json!({}));
        let a: Value = serde_json::from_str(&rx.recv().await.unwrap()).unwrap();
        let b: Value = serde_json::from_str(&rx.recv().await.unwrap()).unwrap();
        assert_eq!(a["params"]["seq"], 1);
        assert_eq!(b["params"]["seq"], 1);
    }

    #[tokio::test]
    async fn notify_rejects_non_object_params() {
        let n = Notifier::with_default_queue();
        assert!(!n.notify("s1", "a0", events::TOKEN, json!("bad")));
    }

    #[test]
    fn respond_enqueues_terminal_frame() {
        let (queue, _wake) = OutboundQueue::new(8);
        let notifier = Notifier::new(queue.clone());
        notifier.respond(&crate::protocol::Response::ok(
            json!(1),
            json!({"ok": true}),
        ));
        assert_eq!(queue.drain_for_write().len(), 1);
    }
}

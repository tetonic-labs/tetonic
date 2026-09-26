//! Multi-turn conversation state (message history, compaction prefix, turn id).

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tetonic_inference::Message;

/// The evolving state of one chat session: the running message history, how much
/// of the front is protected from trimming (system prompt, +running summary after
/// compaction), and a per-session nonce + counter for unique tool-call ids.
///
/// Hold one of these across multiple [`crate::Agent::turn`] calls to get a multi-turn
/// conversation that remembers prior turns. A fresh `Conversation` is a new chat.
pub struct Conversation {
    pub(crate) messages: Vec<Message>,
    pub(crate) prefix_len: usize,
    pub(crate) nonce: u128,
    pub(crate) call_no: usize,
    /// Cooperative cancellation flag. The host flips it (via [`Conversation::cancel_handle`])
    /// to ask an in-flight [`crate::Agent::turn`] to stop cleanly between steps.
    cancel: Arc<AtomicBool>,
    /// Workspace-relative paths already retrieved in full this session.
    /// Invalidated on edit/write; cleared after an unknown tree mutation.
    pub(crate) retrieved_paths: HashSet<String>,
    /// Stable id for the current user turn (`chat/send`); drives fabric turn affinity.
    turn_id: Option<String>,
}

impl Conversation {
    /// Preserve history while discarding tokenizer-specific accounting.
    pub fn invalidate_token_counts(&self) {
        for message in &self.messages {
            message.invalidate_token_count();
        }
    }
    /// Resume an audit transcript into a multi-turn conversation (AR1-1).
    /// Compaction state (`prefix_len` beyond the first system message) is not restored.
    pub fn from_audit_messages(messages: Vec<Message>) -> Self {
        let prefix_len = if messages.first().is_some_and(|m| m.role == "system") {
            1
        } else {
            0
        };
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        Self {
            messages,
            prefix_len,
            nonce,
            call_no: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            retrieved_paths: HashSet::new(),
            turn_id: None,
        }
    }

    pub fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        Self {
            messages: Vec::new(),
            prefix_len: 1,
            nonce,
            call_no: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            retrieved_paths: HashSet::new(),
            turn_id: None,
        }
    }

    /// Message count checkpoint for folding a spawn branch (A13 v5).
    pub fn checkpoint(&self) -> usize {
        self.messages.len()
    }

    /// Drop turns carried from another execution. The cancel handle stays so a
    /// stop requested on this conversation still reaches the attempt.
    pub fn discard_carried_turns(&mut self) {
        self.messages.clear();
        self.prefix_len = 1;
        self.call_no = 0;
        self.retrieved_paths.clear();
        self.turn_id = None;
    }

    /// Drop messages appended after `checkpoint` (child transcript removal).
    pub fn rollback_to(&mut self, checkpoint: usize) {
        if checkpoint <= self.messages.len() {
            self.messages.truncate(checkpoint);
        }
    }

    /// Start a new user turn (resets fabric turn-affinity on the coordinator).
    pub fn begin_turn(&mut self) {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        self.turn_id = Some(format!("turn_{nonce}"));
    }

    /// Current turn id when a user turn is in flight.
    pub fn turn_id(&self) -> Option<&str> {
        self.turn_id.as_deref()
    }

    /// Number of user/assistant/tool messages recorded so far.
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// A handle the host can flip to request cooperative cancellation. The loop
    /// checks it between steps and before each tool call, then stops with a
    /// `Stopped("canceled")` step. Cheap to clone (`Arc`); safe to set from any
    /// thread.
    pub fn cancel_handle(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    pub(crate) fn is_canceled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

impl Default for Conversation {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::*;

    #[test]
    fn discard_carried_turns_drops_messages_and_keeps_cancel() {
        let mut conversation = Conversation::from_audit_messages(vec![tetonic_inference::Message::user(
            "PRIVATECANARY prior turn",
        )]);
        conversation.begin_turn();
        let cancel = conversation.cancel_handle();
        cancel.store(true, Ordering::SeqCst);
        conversation.discard_carried_turns();
        assert!(conversation.is_empty());
        assert!(conversation.is_canceled());
        assert!(conversation.turn_id().is_none());
        assert!(cancel.load(Ordering::SeqCst));
    }
}

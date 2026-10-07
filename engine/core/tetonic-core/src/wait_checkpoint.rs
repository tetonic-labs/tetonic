//! Portable continuation of a single turn at an asynchronous host tool boundary.
//! This is execution state, not long-term agent memory or an audit transcript.
use crate::{Conversation, SpawnRequest};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaitCheckpoint {
    pub(crate) version: u32,
    pub(crate) harness: serde_json::Value,
    pub invocation: tetonic_domain::AgentInvocation,
    pub pending: SpawnRequest,
    #[serde(with = "tetonic_inference::checkpoint")]
    pub(crate) messages: Vec<tetonic_inference::Message>,
    pub(crate) prefix_len: usize,
    pub(crate) nonce: u128,
    pub(crate) call_no: usize,
    pub(crate) retrieved_paths: Vec<String>,
    pub(crate) turn_id: Option<String>,
    pub(crate) steps_used: u32,
    pub(crate) step_index: u32,
    pub(crate) reported_tokens: u64,
    pub(crate) monitor: crate::monitor::HeuristicMonitor,
}

impl WaitCheckpoint {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.version != 1
            || self.messages.is_empty()
            || self.prefix_len > self.messages.len()
            || self.pending.attempt_id.as_deref().is_none_or(str::is_empty)
            || self.pending.call_id.is_empty()
            || self.pending.tool_name.is_empty()
            || self.steps_used == 0
            || self.steps_used as usize > self.invocation.max_steps
            || self.invocation.instructions.is_empty()
        {
            return Err("invalid turn checkpoint");
        }
        let last = self.messages.last().unwrap();
        let calls = last.tool_calls.as_ref().ok_or("missing pending call")?;
        let provider_ids = last.provider_tool_call_ids();
        let expected_id = provider_ids
            .first()
            .cloned()
            .unwrap_or_else(|| format!("tc_{:x}_{}", self.nonce, self.call_no.saturating_sub(1)));
        if last.role != "assistant"
            || calls.len() != 1
            || self.call_no == 0
            || self.pending.call_id != expected_id
            || calls[0].function.name != self.pending.tool_name
            || calls[0].function.arguments != self.pending.arguments
        {
            return Err("checkpoint does not end at its pending host call");
        }
        Ok(())
    }

    /// Refuse a rebuilt harness with different model settings, limits or tool schemas.
    /// Provider credentials and permission grants still require current host checks.
    pub fn matches_agent(&self, agent: &crate::Agent) -> bool {
        self.harness == agent.wait_harness_binding()
    }

    /// Only a verified checkpoint bound to the same managed invocation may enter here.
    pub fn into_conversation(self) -> Result<Conversation, &'static str> {
        self.validate()?;
        let mut conversation = Conversation::from_audit_messages(self.messages.clone());
        conversation.prefix_len = self.prefix_len;
        conversation.nonce = self.nonce;
        conversation.call_no = self.call_no;
        conversation.retrieved_paths = self.retrieved_paths.iter().cloned().collect();
        conversation.restore_turn_id(self.turn_id.clone());
        conversation.pending_resume = Some(self);
        Ok(conversation)
    }
}

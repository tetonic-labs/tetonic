//! Portable continuation of a single turn at an asynchronous host tool boundary.
//! This is execution state, not long-term agent memory or an audit transcript.
use crate::{Conversation, SpawnRequest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[cfg(test)]
#[path = "wait_checkpoint_tests.rs"]
mod tests;

/// A response actually returned through the host hook in this invocation.
/// Digests avoid duplicating private tool output, and survive context compaction.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceivedHostCall {
    pub request: SpawnRequest,
    pub response_digest: String,
}

impl ReceivedHostCall {
    pub(crate) fn new(request: SpawnRequest, response: &str) -> Self {
        Self {
            request,
            response_digest: format!("{:x}", Sha256::digest(response.as_bytes())),
        }
    }

    pub fn matches_response(&self, response: &str) -> bool {
        self.response_digest == format!("{:x}", Sha256::digest(response.as_bytes()))
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaitCheckpoint {
    pub(crate) version: u32,
    pub(crate) harness: serde_json::Value,
    pub invocation: tetonic_domain::AgentInvocation,
    pub pending: SpawnRequest,
    #[serde(default)]
    pub(crate) received_host_calls: Vec<ReceivedHostCall>,
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
    /// Scan content at the private execution-state boundary, retaining all local
    /// instructions, tool arguments/results and schemas. Correlation IDs and
    /// provider ciphertext remain in the sealed bytes, but are not user secrets.
    pub fn disclosure_scan_text(&self) -> Result<String, &'static str> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| "checkpoint encoding failed")?;
        let mut value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| "checkpoint decoding failed")?;
        value["messages"] = tetonic_inference::checkpoint::disclosure_scan_messages(&self.messages);
        value["pending"]["call_id"] = serde_json::json!("correlation");
        if let Some(calls) = value["received_host_calls"].as_array_mut() {
            for call in calls {
                call["request"]["call_id"] = serde_json::json!("correlation");
            }
        }
        serde_json::to_string(&value).map_err(|_| "checkpoint scan encoding failed")
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if !matches!(self.version, 1 | 2)
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
        if (self.version == 1 && !self.received_host_calls.is_empty())
            || self.received_host_calls.len() >= self.steps_used as usize
        {
            return Err("invalid received host calls");
        }
        let mut ids = std::collections::HashSet::new();
        for call in &self.received_host_calls {
            if call.request.call_id.is_empty()
                || call.request.call_id == self.pending.call_id
                || !ids.insert(&call.request.call_id)
                || call.request.attempt_id != self.pending.attempt_id
                || call.request.parent_agent_id != self.pending.parent_agent_id
                || ![
                    self.invocation.discipline.spawn_tool.as_deref(),
                    self.invocation.discipline.handoff_tool.as_deref(),
                ]
                .contains(&Some(call.request.tool_name.as_str()))
                || call.response_digest.len() != 64
                || !call
                    .response_digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err("invalid received host call binding");
            }
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

    /// Version-one checkpoints did not retain delivery state across compaction.
    /// They remain readable for root human waits, but cannot reconstruct a coordinator.
    pub fn received_host_calls(&self) -> Result<&[ReceivedHostCall], &'static str> {
        self.validate()?;
        if self.version != 2 {
            return Err("checkpoint has no host delivery history");
        }
        Ok(&self.received_host_calls)
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
        conversation.received_host_calls = self.received_host_calls.clone();
        conversation.pending_resume = Some(self);
        Ok(conversation)
    }
}

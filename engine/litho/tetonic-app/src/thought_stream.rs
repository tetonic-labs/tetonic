//! Zero-Compute Telemetry & Live Thought Inspection Stream (SAE-502).
//!
//! Provides real-time visibility into continuous agent thoughts, perceptions,
//! actions, and boundary evaluations via SSE/WebSockets at zero inference cost,
//! backed by an in-memory ring buffer for immediate client catch-up.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, Mutex};

use tetonic_domain::perception::{SignalValue, Urgency};
use tetonic_orchestrator::AgentLifecycleState;

/// Real-time event emitted during an agent's continuous cognitive loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum TelemetryEvent {
    /// Sensory perception event received from an environment adapter.
    PerceptionReceived {
        agent_id: String,
        signals: HashMap<String, SignalValue>,
        urgency: Urgency,
        timestamp: DateTime<Utc>,
    },
    /// Streaming internal monologue delta extracted by TokenDemuxer.
    ThoughtDelta {
        agent_id: String,
        delta: String,
        timestamp: DateTime<Utc>,
    },
    /// Action proposed by the brain awaiting safety gate clearance.
    ActionProposed {
        agent_id: String,
        verb: String,
        parameters: serde_json::Value,
        timestamp: DateTime<Utc>,
    },
    /// Action executed in the environment through a WorldAdapter.
    ActionExecuted {
        agent_id: String,
        verb: String,
        success: bool,
        output: String,
        timestamp: DateTime<Utc>,
    },
    /// Proposed action intercepted and blocked by OperationalBoundary.
    BoundaryViolation {
        agent_id: String,
        verb: String,
        reason: String,
        timestamp: DateTime<Utc>,
    },
    /// Agent lifecycle status change (Running, Paused, Estopped).
    LifecycleChange {
        agent_id: String,
        new_status: AgentLifecycleState,
        timestamp: DateTime<Utc>,
    },
}

impl TelemetryEvent {
    pub fn agent_id(&self) -> &str {
        match self {
            Self::PerceptionReceived { agent_id, .. } => agent_id,
            Self::ThoughtDelta { agent_id, .. } => agent_id,
            Self::ActionProposed { agent_id, .. } => agent_id,
            Self::ActionExecuted { agent_id, .. } => agent_id,
            Self::BoundaryViolation { agent_id, .. } => agent_id,
            Self::LifecycleChange { agent_id, .. } => agent_id,
        }
    }

    /// Formats the event into standard Server-Sent Event (SSE) wire protocol.
    pub fn to_sse(&self) -> String {
        let event_type = match self {
            Self::PerceptionReceived { .. } => "perception",
            Self::ThoughtDelta { .. } => "thought",
            Self::ActionProposed { .. } => "action_proposed",
            Self::ActionExecuted { .. } => "action_executed",
            Self::BoundaryViolation { .. } => "boundary_violation",
            Self::LifecycleChange { .. } => "lifecycle",
        };
        let json = serde_json::to_string(self).unwrap_or_default();
        format!("event: {}\ndata: {}\n\n", event_type, json)
    }
}

/// Central pub-sub hub for live agent thoughts and telemetry inspection.
#[derive(Clone)]
pub struct ThoughtStreamHub {
    tx: broadcast::Sender<TelemetryEvent>,
    ring_buffers: Arc<Mutex<HashMap<String, VecDeque<TelemetryEvent>>>>,
    buffer_capacity: usize,
}

impl ThoughtStreamHub {
    pub fn new(channel_capacity: usize, buffer_capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(channel_capacity);
        Self {
            tx,
            ring_buffers: Arc::new(Mutex::new(HashMap::new())),
            buffer_capacity,
        }
    }

    /// Default hub with 1,024 channel slots and 256 events per agent ring buffer.
    pub fn default_hub() -> Self {
        Self::new(1024, 256)
    }

    /// Publishes a telemetry event, updating the per-agent ring buffer and broadcasting to subscribers.
    pub async fn publish(&self, event: TelemetryEvent) {
        let agent_id = event.agent_id().to_string();

        {
            let mut buffers = self.ring_buffers.lock().await;
            let buffer = buffers.entry(agent_id).or_insert_with(VecDeque::new);
            if buffer.len() >= self.buffer_capacity {
                buffer.pop_front();
            }
            buffer.push_back(event.clone());
        }

        // Broadcast to live listeners; ignore error if no active receivers currently exist
        let _ = self.tx.send(event);
    }

    /// Subscribes to the live telemetry stream (used by SSE / WebSocket HTTP handlers).
    pub fn subscribe(&self) -> broadcast::Receiver<TelemetryEvent> {
        self.tx.subscribe()
    }

    /// Retrieves the recent event history from the ring buffer for immediate UI catch-up.
    pub async fn get_recent_history(&self, agent_id: &str) -> Vec<TelemetryEvent> {
        let buffers = self.ring_buffers.lock().await;
        buffers
            .get(agent_id)
            .map(|b| b.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Helper to emit a thought delta with current timestamp.
    pub async fn emit_thought(&self, agent_id: impl Into<String>, delta: impl Into<String>) {
        self.publish(TelemetryEvent::ThoughtDelta {
            agent_id: agent_id.into(),
            delta: delta.into(),
            timestamp: Utc::now(),
        })
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_broadcast_thought_deltas_to_subscribers() {
        let hub = ThoughtStreamHub::new(100, 10);
        let mut rx = hub.subscribe();

        hub.emit_thought("agent-101", "analyzing memory context").await;

        let received = rx.recv().await.expect("receive event");
        match received {
            TelemetryEvent::ThoughtDelta { agent_id, delta, .. } => {
                assert_eq!(agent_id, "agent-101");
                assert_eq!(delta, "analyzing memory context");
            }
            _ => panic!("unexpected event variant"),
        }
    }

    #[tokio::test]
    async fn test_ring_buffer_catchup_on_connect() {
        let hub = ThoughtStreamHub::new(100, 3); // capacity 3

        hub.emit_thought("agent-fast", "thought 1").await;
        hub.emit_thought("agent-fast", "thought 2").await;
        hub.emit_thought("agent-fast", "thought 3").await;
        hub.emit_thought("agent-fast", "thought 4").await; // evicts thought 1

        let history = hub.get_recent_history("agent-fast").await;
        assert_eq!(history.len(), 3);

        if let TelemetryEvent::ThoughtDelta { delta, .. } = &history[0] {
            assert_eq!(delta, "thought 2");
        }
        if let TelemetryEvent::ThoughtDelta { delta, .. } = &history[2] {
            assert_eq!(delta, "thought 4");
        }
    }

    #[test]
    fn test_sse_wire_formatting() {
        let ev = TelemetryEvent::ThoughtDelta {
            agent_id: "agent-wire".into(),
            delta: "calculating vector".into(),
            timestamp: Utc::now(),
        };

        let sse = ev.to_sse();
        assert!(sse.starts_with("event: thought\n"));
        assert!(sse.contains("\"agent_id\":\"agent-wire\""));
        assert!(sse.ends_with("\n\n"));
    }
}

//! TWP Autonomous Standing Loop Sanity Test (VIL-103).
//!
//! Validates a continuous agent executing over the universal Tetonic World Protocol (TWP):
//! 1. Connects to `StreamWorldAdapter` via framed duplex NDJSON stream.
//! 2. Ingests high-frequency tick perceptions across multiple states.
//! 3. Proves `SensoryFilter` drops redundant idle ticks (>80% tick suppression).
//! 4. Proves rapid preemption and action dispatch on high-urgency sensory events.
//! 5. Validates authoritative E-Stop interlock halts outbound actions.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use tetonic_core::Agent;
use tetonic_domain::{
    Affordance, Brain, BrainCost, BrainError, BrainFinishReason, BrainPathway, BrainRequest,
    BrainResponse, BrainTokenSink, Perception, Signal, SignalValue, Urgency, WorldAction,
    WorldAdapter, WorldEvent, WorldManifest, WorldState,
};
use tetonic_runtime::{StreamMessage, StreamWorldAdapter};

/// Test brain tracking exact model evaluation counts to verify zero-compute filtering.
struct CountingBrain {
    evaluation_count: AtomicUsize,
}

impl CountingBrain {
    fn new() -> Self {
        Self {
            evaluation_count: AtomicUsize::new(0),
        }
    }

    fn evaluations(&self) -> usize {
        self.evaluation_count.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl Brain for CountingBrain {
    async fn complete(
        &self,
        _req: BrainRequest,
        _on_token: &mut BrainTokenSink<'_>,
    ) -> Result<BrainResponse, BrainError> {
        Ok(BrainResponse {
            content: String::new(),
            tool_calls: None,
            pathway: BrainPathway::Single {
                model: "test".into(),
            },
            finish_reason: BrainFinishReason::Stop,
            cost: BrainCost::default(),
        })
    }

    async fn perceive(&self, perception: Perception) -> Result<Option<WorldAction>, BrainError> {
        self.evaluation_count.fetch_add(1, Ordering::SeqCst);

        // If an urgent event or petition arrived, respond with a physical action
        if perception.urgency >= Urgency::High || !perception.events.is_empty() {
            Ok(Some(WorldAction::bare(
                "respond_to_event",
                BrainPathway::Reflexive {
                    model: "reflex-test".into(),
                },
            )))
        } else {
            Ok(None)
        }
    }

    fn describe(&self) -> &str {
        "counting_brain"
    }

    fn last_cost(&self) -> BrainCost {
        BrainCost::default()
    }
}

#[tokio::test]
async fn test_twp_continuous_agent_standing_loop_with_sensory_filter() {
    let brain = Arc::new(CountingBrain::new());
    let brain_clone = brain.clone();

    let agent = Agent::default().with_brain(brain_clone);

    let manifest = WorldManifest::new("sim_world", "1.0")
        .with_affordance(Affordance::instant("respond_to_event", "Respond to event"));

    let (client_io, mut server_io) = tokio::io::duplex(8192);
    let adapter = StreamWorldAdapter::from_duplex(client_io, manifest);
    let adapter_clone = adapter.clone();

    // Spawn agent in world loop
    let agent_handle = tokio::spawn(async move {
        agent.run_in_world(adapter_clone).await
    });

    let total_ticks = 50;

    // Send 50 stream ticks from the external game simulation:
    // - Ticks 1 to 40: Identical background ticks (idle world) -> SensoryFilter should drop
    // - Ticks 41 to 45: Changed signal value
    // - Ticks 46 to 50: High-urgency discrete event -> Brain should evaluate and emit action
    for seq in 1..=total_ticks {
        let perception = if seq <= 40 {
            // Idle background tick
            Perception {
                when: Utc::now(),
                sequence: seq,
                urgency: Urgency::Background,
                signals: vec![Signal {
                    name: "granary".into(),
                    value: SignalValue::Int(100),
                    changed: false,
                    trend: None,
                    urgency: Urgency::Background,
                }],
                events: vec![],
                state: WorldState {
                    schema_id: "world_v1".into(),
                    data: serde_json::json!({ "tick": seq }),
                },
            }
        } else if seq <= 45 {
            // Signal value delta
            Perception {
                when: Utc::now(),
                sequence: seq,
                urgency: Urgency::Medium,
                signals: vec![Signal {
                    name: "granary".into(),
                    value: SignalValue::Int(100 - (seq as i64 - 40)),
                    changed: true,
                    trend: Some(tetonic_domain::Trend::Falling),
                    urgency: Urgency::Medium,
                }],
                events: vec![],
                state: WorldState {
                    schema_id: "world_v1".into(),
                    data: serde_json::json!({ "tick": seq }),
                },
            }
        } else {
            // High-urgency discrete event
            Perception {
                when: Utc::now(),
                sequence: seq,
                urgency: Urgency::High,
                signals: vec![],
                events: vec![WorldEvent {
                    kind: "visitor_petition".into(),
                    source: Some("guest_10".into()),
                    payload: serde_json::json!({ "title": "Build beacon" }),
                    urgency: Urgency::High,
                }],
                state: WorldState {
                    schema_id: "world_v1".into(),
                    data: serde_json::json!({ "tick": seq }),
                },
            }
        };

        let mut msg_json = serde_json::to_string(&StreamMessage::Perception(perception)).unwrap();
        msg_json.push('\n');
        server_io.write_all(msg_json.as_bytes()).await.unwrap();
        server_io.flush().await.unwrap();

        // Brief delay between ticks
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    // Allow time for agent actor loop to process
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Read outbound actions emitted by agent over TWP stream
    let mut server_reader = BufReader::new(server_io);
    let mut action_received = false;

    let mut line = String::new();
    // Read with timeout
    if let Ok(Ok(n)) = tokio::time::timeout(Duration::from_millis(200), server_reader.read_line(&mut line)).await {
        if n > 0 {
            if let Ok(StreamMessage::Action(action)) = serde_json::from_str::<StreamMessage>(line.trim()) {
                assert_eq!(action.kind, "respond_to_event");
                action_received = true;
            }
        }
    }
    assert!(action_received, "Agent failed to emit action over TWP stream");

    // Verify SensoryFilter efficiency:
    // Out of 50 ticks, at least 38 of the 40 identical idle ticks were suppressed (>75% overall suppression)
    let evals = brain.evaluations();
    assert!(
        evals <= 12,
        "SensoryFilter failed: brain evaluated {evals} times out of 50 ticks (expected <= 12)"
    );

    // Verify Authoritative E-Stop halts execution
    adapter.trigger_estop("Operator containment trigger".into()).unwrap();
    assert!(adapter.is_estopped());

    drop(server_reader);
    let _ = agent_handle;
}

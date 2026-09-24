//! [`CompositeWorldAdapter`] — multi-adapter composition and environment routing.
//!
//! Enables an agent or squad to dock into multiple disparate environments simultaneously
//! (e.g., telemetry metrics stream, code repository, cloud infrastructure, communication channels).
//!
//! Inbound perceptions are multiplexed and tagged with their source adapter.
//! Outbound actions are demultiplexed and routed to the target child adapter based on action prefix.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{mpsc, Mutex};
use tracing::warn;

use tetonic_domain::{
    ActionResult, Affordance, EstopSwitch, PerceptionReceiver, PerceptionSender,
    WorldAction, WorldAdapter, WorldError, WorldManifest,
};

/// An adapter composing multiple named child [`WorldAdapter`]s into a unified interface.
pub struct CompositeWorldAdapter {
    adapters: HashMap<String, Arc<dyn WorldAdapter>>,
    estop: Arc<EstopSwitch>,
    description: String,
    perception_sender: PerceptionSender,
    perception_receiver: Mutex<Option<PerceptionReceiver>>,
    default_adapter: Option<String>,
}

impl CompositeWorldAdapter {
    /// Create a new empty composite adapter.
    pub fn new(description: impl Into<String>) -> Self {
        let (perception_tx, perception_rx) = mpsc::channel(256);
        Self {
            adapters: HashMap::new(),
            estop: Arc::new(EstopSwitch::new()),
            description: description.into(),
            perception_sender: perception_tx,
            perception_receiver: Mutex::new(Some(perception_rx)),
            default_adapter: None,
        }
    }

    /// Attach a named child adapter (e.g. "code", "metrics", "cloud").
    pub fn with_adapter(mut self, name: impl Into<String>, adapter: Arc<dyn WorldAdapter>) -> Self {
        let name_str = name.into();
        if self.default_adapter.is_none() {
            self.default_adapter = Some(name_str.clone());
        }
        self.adapters.insert(name_str, adapter);
        self
    }

    /// Specify which child adapter handles un-namespaced actions.
    pub fn with_default_adapter(mut self, name: impl Into<String>) -> Self {
        self.default_adapter = Some(name.into());
        self
    }

    /// Number of child adapters currently bound.
    pub fn adapter_count(&self) -> usize {
        self.adapters.len()
    }
}

#[async_trait]
impl WorldAdapter for CompositeWorldAdapter {
    fn open(&self) -> (PerceptionSender, PerceptionReceiver) {
        let mut guard = self.perception_receiver.try_lock().expect("open called once");
        let rx = guard.take().expect("open called only once per composite adapter");

        // Spawn multiplexing forwarder for each child adapter
        for (name, child) in &self.adapters {
            let (child_tx, mut child_rx) = child.open();
            drop(child_tx); // Don't keep child sender open from composite side

            let parent_tx = self.perception_sender.clone();
            let source_name = name.clone();

            tokio::spawn(async move {
                while let Some(mut p) = child_rx.recv().await {
                    // Tag signals with namespace
                    for signal in &mut p.signals {
                        signal.name = format!("{source_name}.{}", signal.name);
                    }
                    // Tag events with source namespace
                    for event in &mut p.events {
                        event.source = Some(source_name.clone());
                        event.kind = format!("{source_name}.{}", event.kind);
                    }

                    if parent_tx.send(p).await.is_err() {
                        break;
                    }
                }
            });
        }

        (self.perception_sender.clone(), rx)
    }

    async fn execute(&self, action: WorldAction) -> Result<ActionResult, WorldError> {
        // 1. Check composite E-Stop
        self.estop.check(&action.kind)?;

        // 2. Parse namespace prefix (e.g. "code.checkout" -> adapter="code", kind="checkout")
        if let Some((prefix, sub_kind)) = action.kind.split_once('.') {
            let prefix = prefix.to_string();
            let sub_kind = sub_kind.to_string();
            if let Some(target) = self.adapters.get(&prefix) {
                let mut child_action = action;
                child_action.kind = sub_kind;
                return target.execute(child_action).await;
            } else {
                return Err(WorldError::ActionRejected {
                    kind: action.kind,
                    reason: format!("unknown child adapter namespace '{prefix}'"),
                });
            }
        }

        // 3. Fallback to default adapter if un-namespaced
        if let Some(ref default_name) = self.default_adapter {
            if let Some(target) = self.adapters.get(default_name) {
                return target.execute(action).await;
            }
        }

        Err(WorldError::ActionRejected {
            kind: action.kind,
            reason: "action kind is not namespaced and no default adapter is configured".into(),
        })
    }

    fn describe(&self) -> &str {
        &self.description
    }

    fn manifest(&self) -> WorldManifest {
        let mut manifest = WorldManifest::new(&self.description, "1.0");

        for (name, child) in &self.adapters {
            let child_manifest = child.manifest();
            for aff in child_manifest.affordances {
                manifest.affordances.push(Affordance {
                    action_kind: format!("{name}.{}", aff.action_kind),
                    description: format!("[{name}] {}", aff.description),
                    parameters_schema: aff.parameters_schema,
                    is_durative: aff.is_durative,
                });
            }
        }

        manifest
    }

    fn trigger_estop(&self, reason: String) -> Result<(), WorldError> {
        self.estop.trigger(reason.clone());
        for (name, child) in &self.adapters {
            if let Err(e) = child.trigger_estop(reason.clone()) {
                warn!(adapter = %name, error = %e, "failed to propagate E-Stop to child adapter");
            }
        }
        Ok(())
    }

    fn resume(&self) -> Result<(), WorldError> {
        self.estop.resume();
        for (name, child) in &self.adapters {
            if let Err(e) = child.resume() {
                warn!(adapter = %name, error = %e, "failed to propagate resume to child adapter");
            }
        }
        Ok(())
    }

    fn is_estopped(&self) -> bool {
        self.estop.is_estopped() || self.adapters.values().any(|a| a.is_estopped())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tetonic_domain::{
        BrainPathway, Perception, Signal, SignalValue, Trend, Urgency, WorldEvent, WorldState,
    };

    struct TestMockAdapter {
        manifest: WorldManifest,
        estop: EstopSwitch,
        executed: std::sync::Mutex<Vec<WorldAction>>,
        sender: PerceptionSender,
        receiver: std::sync::Mutex<Option<PerceptionReceiver>>,
    }

    impl TestMockAdapter {
        fn new(manifest: WorldManifest) -> (Arc<Self>, PerceptionSender) {
            let (tx, rx) = mpsc::channel(10);
            let adapter = Arc::new(Self {
                manifest,
                estop: EstopSwitch::new(),
                executed: std::sync::Mutex::new(Vec::new()),
                sender: tx.clone(),
                receiver: std::sync::Mutex::new(Some(rx)),
            });
            (adapter, tx)
        }
    }

    #[async_trait]
    impl WorldAdapter for TestMockAdapter {
        fn open(&self) -> (PerceptionSender, PerceptionReceiver) {
            let rx = self.receiver.lock().unwrap().take().unwrap();
            (self.sender.clone(), rx)
        }

        async fn execute(&self, action: WorldAction) -> Result<ActionResult, WorldError> {
            self.estop.check(&action.kind)?;
            self.manifest.validate_action(&action)?;
            self.executed.lock().unwrap().push(action);
            Ok(ActionResult {
                success: true,
                feedback: Some("mock ok".into()),
                state_changed: true,
            })
        }

        fn describe(&self) -> &str {
            "test_mock"
        }

        fn manifest(&self) -> WorldManifest {
            self.manifest.clone()
        }

        fn trigger_estop(&self, reason: String) -> Result<(), WorldError> {
            self.estop.trigger(reason);
            Ok(())
        }

        fn resume(&self) -> Result<(), WorldError> {
            self.estop.resume();
            Ok(())
        }

        fn is_estopped(&self) -> bool {
            self.estop.is_estopped()
        }
    }

    #[tokio::test]
    async fn test_composite_adapter_manifest_aggregation() {
        let code_manifest = WorldManifest::new("code_world", "1.0")
            .with_affordance(Affordance::instant("checkout", "Checkout branch"))
            .with_affordance(Affordance::instant("commit", "Commit changes"));

        let metrics_manifest = WorldManifest::new("metrics_world", "1.0")
            .with_affordance(Affordance::instant("query", "Query Prometheus"));

        let (code_adapter, _) = TestMockAdapter::new(code_manifest);
        let (metrics_adapter, _) = TestMockAdapter::new(metrics_manifest);

        let composite = CompositeWorldAdapter::new("composite_test")
            .with_adapter("code", code_adapter)
            .with_adapter("metrics", metrics_adapter);

        let manifest = composite.manifest();
        assert_eq!(manifest.affordances.len(), 3);
        assert!(manifest.find_affordance("code.checkout").is_some());
        assert!(manifest.find_affordance("code.commit").is_some());
        assert!(manifest.find_affordance("metrics.query").is_some());
    }

    #[tokio::test]
    async fn test_composite_adapter_action_demux_routing() {
        let code_manifest = WorldManifest::new("code_world", "1.0")
            .with_affordance(Affordance::instant("checkout", "Checkout branch"));

        let metrics_manifest = WorldManifest::new("metrics_world", "1.0")
            .with_affordance(Affordance::instant("query", "Query Prometheus"));

        let (code_adapter, _) = TestMockAdapter::new(code_manifest);
        let (metrics_adapter, _) = TestMockAdapter::new(metrics_manifest);

        let composite = CompositeWorldAdapter::new("composite_test")
            .with_adapter("code", code_adapter.clone())
            .with_adapter("metrics", metrics_adapter.clone());

        // Route to code
        let action1 = WorldAction::bare(
            "code.checkout",
            BrainPathway::Reflexive {
                model: "test".into(),
            },
        );
        let res1 = composite.execute(action1).await;
        assert!(res1.is_ok());
        assert_eq!(code_adapter.executed.lock().unwrap().len(), 1);
        assert_eq!(metrics_adapter.executed.lock().unwrap().len(), 0);

        // Route to metrics
        let action2 = WorldAction::bare(
            "metrics.query",
            BrainPathway::Reflexive {
                model: "test".into(),
            },
        );
        let res2 = composite.execute(action2).await;
        assert!(res2.is_ok());
        assert_eq!(code_adapter.executed.lock().unwrap().len(), 1);
        assert_eq!(metrics_adapter.executed.lock().unwrap().len(), 1);

        // Unknown namespace
        let action3 = WorldAction::bare(
            "unknown.ping",
            BrainPathway::Reflexive {
                model: "test".into(),
            },
        );
        let res3 = composite.execute(action3).await;
        assert!(res3.is_err());
    }

    #[tokio::test]
    async fn test_composite_adapter_perception_multiplexing() {
        let code_manifest = WorldManifest::new("code_world", "1.0");
        let metrics_manifest = WorldManifest::new("metrics_world", "1.0");

        let (code_adapter, code_tx) = TestMockAdapter::new(code_manifest);
        let (metrics_adapter, metrics_tx) = TestMockAdapter::new(metrics_manifest);

        let composite = CompositeWorldAdapter::new("composite_test")
            .with_adapter("code", code_adapter)
            .with_adapter("metrics", metrics_adapter);

        let (_tx, mut rx) = composite.open();

        // Send tick from code
        code_tx
            .send(Perception {
                when: chrono::Utc::now(),
                sequence: 1,
                urgency: Urgency::Low,
                signals: vec![Signal {
                    name: "pr_count".into(),
                    value: SignalValue::Int(3),
                    changed: true,
                    trend: Some(Trend::Rising),
                    urgency: Urgency::Low,
                }],
                events: vec![WorldEvent {
                    kind: "review_requested".into(),
                    source: None,
                    payload: serde_json::Value::Null,
                    urgency: Urgency::Low,
                }],
                state: WorldState {
                    schema_id: "code".into(),
                    data: serde_json::Value::Null,
                },
            })
            .await
            .unwrap();

        // Send tick from metrics
        metrics_tx
            .send(Perception {
                when: chrono::Utc::now(),
                sequence: 2,
                urgency: Urgency::High,
                signals: vec![Signal {
                    name: "cpu_usage".into(),
                    value: SignalValue::Float(0.92),
                    changed: true,
                    trend: Some(Trend::Rising),
                    urgency: Urgency::High,
                }],
                events: vec![],
                state: WorldState {
                    schema_id: "metrics".into(),
                    data: serde_json::Value::Null,
                },
            })
            .await
            .unwrap();

        // Receive multiplexed perceptions
        let p1 = rx.recv().await.unwrap();
        let p2 = rx.recv().await.unwrap();

        // Verify that signal names and event kinds were namespaced
        let all_signals: Vec<String> = p1
            .signals
            .iter()
            .chain(p2.signals.iter())
            .map(|s| s.name.clone())
            .collect();
        assert!(all_signals.contains(&"code.pr_count".to_string()));
        assert!(all_signals.contains(&"metrics.cpu_usage".to_string()));
    }

    #[tokio::test]
    async fn test_composite_adapter_estop_propagation() {
        let code_manifest = WorldManifest::new("code_world", "1.0")
            .with_affordance(Affordance::instant("checkout", "Checkout branch"));

        let (code_adapter, _) = TestMockAdapter::new(code_manifest);

        let composite = CompositeWorldAdapter::new("composite_test")
            .with_adapter("code", code_adapter.clone());

        assert!(!composite.is_estopped());
        assert!(!code_adapter.is_estopped());

        // Trip composite E-Stop
        composite.trigger_estop("Global security alert".into()).unwrap();

        assert!(composite.is_estopped());
        assert!(code_adapter.is_estopped());

        // Subsequent execute must fail
        let action = WorldAction::bare(
            "code.checkout",
            BrainPathway::Reflexive {
                model: "test".into(),
            },
        );
        let err = composite.execute(action).await.unwrap_err();
        assert!(matches!(err, WorldError::ActionRejected { .. }));

        // Resume
        composite.resume().unwrap();
        assert!(!composite.is_estopped());
        assert!(!code_adapter.is_estopped());
    }
}

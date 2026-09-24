//! [`WorldAdapter`] — the contract any environment must implement to host
//! Tetonic agents in continuous (live-world) mode.
//!
//! Implementing this trait is the *only* thing a world needs to do.
//! The agent runtime drives the perception → brain → action cycle;
//! the adapter provides environment-specific translation on both sides.
//!
//! # World-agnostic agents
//!
//! The adapter is what makes an agent world-agnostic. The agent definition —
//! identity, goals, brain, memory — never references a specific environment.
//! Deploying an agent to a new world means supplying a new adapter:
//!
//! ```text
//! Agent (universal) + WorldAdapter (simulation) → simulation agent
//! Agent (universal) + WorldAdapter (CI/CD)      → autonomous dev agent
//! Agent (universal) + WorldAdapter (robotics)   → physical robot controller
//! ```
//!
//! # Tick rate
//!
//! The adapter controls the cadence. Send perceptions as fast as your world
//! ticks — sub-second for games and robotics, seconds for simulations,
//! minutes for slow async environments. The brain's System 1 layer handles
//! whatever rate it receives; System 2 runs on its own slower clock.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::{ActionResult, Perception, WorldAction, WorldError};

// ── PerceptionReceiver ────────────────────────────────────────────────────────

/// A channel endpoint the agent runtime reads perceptions from.
///
/// The adapter holds the [`mpsc::Sender`] side and pushes ticks.
/// The agent runtime holds the `PerceptionReceiver` and drives the brain loop.
pub type PerceptionReceiver = mpsc::Receiver<Perception>;
pub type PerceptionSender = mpsc::Sender<Perception>;

// ── Affordance & WorldManifest ────────────────────────────────────────────────

/// An affordance represents an authoritative action capability advertised by a world.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Affordance {
    /// World-defined action verb/kind (e.g. "scale_replicas", "move_to", "create_pr").
    pub action_kind: String,
    /// Human and LLM readable explanation of what the action does.
    pub description: String,
    /// JSON schema describing accepted parameters/arguments for this action.
    pub parameters_schema: Value,
    /// Whether this action executes instantly or is durative (long-running with progress tracking).
    pub is_durative: bool,
}

impl Affordance {
    pub fn new(
        action_kind: impl Into<String>,
        description: impl Into<String>,
        parameters_schema: Value,
        is_durative: bool,
    ) -> Self {
        Self {
            action_kind: action_kind.into(),
            description: description.into(),
            parameters_schema,
            is_durative,
        }
    }

    pub fn instant(action_kind: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            action_kind: action_kind.into(),
            description: description.into(),
            parameters_schema: serde_json::json!({
                "type": "object"
            }),
            is_durative: false,
        }
    }

    pub fn durative(action_kind: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            action_kind: action_kind.into(),
            description: description.into(),
            parameters_schema: serde_json::json!({
                "type": "object"
            }),
            is_durative: true,
        }
    }
}

/// The authoritative manifest of capabilities advertised by a world adapter.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct WorldManifest {
    pub world_name: String,
    pub version: String,
    pub affordances: Vec<Affordance>,
}

impl WorldManifest {
    pub fn new(world_name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            world_name: world_name.into(),
            version: version.into(),
            affordances: Vec::new(),
        }
    }

    pub fn with_affordance(mut self, affordance: Affordance) -> Self {
        self.affordances.push(affordance);
        self
    }

    pub fn find_affordance(&self, action_kind: &str) -> Option<&Affordance> {
        self.affordances.iter().find(|a| a.action_kind == action_kind)
    }

    /// Validates an action proposed by an agent against the world's manifest.
    pub fn validate_action(&self, action: &WorldAction) -> Result<(), WorldError> {
        if self.affordances.is_empty() {
            // Open world — no affordance constraints
            return Ok(());
        }

        let affordance = self.find_affordance(&action.kind).ok_or_else(|| {
            WorldError::ActionRejected {
                kind: action.kind.clone(),
                reason: format!(
                    "action kind '{}' is not advertised by world manifest '{}'",
                    action.kind, self.world_name
                ),
            }
        })?;

        if let Some(req_array) = affordance
            .parameters_schema
            .get("required")
            .and_then(|v| v.as_array())
        {
            if let Some(obj) = action.payload.as_object() {
                for req in req_array {
                    if let Some(field_name) = req.as_str() {
                        if !obj.contains_key(field_name) {
                            return Err(WorldError::ActionRejected {
                                kind: action.kind.clone(),
                                reason: format!("missing required parameter '{field_name}'"),
                            });
                        }
                    }
                }
            } else if !req_array.is_empty() {
                return Err(WorldError::ActionRejected {
                    kind: action.kind.clone(),
                    reason: "action payload must be an object with required parameters".into(),
                });
            }
        }

        Ok(())
    }
}

// ── EstopSwitch ───────────────────────────────────────────────────────────────

/// Authoritative emergency stop actuator interlock.
///
/// Can be embedded in any [`WorldAdapter`] or used standalone to implement
/// physical kill-switch semantics. When triggered, all mutation requests
/// are unconditionally rejected at the adapter boundary.
#[derive(Debug, Default)]
pub struct EstopSwitch {
    estopped: AtomicBool,
    reason: RwLock<Option<String>>,
}

impl EstopSwitch {
    pub fn new() -> Self {
        Self {
            estopped: AtomicBool::new(false),
            reason: RwLock::new(None),
        }
    }

    /// Authoritatively engage the emergency stop.
    pub fn trigger(&self, reason: impl Into<String>) {
        let reason_str = reason.into();
        if let Ok(mut r) = self.reason.write() {
            *r = Some(reason_str);
        }
        self.estopped.store(true, Ordering::SeqCst);
    }

    /// Clear the emergency stop and resume normal operation.
    pub fn resume(&self) {
        if let Ok(mut r) = self.reason.write() {
            *r = None;
        }
        self.estopped.store(false, Ordering::SeqCst);
    }

    /// Whether the emergency stop is currently active.
    pub fn is_estopped(&self) -> bool {
        self.estopped.load(Ordering::SeqCst)
    }

    /// Get current E-Stop reason, if engaged.
    pub fn reason(&self) -> Option<String> {
        self.reason.read().ok().and_then(|r| r.clone())
    }

    /// Enforce interlock gate: returns Err if E-Stop is active.
    pub fn check(&self, action_kind: &str) -> Result<(), WorldError> {
        if self.is_estopped() {
            let detail = self.reason().unwrap_or_else(|| "unspecified reason".into());
            Err(WorldError::ActionRejected {
                kind: action_kind.to_string(),
                reason: format!("E-Stop active: {detail}"),
            })
        } else {
            Ok(())
        }
    }
}

// ── WorldAdapter ──────────────────────────────────────────────────────────────

/// The contract any environment must implement to host a Tetonic agent.
///
/// # Implementation guidance
///
/// - `perception_stream` should start pushing ticks immediately. The agent
///   runtime will drain the receiver as fast as the brain can process them.
/// - For latest-value semantics (e.g. fast game worlds where stale ticks are
///   worse than skipped ticks), use a channel capacity of 1 and `try_send`
///   with `replace` semantics instead of queuing.
/// - `execute` should be idempotent where possible; the brain may issue
///   duplicate actions if it retries after a timeout.
#[async_trait]
pub trait WorldAdapter: Send + Sync {
    /// Open a perception stream at the world's natural tick rate.
    ///
    /// Returns a sender the adapter drives and a receiver the agent reads.
    /// The adapter spawns a background task to push ticks; the runtime
    /// drives the brain loop from the receiver.
    fn open(&self) -> (PerceptionSender, PerceptionReceiver);

    /// Execute an action the agent's brain decided on.
    ///
    /// Returns feedback that may be injected into the next perception's events
    /// so the agent can observe whether its action had the intended effect.
    async fn execute(&self, action: WorldAction) -> Result<ActionResult, WorldError>;

    /// Human-readable description of this world for logging and tracing.
    fn describe(&self) -> &str;

    /// The authoritative manifest of affordances supported by this world.
    fn manifest(&self) -> WorldManifest {
        WorldManifest::default()
    }

    /// Authoritatively trip the physical actuator emergency stop.
    fn trigger_estop(&self, _reason: String) -> Result<(), WorldError> {
        Err(WorldError::Adapter {
            detail: "E-Stop not supported by this adapter".into(),
        })
    }

    /// Clear the emergency stop and allow actuator execution to resume.
    fn resume(&self) -> Result<(), WorldError> {
        Err(WorldError::Adapter {
            detail: "Resume not supported by this adapter".into(),
        })
    }

    /// Whether the emergency stop is currently active.
    fn is_estopped(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BrainPathway;

    #[test]
    fn test_affordance_creation_and_validation() {
        let manifest = WorldManifest::new("cloud_cluster", "1.0")
            .with_affordance(Affordance::new(
                "scale_replicas",
                "Scale cluster service replicas",
                serde_json::json!({
                    "type": "object",
                    "required": ["service", "count"]
                }),
                false,
            ))
            .with_affordance(Affordance::instant("restart_node", "Restart a node"));

        assert_eq!(manifest.affordances.len(), 2);
        assert!(manifest.find_affordance("scale_replicas").is_some());
        assert!(manifest.find_affordance("non_existent").is_none());

        // Valid action
        let valid_action = WorldAction::with_payload(
            "scale_replicas",
            serde_json::json!({ "service": "web", "count": 5 }),
            BrainPathway::Reflexive {
                model: "test".into(),
            },
        );
        assert!(manifest.validate_action(&valid_action).is_ok());

        // Missing required parameter
        let missing_param_action = WorldAction::with_payload(
            "scale_replicas",
            serde_json::json!({ "service": "web" }),
            BrainPathway::Reflexive {
                model: "test".into(),
            },
        );
        let err = manifest.validate_action(&missing_param_action).unwrap_err();
        assert!(matches!(err, WorldError::ActionRejected { .. }));

        // Unadvertised action
        let invalid_action = WorldAction::bare(
            "delete_database",
            BrainPathway::Reflexive {
                model: "test".into(),
            },
        );
        let err = manifest.validate_action(&invalid_action).unwrap_err();
        assert!(matches!(err, WorldError::ActionRejected { .. }));
    }

    #[test]
    fn test_estop_switch_interlock() {
        let estop = EstopSwitch::new();
        assert!(!estop.is_estopped());
        assert!(estop.check("move").is_ok());

        // Trigger E-Stop
        estop.trigger("Operator detected runway deviation");
        assert!(estop.is_estopped());
        assert_eq!(
            estop.reason(),
            Some("Operator detected runway deviation".into())
        );

        let err = estop.check("move").unwrap_err();
        match err {
            WorldError::ActionRejected { kind, reason } => {
                assert_eq!(kind, "move");
                assert!(reason.contains("E-Stop active"));
                assert!(reason.contains("runway deviation"));
            }
            _ => panic!("Expected ActionRejected"),
        }

        // Resume
        estop.resume();
        assert!(!estop.is_estopped());
        assert!(estop.reason().is_none());
        assert!(estop.check("move").is_ok());
    }
}

//! Multi-dimensional intent charters, operational boundaries, and in-flight steering vectors.
//!
//! Intent is not a single prompt string or a single pointed repo. Operating a persistent
//! autonomous fleet requires multi-dimensional intent encompassing strategic objectives,
//! operational boundaries, regulatory invariants, and real-time in-flight steering.

use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{Urgency, WorldAction, WorldError, WorldEvent};

static STEER_SEQ: AtomicU64 = AtomicU64::new(1);

// ── OperationalBoundary ───────────────────────────────────────────────────────

/// Explicit operational constraints and safety boundaries governing agent action.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OperationalBoundary {
    /// Disallow any action matching this exact name or verb.
    ForbiddenAction { action_kind: String },
    /// Restrict permitted action namespaces (e.g. only "staging.*" or "read.*").
    NamespaceAllowlist { allowed: Vec<String> },
    /// Disallow mutating files or paths matching this pattern.
    PathFilter { pattern: String, allow: bool },
    /// Hard ceiling on resource or token consumption.
    ResourceCap { metric: String, max_limit: u64 },
}

// ── IntentCharter ─────────────────────────────────────────────────────────────

/// Multi-dimensional mission charter binding a squad or agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct IntentCharter {
    /// Unique charter identifier.
    pub charter_id: String,
    /// Strategic objectives expressed in natural language.
    pub strategic_intent: String,
    /// Hard operational boundaries that authoritatively constrain actions.
    pub operational_boundaries: Vec<OperationalBoundary>,
    /// Non-negotiable physical and safety invariants.
    pub invariants: Vec<String>,
    /// Target environments or world adapters this squad is aimed at.
    pub target_worlds: Vec<String>,
    /// Measurable criteria for determining mission milestone completion.
    pub success_criteria: Vec<String>,
}

impl IntentCharter {
    pub fn new(charter_id: impl Into<String>, strategic_intent: impl Into<String>) -> Self {
        Self {
            charter_id: charter_id.into(),
            strategic_intent: strategic_intent.into(),
            operational_boundaries: Vec::new(),
            invariants: Vec::new(),
            target_worlds: Vec::new(),
            success_criteria: Vec::new(),
        }
    }

    pub fn with_boundary(mut self, boundary: OperationalBoundary) -> Self {
        self.operational_boundaries.push(boundary);
        self
    }

    pub fn with_invariant(mut self, invariant: impl Into<String>) -> Self {
        self.invariants.push(invariant.into());
        self
    }

    pub fn with_target_world(mut self, world: impl Into<String>) -> Self {
        self.target_worlds.push(world.into());
        self
    }

    pub fn with_success_criterion(mut self, criterion: impl Into<String>) -> Self {
        self.success_criteria.push(criterion.into());
        self
    }

    /// Authoritatively evaluates a proposed action against the charter's boundaries.
    pub fn evaluate_action(&self, action: &WorldAction) -> Result<(), WorldError> {
        for boundary in &self.operational_boundaries {
            match boundary {
                OperationalBoundary::ForbiddenAction { action_kind } => {
                    if &action.kind == action_kind {
                        return Err(WorldError::ActionRejected {
                            kind: action.kind.clone(),
                            reason: format!(
                                "action kind '{}' violates charter boundary: forbidden action",
                                action.kind
                            ),
                        });
                    }
                }
                OperationalBoundary::NamespaceAllowlist { allowed } => {
                    if let Some((ns, _)) = action.kind.split_once('.') {
                        if !allowed.iter().any(|a| a == ns) {
                            return Err(WorldError::ActionRejected {
                                kind: action.kind.clone(),
                                reason: format!(
                                    "action namespace '{ns}' is not in charter allowlist {:?}",
                                    allowed
                                ),
                            });
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Dynamically adjust boundaries in-flight (from a course correction vector).
    pub fn apply_boundary_adjustments(&mut self, adjustments: &[OperationalBoundary]) {
        for adj in adjustments {
            if !self.operational_boundaries.contains(adj) {
                self.operational_boundaries.push(adj.clone());
            }
        }
    }

    /// Synthesize concise markdown context suitable for agent deliberation prompts.
    pub fn render_prompt_context(&self) -> String {
        let mut out = format!("## Mission Charter: {}\n\n", self.charter_id);
        out.push_str(&format!("**Strategic Intent:** {}\n\n", self.strategic_intent));

        if !self.invariants.is_empty() {
            out.push_str("**Safety Invariants:**\n");
            for inv in &self.invariants {
                out.push_str(&format!("- {inv}\n"));
            }
            out.push('\n');
        }

        if !self.target_worlds.is_empty() {
            out.push_str(&format!(
                "**Target Environments:** {}\n\n",
                self.target_worlds.join(", ")
            ));
        }

        if !self.success_criteria.is_empty() {
            out.push_str("**Success Milestones:**\n");
            for sc in &self.success_criteria {
                out.push_str(&format!("- {sc}\n"));
            }
            out.push('\n');
        }

        out
    }
}

// ── SteeringVector ────────────────────────────────────────────────────────────

/// A real-time in-flight course correction vector injected by an operator.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SteeringVector {
    pub vector_id: String,
    pub urgency: Urgency,
    pub directive: String,
    pub boundary_adjustments: Vec<OperationalBoundary>,
    pub issued_at: DateTime<Utc>,
}

impl SteeringVector {
    pub fn new(directive: impl Into<String>) -> Self {
        Self {
            vector_id: format!(
                "steer-{}-{}",
                Utc::now().timestamp_millis(),
                STEER_SEQ.fetch_add(1, Ordering::Relaxed)
            ),
            urgency: Urgency::High,
            directive: directive.into(),
            boundary_adjustments: Vec::new(),
            issued_at: Utc::now(),
        }
    }

    pub fn critical(directive: impl Into<String>) -> Self {
        Self {
            vector_id: format!(
                "steer-{}-{}",
                Utc::now().timestamp_millis(),
                STEER_SEQ.fetch_add(1, Ordering::Relaxed)
            ),
            urgency: Urgency::Critical,
            directive: directive.into(),
            boundary_adjustments: Vec::new(),
            issued_at: Utc::now(),
        }
    }

    pub fn with_boundary_adjustment(mut self, boundary: OperationalBoundary) -> Self {
        self.boundary_adjustments.push(boundary);
        self
    }

    /// Converts this steering vector into a high-urgency sensory event.
    pub fn to_world_event(&self) -> WorldEvent {
        WorldEvent {
            kind: "steering.course_correction".into(),
            source: Some("operator".into()),
            payload: serde_json::json!({
                "vector_id": self.vector_id,
                "directive": self.directive,
                "boundary_adjustments": self.boundary_adjustments,
                "issued_at": self.issued_at,
            }),
            urgency: self.urgency,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BrainPathway;

    #[test]
    fn test_charter_boundary_evaluation() {
        let charter = IntentCharter::new("charter-1", "Scale cluster securely")
            .with_boundary(OperationalBoundary::ForbiddenAction {
                action_kind: "drop_tables".into(),
            })
            .with_boundary(OperationalBoundary::NamespaceAllowlist {
                allowed: vec!["staging".into(), "telemetry".into()],
            });

        // Valid action
        let valid = WorldAction::bare(
            "staging.scale",
            BrainPathway::Reflexive {
                model: "test".into(),
            },
        );
        assert!(charter.evaluate_action(&valid).is_ok());

        // Forbidden action
        let forbidden = WorldAction::bare(
            "drop_tables",
            BrainPathway::Reflexive {
                model: "test".into(),
            },
        );
        let err = charter.evaluate_action(&forbidden).unwrap_err();
        assert!(matches!(err, WorldError::ActionRejected { .. }));

        // Unallowed namespace
        let prod_action = WorldAction::bare(
            "production.scale",
            BrainPathway::Reflexive {
                model: "test".into(),
            },
        );
        let err = charter.evaluate_action(&prod_action).unwrap_err();
        assert!(matches!(err, WorldError::ActionRejected { .. }));
    }

    #[test]
    fn test_steering_vector_to_world_event() {
        let steer = SteeringVector::critical("Halt scaling; diagnose latency spike")
            .with_boundary_adjustment(OperationalBoundary::ForbiddenAction {
                action_kind: "scale".into(),
            });

        let event = steer.to_world_event();
        assert_eq!(event.kind, "steering.course_correction");
        assert_eq!(event.urgency, Urgency::Critical);
        assert_eq!(event.source, Some("operator".into()));
        assert!(event.payload.get("directive").unwrap().as_str().unwrap().contains("Halt scaling"));
    }
}

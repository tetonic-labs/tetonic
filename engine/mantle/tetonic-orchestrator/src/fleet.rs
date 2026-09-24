//! [`Organization`], [`Squad`], and runtime tenancy structures.
//!
//! Organizes standing continuous agents into multi-tenant organizational hierarchies
//! with enforced budget quotas, shared mission charters, and collaborative workpads.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use tetonic_domain::{AgentId, IntentCharter, OrgId, SquadId, SteeringVector};

#[derive(Debug, Error)]
pub enum FleetError {
    #[error("organization '{0}' budget quota exceeded: {1}")]
    BudgetExceeded(OrgId, String),
    #[error("squad '{0}' already registered in organization '{1}'")]
    SquadAlreadyExists(SquadId, OrgId),
    #[error("squad '{0}' not found")]
    SquadNotFound(SquadId),
    #[error("agent '{0}' not found in fleet")]
    AgentNotFound(AgentId),
    #[error("fleet operation failed: {0}")]
    OperationFailed(String),
}

// ── BudgetQuota ───────────────────────────────────────────────────────────────

/// Budget constraints governing an organization's resource consumption.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BudgetQuota {
    pub max_tokens_per_hour: u64,
    pub max_active_agents: usize,
}

impl Default for BudgetQuota {
    fn default() -> Self {
        Self {
            max_tokens_per_hour: 10_000_000,
            max_active_agents: 50,
        }
    }
}

// ── Organization ──────────────────────────────────────────────────────────────

/// An organization representing a tenancy boundary with shared quotas and squads.
pub struct Organization {
    pub id: OrgId,
    pub name: String,
    pub quota: BudgetQuota,
    tokens_consumed: AtomicU64,
    squads: RwLock<HashMap<SquadId, Arc<Squad>>>,
}

impl Organization {
    pub fn new(id: OrgId, name: impl Into<String>, quota: BudgetQuota) -> Self {
        Self {
            id,
            name: name.into(),
            quota,
            tokens_consumed: AtomicU64::new(0),
            squads: RwLock::new(HashMap::new()),
        }
    }

    /// Register a new squad under this organization.
    pub fn register_squad(&self, squad: Arc<Squad>) -> Result<(), FleetError> {
        let mut guard = self.squads.write().unwrap();
        if guard.contains_key(&squad.id) {
            return Err(FleetError::SquadAlreadyExists(squad.id.clone(), self.id.clone()));
        }
        guard.insert(squad.id.clone(), squad);
        Ok(())
    }

    /// Retrieve a squad by ID.
    pub fn get_squad(&self, squad_id: &SquadId) -> Option<Arc<Squad>> {
        self.squads.read().unwrap().get(squad_id).cloned()
    }

    /// Record token consumption and verify against hourly quota.
    pub fn record_tokens(&self, tokens: u64) -> Result<u64, FleetError> {
        let current = self.tokens_consumed.fetch_add(tokens, Ordering::SeqCst) + tokens;
        if current > self.quota.max_tokens_per_hour {
            return Err(FleetError::BudgetExceeded(
                self.id.clone(),
                format!("consumed {current} tokens, exceeding ceiling {}", self.quota.max_tokens_per_hour),
            ));
        }
        Ok(current)
    }

    /// Total tokens consumed in the current accounting window.
    pub fn tokens_consumed(&self) -> u64 {
        self.tokens_consumed.load(Ordering::SeqCst)
    }

    /// Total count of all agents active across all squads in this organization.
    pub fn total_agent_count(&self) -> usize {
        self.squads
            .read()
            .unwrap()
            .values()
            .map(|s| s.member_count())
            .sum()
    }

    /// List all squad IDs in this organization.
    pub fn squad_ids(&self) -> Vec<SquadId> {
        self.squads.read().unwrap().keys().cloned().collect()
    }
}

// ── SharedWorkpad & Bulletin ──────────────────────────────────────────────────

/// A high-bandwidth synchronization board shared across peer agents in a squad.
#[derive(Debug, Default)]
pub struct SharedWorkpad {
    bulletins: RwLock<Vec<Bulletin>>,
}

/// A bulletin posted by an agent for its squad peers to inspect.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bulletin {
    pub author: AgentId,
    pub subject: String,
    pub body: String,
    pub posted_at: DateTime<Utc>,
}

impl SharedWorkpad {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn post(&self, author: AgentId, subject: impl Into<String>, body: impl Into<String>) {
        let mut guard = self.bulletins.write().unwrap();
        guard.push(Bulletin {
            author,
            subject: subject.into(),
            body: body.into(),
            posted_at: Utc::now(),
        });
    }

    pub fn read_all(&self) -> Vec<Bulletin> {
        self.bulletins.read().unwrap().clone()
    }

    pub fn clear(&self) {
        self.bulletins.write().unwrap().clear();
    }
}

// ── Squad ─────────────────────────────────────────────────────────────────────

/// A collaborative squad of standing agents working toward a shared intent charter.
pub struct Squad {
    pub id: SquadId,
    pub org_id: OrgId,
    pub name: String,
    charter: RwLock<IntentCharter>,
    members: RwLock<Vec<AgentId>>,
    pub workpad: SharedWorkpad,
}

impl Squad {
    pub fn new(
        id: SquadId,
        org_id: OrgId,
        name: impl Into<String>,
        charter: IntentCharter,
    ) -> Self {
        Self {
            id,
            org_id,
            name: name.into(),
            charter: RwLock::new(charter),
            members: RwLock::new(Vec::new()),
            workpad: SharedWorkpad::new(),
        }
    }

    /// Add an agent member to this squad.
    pub fn add_member(&self, agent_id: AgentId) {
        let mut guard = self.members.write().unwrap();
        if !guard.contains(&agent_id) {
            guard.push(agent_id);
        }
    }

    /// Remove an agent member from this squad.
    pub fn remove_member(&self, agent_id: &AgentId) {
        let mut guard = self.members.write().unwrap();
        guard.retain(|id| id != agent_id);
    }

    /// Count of agents currently assigned to this squad.
    pub fn member_count(&self) -> usize {
        self.members.read().unwrap().len()
    }

    /// List all agent member IDs.
    pub fn members(&self) -> Vec<AgentId> {
        self.members.read().unwrap().clone()
    }

    /// Get a snapshot of the squad's current intent charter.
    pub fn charter(&self) -> IntentCharter {
        self.charter.read().unwrap().clone()
    }

    /// Apply an in-flight steering vector, adjusting charter boundaries in real time.
    pub fn apply_steering(&self, vector: &SteeringVector) {
        let mut guard = self.charter.write().unwrap();
        guard.apply_boundary_adjustments(&vector.boundary_adjustments);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tetonic_domain::OperationalBoundary;

    #[test]
    fn test_organization_quota_enforcement() {
        let org = Organization::new(
            OrgId::new("org_acme"),
            "Acme Corp",
            BudgetQuota {
                max_tokens_per_hour: 1000,
                max_active_agents: 5,
            },
        );

        assert_eq!(org.tokens_consumed(), 0);
        assert!(org.record_tokens(500).is_ok());
        assert_eq!(org.tokens_consumed(), 500);

        // Exceed quota
        let err = org.record_tokens(600).unwrap_err();
        assert!(matches!(err, FleetError::BudgetExceeded(_, _)));
    }

    #[test]
    fn test_squad_membership_and_workpad() {
        let charter = IntentCharter::new("charter_sre", "Maintain 99.99% uptime");
        let squad = Squad::new(
            SquadId::new("squad_sre"),
            OrgId::new("org_acme"),
            "Site Reliability",
            charter,
        );

        let agent1 = AgentId::new("agent_monitor");
        let agent2 = AgentId::new("agent_remediator");

        squad.add_member(agent1.clone());
        squad.add_member(agent2.clone());
        assert_eq!(squad.member_count(), 2);

        // Peer communication on workpad
        squad.workpad.post(agent1.clone(), "CPU Spike Alert", "Node 4 reporting 98% load");
        let bulletins = squad.workpad.read_all();
        assert_eq!(bulletins.len(), 1);
        assert_eq!(bulletins[0].author, agent1);
        assert_eq!(bulletins[0].subject, "CPU Spike Alert");

        // Apply steering vector to dynamically tighten boundaries
        let steer = SteeringVector::new("Restrict restarts")
            .with_boundary_adjustment(OperationalBoundary::ForbiddenAction {
                action_kind: "hard_reboot".into(),
            });

        squad.apply_steering(&steer);
        let updated_charter = squad.charter();
        assert_eq!(updated_charter.operational_boundaries.len(), 1);

        squad.remove_member(&agent1);
        assert_eq!(squad.member_count(), 1);
    }
}

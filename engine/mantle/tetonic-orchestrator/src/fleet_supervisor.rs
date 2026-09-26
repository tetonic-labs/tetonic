//! [`FleetSupervisor`] — persistent actor lifecycle supervision and fleet controls.
//!
//! Tracks standing agents, monitors heartbeats, distributes in-flight steering vectors,
//! and enforces fleet-wide emergency stop actuator interlocks.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{info, warn};

use tetonic_domain::{
    AgentId, EstopSwitch, OrgId, Perception, SquadId, SteeringVector, WorldAdapter, WorldState,
};

use crate::fleet::{FleetError, Organization};

/// Current lifecycle operational state of a continuous standing agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentLifecycleState {
    Idle,
    Running,
    Paused,
    Estopped,
    Failed,
}

/// A continuous agent managed by the supervisor.
pub struct ManagedAgent {
    pub agent_id: AgentId,
    pub squad_id: Option<SquadId>,
    pub state: RwLock<AgentLifecycleState>,
    pub last_heartbeat: RwLock<DateTime<Utc>>,
    pub perception_tx: Option<mpsc::Sender<Perception>>,
    pub adapter: Option<Arc<dyn WorldAdapter>>,
}

impl ManagedAgent {
    pub fn new(
        agent_id: AgentId,
        squad_id: Option<SquadId>,
        adapter: Option<Arc<dyn WorldAdapter>>,
        perception_tx: Option<mpsc::Sender<Perception>>,
    ) -> Self {
        Self {
            agent_id,
            squad_id,
            state: RwLock::new(AgentLifecycleState::Idle),
            last_heartbeat: RwLock::new(Utc::now()),
            perception_tx,
            adapter,
        }
    }

    pub fn record_heartbeat(&self) {
        *self.last_heartbeat.write().unwrap() = Utc::now();
    }

    pub fn is_stale(&self, max_stale: Duration) -> bool {
        let last = *self.last_heartbeat.read().unwrap();
        Utc::now().signed_duration_since(last) > chrono::Duration::from_std(max_stale).unwrap()
    }
}

/// Aggregated real-time inventory of standing fleet state for the Portal UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetSnapshot {
    pub total_organizations: usize,
    pub total_squads: usize,
    pub total_agents: usize,
    pub running_agents: usize,
    pub estopped_agents: usize,
    pub failed_agents: usize,
    pub is_fleet_estopped: bool,
    pub agents: Vec<AgentStatusSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatusSummary {
    pub agent_id: String,
    pub squad_id: Option<String>,
    pub state: AgentLifecycleState,
    pub last_heartbeat: DateTime<Utc>,
}

/// The runtime coordinator managing standing agents across organizations and squads.
pub struct FleetSupervisor {
    orgs: RwLock<HashMap<OrgId, Arc<Organization>>>,
    agents: RwLock<HashMap<AgentId, Arc<ManagedAgent>>>,
    global_estop: Arc<EstopSwitch>,
}

impl Default for FleetSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl FleetSupervisor {
    pub fn new() -> Self {
        Self {
            orgs: RwLock::new(HashMap::new()),
            agents: RwLock::new(HashMap::new()),
            global_estop: Arc::new(EstopSwitch::new()),
        }
    }

    /// Register an organization with the fleet supervisor.
    pub fn register_org(&self, org: Arc<Organization>) {
        self.orgs.write().unwrap().insert(org.id.clone(), org);
    }

    /// Retrieve an organization by ID.
    pub fn get_org(&self, org_id: &OrgId) -> Option<Arc<Organization>> {
        self.orgs.read().unwrap().get(org_id).cloned()
    }

    /// Register a continuous agent under supervision.
    pub fn register_agent(
        &self,
        agent_id: AgentId,
        squad_id: Option<SquadId>,
        adapter: Option<Arc<dyn WorldAdapter>>,
        perception_tx: Option<mpsc::Sender<Perception>>,
    ) -> Arc<ManagedAgent> {
        let agent = Arc::new(ManagedAgent::new(agent_id.clone(), squad_id, adapter, perception_tx));
        self.agents.write().unwrap().insert(agent_id, agent.clone());
        agent
    }

    /// Record a health heartbeat from a running agent loop.
    pub fn record_heartbeat(&self, agent_id: &AgentId) -> Result<(), FleetError> {
        let guard = self.agents.read().unwrap();
        let agent = guard.get(agent_id).ok_or_else(|| FleetError::AgentNotFound(agent_id.clone()))?;
        agent.record_heartbeat();
        Ok(())
    }

    /// Check if a managed agent is alive and reporting heartbeats within threshold.
    pub fn is_agent_healthy(&self, agent_id: &AgentId, max_stale: Duration) -> bool {
        let guard = self.agents.read().unwrap();
        guard
            .get(agent_id)
            .map(|a| !a.is_stale(max_stale))
            .unwrap_or(false)
    }

    /// Inject an in-flight course correction steering vector across a squad.
    ///
    /// 1. Real-time updates the squad's charter operational boundaries.
    /// 2. Injects a high-urgency sensory event into the perception channel of all member agents.
    pub async fn inject_steering(
        &self,
        squad_id: &SquadId,
        vector: SteeringVector,
    ) -> Result<usize, FleetError> {
        // Find squad across all registered orgs
        let orgs = self.orgs.read().unwrap();
        let mut target_squad = None;
        for org in orgs.values() {
            if let Some(squad) = org.get_squad(squad_id) {
                target_squad = Some(squad);
                break;
            }
        }

        let squad = target_squad.ok_or_else(|| FleetError::SquadNotFound(squad_id.clone()))?;

        // 1. Update charter in-flight
        squad.apply_steering(&vector);

        // 2. Deliver event to all squad members
        let event = vector.to_world_event();
        let mut delivered = 0;
        let member_ids = squad.members();

        let agents_guard = self.agents.read().unwrap();
        for id in member_ids {
            if let Some(agent) = agents_guard.get(&id) {
                if let Some(ref tx) = agent.perception_tx {
                    let perception = Perception {
                        when: Utc::now(),
                        sequence: 0,
                        urgency: event.urgency,
                        signals: vec![],
                        events: vec![event.clone()],
                        state: WorldState {
                            schema_id: "steering".into(),
                            data: serde_json::Value::Null,
                        },
                    };
                    if tx.send(perception).await.is_ok() {
                        delivered += 1;
                    }
                }
            }
        }

        info!(squad_id = %squad_id, delivered = delivered, directive = %vector.directive, "injected steering vector");
        Ok(delivered)
    }

    /// Authoritatively trip the fleet-wide emergency stop.
    ///
    /// Freezes all managed agents, drops mutations, and trips E-Stop on all registered adapters.
    pub fn emergency_stop_fleet(&self, reason: impl Into<String>) {
        let reason_str = reason.into();
        warn!(reason = %reason_str, "engaging fleet-wide emergency stop");

        self.global_estop.trigger(reason_str.clone());

        let agents_guard = self.agents.read().unwrap();
        for agent in agents_guard.values() {
            *agent.state.write().unwrap() = AgentLifecycleState::Estopped;
            if let Some(ref adapter) = agent.adapter {
                let _ = adapter.trigger_estop(reason_str.clone());
            }
        }
    }

    /// Resume the fleet from emergency stop.
    pub fn resume_fleet(&self) {
        info!("resuming fleet from emergency stop");
        self.global_estop.resume();

        let agents_guard = self.agents.read().unwrap();
        for agent in agents_guard.values() {
            // Clearing an emergency stop is not an execution claim.
            *agent.state.write().unwrap() = AgentLifecycleState::Idle;
            if let Some(ref adapter) = agent.adapter {
                let _ = adapter.resume();
            }
        }
    }

    /// Retrieve a managed agent by ID.
    pub fn get_agent(&self, agent_id: &AgentId) -> Option<Arc<ManagedAgent>> {
        self.agents.read().unwrap().get(agent_id).cloned()
    }

    /// Emergency stop a specific managed agent.
    pub fn emergency_stop_agent(
        &self,
        agent_id: &AgentId,
        reason: impl Into<String>,
    ) -> Result<(), FleetError> {
        let reason_str = reason.into();
        let guard = self.agents.read().unwrap();
        let agent = guard
            .get(agent_id)
            .ok_or_else(|| FleetError::AgentNotFound(agent_id.clone()))?;
        *agent.state.write().unwrap() = AgentLifecycleState::Estopped;
        if let Some(ref adapter) = agent.adapter {
            let _ = adapter.trigger_estop(reason_str);
        }
        Ok(())
    }

    /// Emergency stop all agents in a designated squad.
    pub fn emergency_stop_squad(
        &self,
        squad_id: &SquadId,
        reason: impl Into<String>,
    ) -> Result<Vec<AgentId>, FleetError> {
        let reason_str = reason.into();
        let guard = self.agents.read().unwrap();
        let mut stopped = Vec::new();

        for agent in guard.values() {
            if agent.squad_id.as_ref() == Some(squad_id) {
                *agent.state.write().unwrap() = AgentLifecycleState::Estopped;
                if let Some(ref adapter) = agent.adapter {
                    let _ = adapter.trigger_estop(reason_str.clone());
                }
                stopped.push(agent.agent_id.clone());
            }
        }

        Ok(stopped)
    }

    /// Resume a specific managed agent from emergency stop.
    pub fn resume_agent(&self, agent_id: &AgentId) -> Result<(), FleetError> {
        let guard = self.agents.read().unwrap();
        let agent = guard
            .get(agent_id)
            .ok_or_else(|| FleetError::AgentNotFound(agent_id.clone()))?;
        // Clearing an emergency stop is not an execution claim.
        *agent.state.write().unwrap() = AgentLifecycleState::Idle;
        if let Some(ref adapter) = agent.adapter {
            let _ = adapter.resume();
        }
        Ok(())
    }

    /// Whether the fleet is currently under emergency stop.
    pub fn is_fleet_estopped(&self) -> bool {
        self.global_estop.is_estopped()
    }

    /// Produce an aggregated real-time status snapshot for UI and telemetry.
    pub fn fleet_snapshot(&self) -> FleetSnapshot {
        let orgs_guard = self.orgs.read().unwrap();
        let agents_guard = self.agents.read().unwrap();

        let mut total_squads = 0;
        for org in orgs_guard.values() {
            total_squads += org.squad_ids().len();
        }

        let mut running = 0;
        let mut estopped = 0;
        let mut failed = 0;
        let mut summaries = Vec::new();

        for agent in agents_guard.values() {
            let state = *agent.state.read().unwrap();
            match state {
                AgentLifecycleState::Running => running += 1,
                AgentLifecycleState::Estopped => estopped += 1,
                AgentLifecycleState::Failed => failed += 1,
                _ => {}
            }

            summaries.push(AgentStatusSummary {
                agent_id: agent.agent_id.to_string(),
                squad_id: agent.squad_id.as_ref().map(|s| s.to_string()),
                state,
                last_heartbeat: *agent.last_heartbeat.read().unwrap(),
            });
        }

        FleetSnapshot {
            total_organizations: orgs_guard.len(),
            total_squads,
            total_agents: agents_guard.len(),
            running_agents: running,
            estopped_agents: estopped,
            failed_agents: failed,
            is_fleet_estopped: self.global_estop.is_estopped(),
            agents: summaries,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tetonic_domain::{ActionResult, Affordance, BrainPathway, IntentCharter, OperationalBoundary, Urgency, WorldAction, WorldError, WorldManifest};
    use crate::fleet::{BudgetQuota, Squad};

    struct TestSupervisorAdapter {
        estop: EstopSwitch,
        executed: std::sync::Mutex<Vec<WorldAction>>,
    }

    impl TestSupervisorAdapter {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                estop: EstopSwitch::new(),
                executed: std::sync::Mutex::new(Vec::new()),
            })
        }
    }

    #[async_trait::async_trait]
    impl WorldAdapter for TestSupervisorAdapter {
        fn open(&self) -> (tetonic_domain::PerceptionSender, tetonic_domain::PerceptionReceiver) {
            let (tx, rx) = mpsc::channel(1);
            (tx, rx)
        }

        async fn execute(&self, action: WorldAction) -> Result<ActionResult, WorldError> {
            self.estop.check(&action.kind)?;
            self.executed.lock().unwrap().push(action);
            Ok(ActionResult {
                success: true,
                feedback: Some("ok".into()),
                state_changed: true,
            })
        }

        fn describe(&self) -> &str {
            "test_sup"
        }

        fn manifest(&self) -> WorldManifest {
            WorldManifest::new("sup", "1.0")
                .with_affordance(Affordance::instant("act", "Do action"))
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
    async fn test_fleet_supervisor_lifecycle_and_steering() {
        let supervisor = FleetSupervisor::new();

        // 1. Setup Org and Squad
        let org_id = OrgId::new("org_ops");
        let org = Arc::new(Organization::new(org_id.clone(), "Ops Org", BudgetQuota::default()));
        let squad_id = SquadId::new("squad_sec");
        let squad = Arc::new(Squad::new(
            squad_id.clone(),
            org_id.clone(),
            "Security Squad",
            IntentCharter::new("sec_charter", "Maintain security perimeters"),
        ));

        let agent_id = AgentId::new("agent_firewall");
        squad.add_member(agent_id.clone());
        org.register_squad(squad.clone()).unwrap();
        supervisor.register_org(org);

        // 2. Register agent with supervisor and perception channel
        let (tx, mut rx) = mpsc::channel(10);
        let adapter = TestSupervisorAdapter::new();
        supervisor.register_agent(agent_id.clone(), Some(squad_id.clone()), Some(adapter.clone()), Some(tx));

        // 3. Heartbeat check
        assert!(supervisor.is_agent_healthy(&agent_id, Duration::from_secs(5)));
        supervisor.record_heartbeat(&agent_id).unwrap();

        // 4. Inject course correction steering vector
        let steer = SteeringVector::critical("Block inbound port 8080 immediately")
            .with_boundary_adjustment(OperationalBoundary::ForbiddenAction {
                action_kind: "open_port".into(),
            });

        let delivered = supervisor.inject_steering(&squad_id, steer).await.unwrap();
        assert_eq!(delivered, 1);

        // Agent channel receives the high-urgency perception
        let perception = rx.recv().await.expect("steering perception received");
        assert_eq!(perception.urgency, Urgency::Critical);
        assert_eq!(perception.events.len(), 1);
        assert_eq!(perception.events[0].kind, "steering.course_correction");

        // Squad charter has dynamically adopted the boundary
        let updated_charter = squad.charter();
        assert_eq!(updated_charter.operational_boundaries.len(), 1);

        // 5. Emergency Stop Fleet
        assert!(!supervisor.is_fleet_estopped());
        assert!(!adapter.is_estopped());

        supervisor.emergency_stop_fleet("Critical infrastructure alert");

        assert!(supervisor.is_fleet_estopped());
        assert!(adapter.is_estopped());

        // Subsequent execution fails at adapter level
        let action = WorldAction::bare("act", BrainPathway::Reflexive { model: "m".into() });
        let err = adapter.execute(action).await.unwrap_err();
        assert!(matches!(err, WorldError::ActionRejected { .. }));

        // 6. Snapshot verification
        let snapshot = supervisor.fleet_snapshot();
        assert_eq!(snapshot.total_organizations, 1);
        assert_eq!(snapshot.total_squads, 1);
        assert_eq!(snapshot.total_agents, 1);
        assert_eq!(snapshot.estopped_agents, 1);
        assert!(snapshot.is_fleet_estopped);

        // 7. Resume fleet
        supervisor.resume_fleet();
        assert!(!supervisor.is_fleet_estopped());
        assert!(!adapter.is_estopped());
    }
}

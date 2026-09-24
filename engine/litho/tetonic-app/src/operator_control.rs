//! Operator Control Surface: Intent Formulation, In-Flight Steering, and E-Stop (SAE-503).
//!
//! Provides the primary operational control plane for human operators to:
//! 1. Formulate intent and inject in-flight steering vectors (`SteeringVector`).
//! 2. Execute authoritative emergency stops (`EstopSwitch`) at agent, squad, or fleet levels.
//! 3. Inspect high-level fleet dashboard states with real-time status cards and live thought snippets.

use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;

use tetonic_domain::charter::{OperationalBoundary, SteeringVector};
use tetonic_domain::ids::{AgentId, SquadId};
use tetonic_domain::perception::Urgency;
use tetonic_orchestrator::{AgentLifecycleState, FleetSupervisor};

use crate::fleet_api::{FleetApiError, FleetManager};
use crate::thought_stream::{TelemetryEvent, ThoughtStreamHub};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum OperatorControlError {
    #[error("Fleet API error: {0}")]
    Fleet(#[from] FleetApiError),
    #[error("Agent '{0}' not found")]
    AgentNotFound(String),
    #[error("Squad '{0}' not found")]
    SquadNotFound(String),
    #[error("Invalid request payload: {0}")]
    BadRequest(String),
    #[error("HTTP route not found: {method} {path}")]
    NotFound { method: String, path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteerAgentRequest {
    pub directive: String,
    pub urgency: Option<Urgency>,
    pub boundaries: Option<Vec<OperationalBoundary>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteerResponse {
    pub agent_id: String,
    pub squad_id: String,
    pub directive: String,
    pub applied: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EstopRequest {
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EstopResponse {
    pub target: String,
    pub target_type: String,
    pub reason: String,
    pub affected_agents: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeResponse {
    pub target: String,
    pub status: AgentLifecycleState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorAgentCard {
    pub agent_id: String,
    pub squad_id: String,
    pub role: String,
    pub status: AgentLifecycleState,
    pub latest_thought: Option<String>,
    pub active_boundaries_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorDashboardView {
    pub total_orgs: usize,
    pub total_squads: usize,
    pub total_agents: usize,
    pub running_agents: usize,
    pub estopped_agents: usize,
    pub is_fleet_estopped: bool,
    pub agents: Vec<OperatorAgentCard>,
}

/// The operator control surface managing steering, E-stops, and dashboard assembly.
pub struct OperatorController {
    supervisor: Arc<Mutex<FleetSupervisor>>,
    fleet_manager: Arc<FleetManager>,
    thought_hub: Arc<ThoughtStreamHub>,
}

impl OperatorController {
    pub fn new(
        supervisor: Arc<Mutex<FleetSupervisor>>,
        fleet_manager: Arc<FleetManager>,
        thought_hub: Arc<ThoughtStreamHub>,
    ) -> Self {
        Self {
            supervisor,
            fleet_manager,
            thought_hub,
        }
    }

    /// Injects an in-flight steering vector to realign an agent and its squad without restart.
    pub async fn steer_agent(
        &self,
        agent_id: &str,
        req: SteerAgentRequest,
    ) -> Result<SteerResponse, OperatorControlError> {
        let agent_resp = self.fleet_manager.get_agent(agent_id).await?;
        let squad_id = SquadId::new(agent_resp.squad_id.clone());

        let mut vector = SteeringVector::new(&req.directive);
        if let Some(urgency) = req.urgency {
            vector.urgency = urgency;
        }
        if let Some(boundaries) = req.boundaries {
            vector.boundary_adjustments = boundaries;
        }

        let supervisor = self.supervisor.lock().await;
        let delivered = supervisor
            .inject_steering(&squad_id, vector)
            .await
            .map_err(|_| OperatorControlError::SquadNotFound(squad_id.0.clone()))?;

        // Broadcast steering signal to live thought stream
        self.thought_hub
            .publish(TelemetryEvent::ThoughtDelta {
                agent_id: agent_id.to_string(),
                delta: format!("[Steering Injected]: {}", req.directive),
                timestamp: Utc::now(),
            })
            .await;

        Ok(SteerResponse {
            agent_id: agent_id.to_string(),
            squad_id: squad_id.0,
            directive: req.directive,
            applied: delivered > 0,
        })
    }

    /// Authoritatively trips the emergency stop for a specific agent.
    pub async fn estop_agent(
        &self,
        agent_id: &str,
        reason: &str,
    ) -> Result<EstopResponse, OperatorControlError> {
        let id = AgentId::new(agent_id);
        let supervisor = self.supervisor.lock().await;

        supervisor
            .emergency_stop_agent(&id, reason)
            .map_err(|_| OperatorControlError::AgentNotFound(agent_id.to_string()))?;

        self.thought_hub
            .publish(TelemetryEvent::LifecycleChange {
                agent_id: agent_id.to_string(),
                new_status: AgentLifecycleState::Estopped,
                timestamp: Utc::now(),
            })
            .await;

        Ok(EstopResponse {
            target: agent_id.to_string(),
            target_type: "agent".to_string(),
            reason: reason.to_string(),
            affected_agents: vec![agent_id.to_string()],
        })
    }

    /// Authoritatively trips the emergency stop across all agents in a squad.
    pub async fn estop_squad(
        &self,
        squad_id: &str,
        reason: &str,
    ) -> Result<EstopResponse, OperatorControlError> {
        let id = SquadId::new(squad_id);
        let supervisor = self.supervisor.lock().await;

        let stopped = supervisor
            .emergency_stop_squad(&id, reason)
            .map_err(|_| OperatorControlError::SquadNotFound(squad_id.to_string()))?;

        let affected_strings: Vec<String> = stopped.into_iter().map(|a| a.0).collect();

        for ag in &affected_strings {
            self.thought_hub
                .publish(TelemetryEvent::LifecycleChange {
                    agent_id: ag.clone(),
                    new_status: AgentLifecycleState::Estopped,
                    timestamp: Utc::now(),
                })
                .await;
        }

        Ok(EstopResponse {
            target: squad_id.to_string(),
            target_type: "squad".to_string(),
            reason: reason.to_string(),
            affected_agents: affected_strings,
        })
    }

    /// Authoritatively trips the emergency stop across the entire fleet.
    pub async fn estop_fleet(&self, reason: &str) -> Result<EstopResponse, OperatorControlError> {
        let supervisor = self.supervisor.lock().await;
        supervisor.emergency_stop_fleet(reason);

        let snap = supervisor.fleet_snapshot();
        let affected: Vec<String> = snap.agents.into_iter().map(|a| a.agent_id).collect();

        for ag in &affected {
            self.thought_hub
                .publish(TelemetryEvent::LifecycleChange {
                    agent_id: ag.clone(),
                    new_status: AgentLifecycleState::Estopped,
                    timestamp: Utc::now(),
                })
                .await;
        }

        Ok(EstopResponse {
            target: "fleet".to_string(),
            target_type: "fleet".to_string(),
            reason: reason.to_string(),
            affected_agents: affected,
        })
    }

    /// Resumes an estopped agent once cleared by an operator.
    pub async fn resume_agent(&self, agent_id: &str) -> Result<ResumeResponse, OperatorControlError> {
        let id = AgentId::new(agent_id);
        let supervisor = self.supervisor.lock().await;

        supervisor
            .resume_agent(&id)
            .map_err(|_| OperatorControlError::AgentNotFound(agent_id.to_string()))?;

        self.thought_hub
            .publish(TelemetryEvent::LifecycleChange {
                agent_id: agent_id.to_string(),
                new_status: AgentLifecycleState::Running,
                timestamp: Utc::now(),
            })
            .await;

        Ok(ResumeResponse {
            target: agent_id.to_string(),
            status: AgentLifecycleState::Running,
        })
    }

    /// Gathers a real-time operational dashboard snapshot.
    pub async fn get_dashboard_snapshot(&self) -> Result<OperatorDashboardView, OperatorControlError> {
        let supervisor = self.supervisor.lock().await;
        let snap = supervisor.fleet_snapshot();
        drop(supervisor);

        let mut cards = Vec::new();

        for agent in snap.agents {
            let record = match self.fleet_manager.get_agent(&agent.agent_id).await {
                Ok(r) => r,
                Err(_) => continue,
            };

            // Grab the newest thought delta from the ring buffer
            let history = self.thought_hub.get_recent_history(&agent.agent_id).await;
            let latest_thought = history.iter().rev().find_map(|ev| {
                if let TelemetryEvent::ThoughtDelta { delta, .. } = ev {
                    Some(delta.clone())
                } else {
                    None
                }
            });

            cards.push(OperatorAgentCard {
                agent_id: agent.agent_id,
                squad_id: record.squad_id,
                role: record.role,
                status: agent.state,
                latest_thought,
                active_boundaries_count: record.active_boundaries_count,
            });
        }

        Ok(OperatorDashboardView {
            total_orgs: snap.total_organizations,
            total_squads: snap.total_squads,
            total_agents: snap.total_agents,
            running_agents: snap.running_agents,
            estopped_agents: snap.estopped_agents,
            is_fleet_estopped: snap.is_fleet_estopped,
            agents: cards,
        })
    }

    /// Dispatches an operator control REST action.
    pub async fn dispatch_control(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<serde_json::Value, OperatorControlError> {
        let segments: Vec<&str> = path.trim_matches('/').split('/').collect();

        match (method, segments.as_slice()) {
            ("POST", ["api", "v1", "agents", agent_id, "steer"]) => {
                let body_str = body.ok_or_else(|| OperatorControlError::BadRequest("Missing body".into()))?;
                let req: SteerAgentRequest = serde_json::from_str(body_str)
                    .map_err(|e| OperatorControlError::BadRequest(e.to_string()))?;
                let res = self.steer_agent(agent_id, req).await?;
                Ok(serde_json::to_value(res).unwrap())
            }
            ("POST", ["api", "v1", "agents", agent_id, "estop"]) => {
                let reason = body.unwrap_or("operator manual estop");
                let res = self.estop_agent(agent_id, reason).await?;
                Ok(serde_json::to_value(res).unwrap())
            }
            ("POST", ["api", "v1", "squads", squad_id, "estop"]) => {
                let reason = body.unwrap_or("operator squad estop");
                let res = self.estop_squad(squad_id, reason).await?;
                Ok(serde_json::to_value(res).unwrap())
            }
            ("POST", ["api", "v1", "orgs", _, "estop"])
            | ("POST", ["api", "v1", "fleet", "estop"]) => {
                let reason = body.unwrap_or("fleet-wide emergency stop");
                let res = self.estop_fleet(reason).await?;
                Ok(serde_json::to_value(res).unwrap())
            }
            ("POST", ["api", "v1", "agents", agent_id, "resume"]) => {
                let res = self.resume_agent(agent_id).await?;
                Ok(serde_json::to_value(res).unwrap())
            }
            ("GET", ["api", "v1", "operator", "dashboard"]) => {
                let res = self.get_dashboard_snapshot().await?;
                Ok(serde_json::to_value(res).unwrap())
            }
            _ => Err(OperatorControlError::NotFound {
                method: method.to_string(),
                path: path.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet_api::{CreateAgentRequest, CreateOrgRequest, CreateSquadRequest};

    async fn setup_test_fleet() -> (Arc<OperatorController>, Arc<FleetManager>) {
        let supervisor = Arc::new(Mutex::new(FleetSupervisor::new()));
        let manager = Arc::new(FleetManager::new(Arc::clone(&supervisor)));
        let hub = Arc::new(ThoughtStreamHub::default_hub());

        let controller = Arc::new(OperatorController::new(
            supervisor,
            Arc::clone(&manager),
            hub,
        ));

        // Create standard org, squad, agent
        manager
            .create_org(CreateOrgRequest {
                org_id: "org-ops".into(),
                name: "Ops Corp".into(),
                token_budget_hourly: 50_000,
            })
            .await
            .expect("create org");

        manager
            .create_squad(
                "org-ops",
                CreateSquadRequest {
                    squad_id: "squad-ops".into(),
                    name: "Ops Squad".into(),
                    domain_context: "Infra".into(),
                },
            )
            .await
            .expect("create squad");

        manager
            .create_agent(
                "squad-ops",
                CreateAgentRequest {
                    agent_id: "agent-target".into(),
                    role: "SRE".into(),
                    charter: None,
                    world_adapters: vec!["cloud-api".into()],
                },
            )
            .await
            .expect("create agent");

        (controller, manager)
    }

    #[tokio::test]
    async fn test_in_flight_steering_injection() {
        let (controller, _) = setup_test_fleet().await;

        let req = SteerAgentRequest {
            directive: "Focus on reducing tail latency".into(),
            urgency: Some(Urgency::Critical),
            boundaries: None,
        };

        let resp = controller.steer_agent("agent-target", req).await.expect("steer");
        assert_eq!(resp.agent_id, "agent-target");
        assert_eq!(resp.directive, "Focus on reducing tail latency");
    }

    #[tokio::test]
    async fn test_agent_estop_and_resume_lifecycle() {
        let (controller, manager) = setup_test_fleet().await;

        // 1. Initial state is Running
        let ag = manager.get_agent("agent-target").await.expect("get agent");
        assert_eq!(ag.status, AgentLifecycleState::Running);

        // 2. Trigger E-Stop
        let estop_resp = controller
            .estop_agent("agent-target", "critical boundary excursion")
            .await
            .expect("estop");
        assert_eq!(estop_resp.target, "agent-target");

        // 3. Confirm agent is Estopped
        let ag2 = manager.get_agent("agent-target").await.expect("get agent");
        assert_eq!(ag2.status, AgentLifecycleState::Estopped);

        // 4. Resume agent
        let resume_resp = controller.resume_agent("agent-target").await.expect("resume");
        assert_eq!(resume_resp.status, AgentLifecycleState::Running);

        let ag3 = manager.get_agent("agent-target").await.expect("get agent");
        assert_eq!(ag3.status, AgentLifecycleState::Running);
    }

    #[tokio::test]
    async fn test_operator_dashboard_view_aggregates_state() {
        let (controller, _) = setup_test_fleet().await;

        let dashboard = controller.get_dashboard_snapshot().await.expect("dashboard");
        assert_eq!(dashboard.total_agents, 1);
        assert_eq!(dashboard.running_agents, 1);
        assert_eq!(dashboard.estopped_agents, 0);
        assert!(!dashboard.is_fleet_estopped);
        assert_eq!(dashboard.agents.len(), 1);
        assert_eq!(dashboard.agents[0].agent_id, "agent-target");
    }
}

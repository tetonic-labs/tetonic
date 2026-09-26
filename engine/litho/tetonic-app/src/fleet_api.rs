//! Legacy in-memory fleet prototype (SAE-501), pending MVP caller migration.
//!
//! Its REST-shaped dispatcher does not establish an authenticated caller, and
//! its maps are not durable resource or execution authority. Do not expose it
//! as the production control API. New resource composition belongs in
//! `crate::resources`; activation must converge on managed execution before
//! this prototype and its operator consumers can be removed.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;

use tetonic_domain::charter::IntentCharter;
use tetonic_domain::ids::{AgentId, OrgId, SquadId};
use tetonic_orchestrator::{
    AgentLifecycleState, BudgetQuota, FleetSupervisor, Organization, Squad,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FleetApiError {
    #[error("Organization '{0}' already exists")]
    OrgAlreadyExists(String),
    #[error("Organization '{0}' not found")]
    OrgNotFound(String),
    #[error("Squad '{0}' already exists")]
    SquadAlreadyExists(String),
    #[error("Squad '{0}' not found")]
    SquadNotFound(String),
    #[error("Agent '{0}' already exists")]
    AgentAlreadyExists(String),
    #[error("Agent '{0}' not found")]
    AgentNotFound(String),
    #[error("Budget exceeded for organization '{org_id}': attempted {attempted}, available {available}")]
    BudgetExceeded {
        org_id: String,
        attempted: u64,
        available: u64,
    },
    #[error("World adapter validation error: at least one world adapter must be bound")]
    NoWorldAdaptersSpecified,
    #[error("Invalid request payload: {0}")]
    BadRequest(String),
    #[error("HTTP route not found: {method} {path}")]
    NotFound { method: String, path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOrgRequest {
    pub org_id: String,
    pub name: String,
    pub token_budget_hourly: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgResponse {
    pub org_id: String,
    pub name: String,
    pub token_budget_hourly: u64,
    pub tokens_consumed_this_hour: u64,
    pub squad_count: usize,
    pub squads: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSquadRequest {
    pub squad_id: String,
    pub name: String,
    pub domain_context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SquadResponse {
    pub squad_id: String,
    pub org_id: String,
    pub name: String,
    pub domain_context: String,
    pub agent_count: usize,
    pub agents: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAgentRequest {
    pub agent_id: String,
    pub role: String,
    pub charter: Option<IntentCharter>,
    pub world_adapters: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    pub agent_id: String,
    pub squad_id: String,
    pub org_id: String,
    pub role: String,
    pub status: AgentLifecycleState,
    pub world_adapters: Vec<String>,
    pub active_boundaries_count: usize,
}

/// In-memory state and operational supervisor for the fleet management plane.
pub struct FleetManager {
    orgs: Mutex<HashMap<String, Arc<Organization>>>,
    squads: Mutex<HashMap<String, Arc<Squad>>>,
    squad_to_org: Mutex<HashMap<String, String>>,
    agent_records: Mutex<HashMap<String, AgentResponse>>,
    supervisor: Arc<Mutex<FleetSupervisor>>,
}

impl FleetManager {
    pub fn new(supervisor: Arc<Mutex<FleetSupervisor>>) -> Self {
        Self {
            orgs: Mutex::new(HashMap::new()),
            squads: Mutex::new(HashMap::new()),
            squad_to_org: Mutex::new(HashMap::new()),
            agent_records: Mutex::new(HashMap::new()),
            supervisor,
        }
    }

    /// Creates a new Organization with the given hourly budget quota.
    pub async fn create_org(&self, req: CreateOrgRequest) -> Result<OrgResponse, FleetApiError> {
        let mut orgs = self.orgs.lock().await;
        if orgs.contains_key(&req.org_id) {
            return Err(FleetApiError::OrgAlreadyExists(req.org_id));
        }

        let org = Arc::new(Organization::new(
            OrgId::new(req.org_id.clone()),
            req.name.clone(),
            BudgetQuota {
                max_tokens_per_hour: req.token_budget_hourly,
                max_active_agents: 50,
            },
        ));

        self.supervisor.lock().await.register_org(Arc::clone(&org));
        orgs.insert(req.org_id.clone(), org);

        Ok(OrgResponse {
            org_id: req.org_id,
            name: req.name,
            token_budget_hourly: req.token_budget_hourly,
            tokens_consumed_this_hour: 0,
            squad_count: 0,
            squads: Vec::new(),
        })
    }

    /// Retrieves an organization by ID.
    pub async fn get_org(&self, org_id: &str) -> Result<OrgResponse, FleetApiError> {
        let orgs = self.orgs.lock().await;
        let org = orgs
            .get(org_id)
            .ok_or_else(|| FleetApiError::OrgNotFound(org_id.to_string()))?;

        let squads = self.squads.lock().await;
        let org_squads: Vec<String> = squads
            .values()
            .filter(|s| s.org_id.0 == org_id)
            .map(|s| s.id.0.clone())
            .collect();

        Ok(OrgResponse {
            org_id: org.id.0.clone(),
            name: org.name.clone(),
            token_budget_hourly: org.quota.max_tokens_per_hour,
            tokens_consumed_this_hour: org.tokens_consumed(),
            squad_count: org_squads.len(),
            squads: org_squads,
        })
    }

    /// Creates a Squad inside an Organization aimed at a shared domain context.
    pub async fn create_squad(
        &self,
        org_id: &str,
        req: CreateSquadRequest,
    ) -> Result<SquadResponse, FleetApiError> {
        let orgs = self.orgs.lock().await;
        if !orgs.contains_key(org_id) {
            return Err(FleetApiError::OrgNotFound(org_id.to_string()));
        }
        drop(orgs);

        let mut squads = self.squads.lock().await;
        if squads.contains_key(&req.squad_id) {
            return Err(FleetApiError::SquadAlreadyExists(req.squad_id));
        }

        let charter = IntentCharter::new(
            format!("charter-{}", req.squad_id),
            req.domain_context.clone(),
        );
        let squad = Arc::new(Squad::new(
            SquadId::new(req.squad_id.clone()),
            OrgId::new(org_id),
            req.name.clone(),
            charter,
        ));

        // Register squad under the organization
        let orgs = self.orgs.lock().await;
        if let Some(org) = orgs.get(org_id) {
            let _ = org.register_squad(Arc::clone(&squad));
        }
        drop(orgs);

        squads.insert(req.squad_id.clone(), squad);
        self.squad_to_org
            .lock()
            .await
            .insert(req.squad_id.clone(), org_id.to_string());

        Ok(SquadResponse {
            squad_id: req.squad_id,
            org_id: org_id.to_string(),
            name: req.name,
            domain_context: req.domain_context,
            agent_count: 0,
            agents: Vec::new(),
        })
    }

    /// Retrieves a Squad by ID.
    pub async fn get_squad(&self, squad_id: &str) -> Result<SquadResponse, FleetApiError> {
        let squads = self.squads.lock().await;
        let squad = squads
            .get(squad_id)
            .ok_or_else(|| FleetApiError::SquadNotFound(squad_id.to_string()))?;

        let agents = self.agent_records.lock().await;
        let squad_agents: Vec<String> = agents
            .values()
            .filter(|a| a.squad_id == squad_id)
            .map(|a| a.agent_id.clone())
            .collect();

        let charter = squad.charter();
        Ok(SquadResponse {
            squad_id: squad.id.0.clone(),
            org_id: squad.org_id.0.clone(),
            name: squad.name.clone(),
            domain_context: charter.strategic_intent,
            agent_count: squad_agents.len(),
            agents: squad_agents,
        })
    }

    /// Registers agent metadata in a squad. This does not start execution or charge tokens.
    pub async fn create_agent(
        &self,
        squad_id: &str,
        req: CreateAgentRequest,
    ) -> Result<AgentResponse, FleetApiError> {
        if req.world_adapters.is_empty() {
            return Err(FleetApiError::NoWorldAdaptersSpecified);
        }

        let squad_to_org = self.squad_to_org.lock().await;
        let org_id = squad_to_org
            .get(squad_id)
            .cloned()
            .ok_or_else(|| FleetApiError::SquadNotFound(squad_id.to_string()))?;
        drop(squad_to_org);

        // Metadata registration is not inference and does not consume a token budget.
        let orgs = self.orgs.lock().await;
        if !orgs.contains_key(&org_id) {
            return Err(FleetApiError::OrgNotFound(org_id));
        }
        drop(orgs);

        let mut agents = self.agent_records.lock().await;
        if agents.contains_key(&req.agent_id) {
            return Err(FleetApiError::AgentAlreadyExists(req.agent_id));
        }

        let agent_id = AgentId::new(req.agent_id.clone());
        let supervisor = self.supervisor.lock().await;

        let active_boundaries_count = req
            .charter
            .as_ref()
            .map(|c| c.operational_boundaries.len())
            .unwrap_or(0);

        supervisor.register_agent(
            agent_id.clone(),
            Some(SquadId::new(squad_id)),
            None,
            None,
        );

        let resp = AgentResponse {
            agent_id: req.agent_id.clone(),
            squad_id: squad_id.to_string(),
            org_id,
            role: req.role,
            status: AgentLifecycleState::Idle,
            world_adapters: req.world_adapters,
            active_boundaries_count,
        };

        agents.insert(req.agent_id, resp.clone());

        // Update squad membership
        let squads = self.squads.lock().await;
        if let Some(squad) = squads.get(squad_id) {
            squad.add_member(agent_id);
        }

        Ok(resp)
    }

    /// Retrieves an agent record by ID.
    pub async fn get_agent(&self, agent_id: &str) -> Result<AgentResponse, FleetApiError> {
        let mut agents = self.agent_records.lock().await;
        let agent = agents
            .get_mut(agent_id)
            .ok_or_else(|| FleetApiError::AgentNotFound(agent_id.to_string()))?;

        // Reconcile status with live supervisor
        let supervisor = self.supervisor.lock().await;
        if let Some(managed) = supervisor.get_agent(&AgentId::new(agent_id)) {
            agent.status = *managed.state.read().unwrap();
        }

        Ok(agent.clone())
    }

    /// Dispatches a simulated or real HTTP REST route to the corresponding fleet handler.
    pub async fn dispatch_rest(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<serde_json::Value, FleetApiError> {
        let segments: Vec<&str> = path.trim_matches('/').split('/').collect();

        match (method, segments.as_slice()) {
            ("POST", ["api", "v1", "orgs"]) => {
                let body_str = body.ok_or_else(|| FleetApiError::BadRequest("Missing JSON body".into()))?;
                let req: CreateOrgRequest = serde_json::from_str(body_str)
                    .map_err(|e| FleetApiError::BadRequest(e.to_string()))?;
                let res = self.create_org(req).await?;
                Ok(serde_json::to_value(res).unwrap())
            }
            ("GET", ["api", "v1", "orgs", org_id]) => {
                let res = self.get_org(org_id).await?;
                Ok(serde_json::to_value(res).unwrap())
            }
            ("POST", ["api", "v1", "orgs", org_id, "squads"]) => {
                let body_str = body.ok_or_else(|| FleetApiError::BadRequest("Missing JSON body".into()))?;
                let req: CreateSquadRequest = serde_json::from_str(body_str)
                    .map_err(|e| FleetApiError::BadRequest(e.to_string()))?;
                let res = self.create_squad(org_id, req).await?;
                Ok(serde_json::to_value(res).unwrap())
            }
            ("GET", ["api", "v1", "squads", squad_id]) => {
                let res = self.get_squad(squad_id).await?;
                Ok(serde_json::to_value(res).unwrap())
            }
            ("POST", ["api", "v1", "orgs", _, "squads", squad_id, "agents"])
            | ("POST", ["api", "v1", "squads", squad_id, "agents"]) => {
                let body_str = body.ok_or_else(|| FleetApiError::BadRequest("Missing JSON body".into()))?;
                let req: CreateAgentRequest = serde_json::from_str(body_str)
                    .map_err(|e| FleetApiError::BadRequest(e.to_string()))?;
                let res = self.create_agent(squad_id, req).await?;
                Ok(serde_json::to_value(res).unwrap())
            }
            ("GET", ["api", "v1", "agents", agent_id]) => {
                let res = self.get_agent(agent_id).await?;
                Ok(serde_json::to_value(res).unwrap())
            }
            _ => Err(FleetApiError::NotFound {
                method: method.to_string(),
                path: path.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_org_and_squad_hierarchy() {
        let supervisor = Arc::new(Mutex::new(FleetSupervisor::new()));
        let manager = FleetManager::new(supervisor);

        let org_req = CreateOrgRequest {
            org_id: "org-alpha".into(),
            name: "Alpha Corp".into(),
            token_budget_hourly: 50_000,
        };
        let org = manager.create_org(org_req).await.expect("create org");
        assert_eq!(org.org_id, "org-alpha");

        let squad_req = CreateSquadRequest {
            squad_id: "squad-frontend".into(),
            name: "Frontend Squad".into(),
            domain_context: "React, Web, UI".into(),
        };
        let squad = manager
            .create_squad("org-alpha", squad_req)
            .await
            .expect("create squad");
        assert_eq!(squad.squad_id, "squad-frontend");
        assert_eq!(squad.org_id, "org-alpha");
    }

    #[tokio::test]
    async fn test_create_agent_within_budget_and_register_with_supervisor() {
        let supervisor = Arc::new(Mutex::new(FleetSupervisor::new()));
        let manager = FleetManager::new(Arc::clone(&supervisor));

        manager
            .create_org(CreateOrgRequest {
                org_id: "org-1".into(),
                name: "Org One".into(),
                token_budget_hourly: 10_000,
            })
            .await
            .expect("create org");

        manager
            .create_squad(
                "org-1",
                CreateSquadRequest {
                    squad_id: "squad-1".into(),
                    name: "Squad One".into(),
                    domain_context: "Engine".into(),
                },
            )
            .await
            .expect("create squad");

        let agent_req = CreateAgentRequest {
            agent_id: "agent-001".into(),
            role: "Developer".into(),
            charter: None,
            world_adapters: vec!["fs-adapter".into()],
        };

        let agent = manager
            .create_agent("squad-1", agent_req)
            .await
            .expect("create agent");
        assert_eq!(agent.agent_id, "agent-001");
        assert_eq!(agent.status, AgentLifecycleState::Idle);
        assert_eq!(
            manager
                .get_org("org-1")
                .await
                .expect("org")
                .tokens_consumed_this_hour,
            0
        );

        // Confirm agent was registered in FleetSupervisor
        let sup = supervisor.lock().await;
        assert!(sup.get_agent(&AgentId::new("agent-001")).is_some());
    }

    #[tokio::test]
    async fn metadata_creation_does_not_charge_or_report_running() {
        let supervisor = Arc::new(Mutex::new(FleetSupervisor::new()));
        let manager = FleetManager::new(supervisor);

        // Org with microscopic budget
        manager
            .create_org(CreateOrgRequest {
                org_id: "org-broke".into(),
                name: "Budget Exhausted Corp".into(),
                token_budget_hourly: 500,
            })
            .await
            .expect("create org");

        manager
            .create_squad(
                "org-broke",
                CreateSquadRequest {
                    squad_id: "squad-broke".into(),
                    name: "Squad Broke".into(),
                    domain_context: "None".into(),
                },
            )
            .await
            .expect("create squad");

        let created = manager
            .create_agent(
                "squad-broke",
                CreateAgentRequest {
                    agent_id: "agent-broke-1".into(),
                    role: "Tester".into(),
                    charter: None,
                    world_adapters: vec!["mock".into()],
                },
            )
            .await
            .expect("metadata creation");
        assert_eq!(created.status, AgentLifecycleState::Idle);
        let duplicate = manager
            .create_agent(
                "squad-broke",
                CreateAgentRequest {
                    agent_id: "agent-broke-1".into(),
                    role: "Tester".into(),
                    charter: None,
                    world_adapters: vec!["mock".into()],
                },
            )
            .await;
        assert!(matches!(
            duplicate,
            Err(FleetApiError::AgentAlreadyExists(_))
        ));
        assert_eq!(
            manager
                .get_org("org-broke")
                .await
                .expect("org")
                .tokens_consumed_this_hour,
            0
        );
        assert_eq!(
            manager
                .get_agent("agent-broke-1")
                .await
                .expect("agent")
                .status,
            AgentLifecycleState::Idle
        );
    }

    #[tokio::test]
    async fn test_rest_route_dispatching() {
        let supervisor = Arc::new(Mutex::new(FleetSupervisor::new()));
        let manager = FleetManager::new(supervisor);

        let org_body = r#"{"org_id":"org-rest","name":"Rest Corp","token_budget_hourly":100000}"#;
        let org_json = manager
            .dispatch_rest("POST", "/api/v1/orgs", Some(org_body))
            .await
            .expect("POST org");
        assert_eq!(org_json["org_id"], "org-rest");

        let get_org_json = manager
            .dispatch_rest("GET", "/api/v1/orgs/org-rest", None)
            .await
            .expect("GET org");
        assert_eq!(get_org_json["name"], "Rest Corp");
    }
}

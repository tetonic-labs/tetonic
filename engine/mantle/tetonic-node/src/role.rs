//! Node Roles, Keeper Registry, and Runner Lifecycle (SAE-402).
//!
//! Provides the architectural distinction between:
//! - **Standalone**: Embedded coordinator, local runner, and local SQLite.
//! - **Coordinator ("The Keeper")**: Manages metadata, lease proofs, runner registries, heartbeats, and cluster failover.
//! - **Runner**: Executes continuous agent loops, holds local volumes, and streams heartbeats to the coordinator.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use tetonic_domain::execution::ExecutionTargetId;
use tetonic_domain::ids::{AgentId, LeaseId};
use tetonic_domain::{LeaseProof, NodeMode};

/// Role assumed by an engine node in cluster topology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeRole {
    /// Local desktop single-process node.
    Standalone,
    /// Cluster coordinator ("The Keeper") managing state and leases.
    Coordinator,
    /// Worker node executing continuous agent actor loops.
    Runner,
}

impl From<NodeMode> for NodeRole {
    fn from(mode: NodeMode) -> Self {
        match mode {
            NodeMode::Standalone => Self::Standalone,
            NodeMode::Coordinator => Self::Coordinator,
            NodeMode::Runner => Self::Runner,
        }
    }
}

impl From<NodeRole> for NodeMode {
    fn from(role: NodeRole) -> Self {
        match role {
            NodeRole::Standalone => Self::Standalone,
            NodeRole::Coordinator => Self::Coordinator,
            NodeRole::Runner => Self::Runner,
        }
    }
}

/// Operational capability matrix for a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeCapabilities {
    /// Whether this node can host and serve inference token requests.
    pub can_serve_inference: bool,
    /// Whether this node executes persistent agent cognitive actor loops.
    pub can_run_agent_actors: bool,
    /// Whether this node can issue, manage, and audit lease proofs.
    pub can_manage_leases: bool,
    /// Whether this node accepts runner registration handshakes.
    pub can_accept_registrations: bool,
    /// Whether this node communicates over a distributed network fabric.
    pub is_distributed: bool,
}

impl NodeRole {
    /// Returns the capability matrix for this role.
    pub fn capabilities(&self) -> NodeCapabilities {
        match self {
            Self::Standalone => NodeCapabilities {
                can_serve_inference: true,
                can_run_agent_actors: true,
                can_manage_leases: true,
                can_accept_registrations: false,
                is_distributed: false,
            },
            Self::Coordinator => NodeCapabilities {
                can_serve_inference: false,
                can_run_agent_actors: false,
                can_manage_leases: true,
                can_accept_registrations: true,
                is_distributed: true,
            },
            Self::Runner => NodeCapabilities {
                can_serve_inference: false,
                can_run_agent_actors: true,
                can_manage_leases: false,
                can_accept_registrations: false,
                is_distributed: true,
            },
        }
    }
}

/// Status of a registered runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunnerStatus {
    /// Actively streaming heartbeats.
    Active,
    /// Gracefully disconnected.
    Deregistered,
    /// Missed heartbeat deadline and evicted by keeper audit.
    Evicted,
}

/// Registration record for a runner held by the Coordinator.
#[derive(Debug, Clone)]
pub struct RunnerRegistration {
    pub runner_id: String,
    pub bind_addr: String,
    pub capabilities: NodeCapabilities,
    pub last_heartbeat: Instant,
    pub status: RunnerStatus,
    pub assigned_agents: HashSet<AgentId>,
    pub current_lease_epoch: u64,
    pub lease_id: LeaseId,
}

/// Failover event emitted when a runner is evicted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailoverEvent {
    pub evicted_runner_id: String,
    pub orphaned_agents: Vec<AgentId>,
    pub reason: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum KeeperError {
    #[error("Runner '{0}' is already registered and active")]
    AlreadyRegistered(String),
    #[error("Runner '{0}' not found in registry")]
    RunnerNotFound(String),
    #[error("Runner '{0}' is not active (status: {1:?})")]
    RunnerNotActive(String, RunnerStatus),
    #[error("Invalid lease proof for runner '{runner_id}': expected epoch {expected_epoch}, got {actual_epoch}")]
    InvalidLeaseProof {
        runner_id: String,
        expected_epoch: u64,
        actual_epoch: u64,
    },
    #[error("Agent '{0}' is already assigned to runner '{1}'")]
    AgentAlreadyAssigned(String, String),
}

/// Cluster coordinator registry ("The Keeper") managing runners, leases, and failover.
#[derive(Debug, Default)]
pub struct KeeperRegistry {
    runners: HashMap<String, RunnerRegistration>,
    agent_locations: HashMap<AgentId, String>,
    epoch_counter: u64,
}

impl KeeperRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Handles a registration handshake from a runner node, issuing an authoritative lease proof.
    pub fn register_runner(
        &mut self,
        runner_id: String,
        bind_addr: String,
        capabilities: NodeCapabilities,
    ) -> Result<LeaseProof, KeeperError> {
        if let Some(existing) = self.runners.get(&runner_id) {
            if existing.status == RunnerStatus::Active {
                return Err(KeeperError::AlreadyRegistered(runner_id));
            }
        }

        self.epoch_counter += 1;
        let lease_id = LeaseId::new(format!("lease-{}", self.epoch_counter));
        let epoch = 1;

        let reg = RunnerRegistration {
            runner_id: runner_id.clone(),
            bind_addr,
            capabilities,
            last_heartbeat: Instant::now(),
            status: RunnerStatus::Active,
            assigned_agents: HashSet::new(),
            current_lease_epoch: epoch,
            lease_id: lease_id.clone(),
        };

        self.runners.insert(runner_id.clone(), reg);

        Ok(LeaseProof {
            lease_id,
            lease_epoch: epoch,
            holder: ExecutionTargetId::worker(runner_id),
        })
    }

    /// Records a periodic heartbeat from a runner and increments its lease epoch.
    pub fn record_heartbeat(
        &mut self,
        runner_id: &str,
        proof: &LeaseProof,
    ) -> Result<LeaseProof, KeeperError> {
        let entry = self
            .runners
            .get_mut(runner_id)
            .ok_or_else(|| KeeperError::RunnerNotFound(runner_id.to_string()))?;

        if entry.status != RunnerStatus::Active {
            return Err(KeeperError::RunnerNotActive(
                runner_id.to_string(),
                entry.status,
            ));
        }

        if entry.current_lease_epoch != proof.lease_epoch || entry.lease_id != proof.lease_id {
            return Err(KeeperError::InvalidLeaseProof {
                runner_id: runner_id.to_string(),
                expected_epoch: entry.current_lease_epoch,
                actual_epoch: proof.lease_epoch,
            });
        }

        entry.last_heartbeat = Instant::now();
        entry.current_lease_epoch += 1;

        Ok(LeaseProof {
            lease_id: entry.lease_id.clone(),
            lease_epoch: entry.current_lease_epoch,
            holder: ExecutionTargetId::worker(runner_id),
        })
    }

    /// Assigns an agent actor to a healthy active runner.
    pub fn assign_agent(&mut self, runner_id: &str, agent_id: AgentId) -> Result<(), KeeperError> {
        let entry = self
            .runners
            .get_mut(runner_id)
            .ok_or_else(|| KeeperError::RunnerNotFound(runner_id.to_string()))?;

        if entry.status != RunnerStatus::Active {
            return Err(KeeperError::RunnerNotActive(
                runner_id.to_string(),
                entry.status,
            ));
        }

        if let Some(existing_runner) = self.agent_locations.get(&agent_id) {
            if existing_runner != runner_id {
                return Err(KeeperError::AgentAlreadyAssigned(
                    agent_id.0,
                    existing_runner.clone(),
                ));
            }
        }

        entry.assigned_agents.insert(agent_id.clone());
        self.agent_locations.insert(agent_id, runner_id.to_string());
        Ok(())
    }

    /// Gracefully unregisters a runner, releasing its assigned agents without eviction.
    pub fn deregister_runner(&mut self, runner_id: &str) -> Result<Vec<AgentId>, KeeperError> {
        let entry = self
            .runners
            .get_mut(runner_id)
            .ok_or_else(|| KeeperError::RunnerNotFound(runner_id.to_string()))?;

        entry.status = RunnerStatus::Deregistered;
        let released: Vec<AgentId> = entry.assigned_agents.drain().collect();

        for agent in &released {
            self.agent_locations.remove(agent);
        }

        Ok(released)
    }

    /// Audits all active runners against a heartbeat deadline, evicting stale nodes and emitting failover events.
    pub fn audit_heartbeats(&mut self, now: Instant, timeout: Duration) -> Vec<FailoverEvent> {
        let mut events = Vec::new();

        for (runner_id, reg) in self.runners.iter_mut() {
            if reg.status == RunnerStatus::Active && now.duration_since(reg.last_heartbeat) > timeout {
                reg.status = RunnerStatus::Evicted;
                let orphaned: Vec<AgentId> = reg.assigned_agents.drain().collect();

                events.push(FailoverEvent {
                    evicted_runner_id: runner_id.clone(),
                    orphaned_agents: orphaned,
                    reason: format!(
                        "Missed heartbeat deadline (elapsed {:?}, timeout {:?})",
                        now.duration_since(reg.last_heartbeat),
                        timeout
                    ),
                });
            }
        }

        for ev in &events {
            for agent in &ev.orphaned_agents {
                self.agent_locations.remove(agent);
            }
        }

        events
    }

    /// Returns list of active runners.
    pub fn active_runners(&self) -> Vec<RunnerRegistration> {
        self.runners
            .values()
            .filter(|r| r.status == RunnerStatus::Active)
            .cloned()
            .collect()
    }

    /// Finds the runner currently hosting an agent.
    pub fn runner_for_agent(&self, agent_id: &AgentId) -> Option<&str> {
        self.agent_locations.get(agent_id).map(|s| s.as_str())
    }
}

/// Client running inside a Runner node managing handshake and heartbeats with the Coordinator.
#[derive(Debug)]
pub struct RunnerClient {
    pub runner_id: String,
    pub bind_addr: String,
    pub capabilities: NodeCapabilities,
    active_proof: Option<LeaseProof>,
}

impl RunnerClient {
    pub fn new(
        runner_id: impl Into<String>,
        bind_addr: impl Into<String>,
        capabilities: NodeCapabilities,
    ) -> Self {
        Self {
            runner_id: runner_id.into(),
            bind_addr: bind_addr.into(),
            capabilities,
            active_proof: None,
        }
    }

    /// Completes registration handshake by storing the authoritative lease proof.
    pub fn complete_handshake(&mut self, proof: LeaseProof) {
        self.active_proof = Some(proof);
    }

    /// Returns the current active lease proof if registered.
    pub fn active_lease(&self) -> Option<&LeaseProof> {
        self.active_proof.as_ref()
    }

    /// Updates local lease proof upon successful heartbeat renewal.
    pub fn update_lease(&mut self, proof: LeaseProof) {
        self.active_proof = Some(proof);
    }

    /// Checks if this runner currently holds an active lease proof.
    pub fn is_registered(&self) -> bool {
        self.active_proof.is_some()
    }
}

/// High-level lifecycle coordinator for a node instance.
#[derive(Debug)]
pub struct NodeLifecycle {
    pub role: NodeRole,
    pub node_id: String,
    pub bind_addr: String,
    pub keeper: Option<KeeperRegistry>,
    pub runner: Option<RunnerClient>,
    pub is_running: bool,
}

impl NodeLifecycle {
    /// Initializes node lifecycle based on configured role.
    pub fn init(role: NodeRole, node_id: impl Into<String>, bind_addr: impl Into<String>) -> Self {
        let node_id = node_id.into();
        let bind_addr = bind_addr.into();
        let capabilities = role.capabilities();

        let (keeper, runner) = match role {
            NodeRole::Standalone => {
                let mut k = KeeperRegistry::new();
                let mut r = RunnerClient::new(node_id.clone(), bind_addr.clone(), capabilities);
                let proof = k
                    .register_runner(node_id.clone(), bind_addr.clone(), capabilities)
                    .expect("standalone self-registration must succeed");
                r.complete_handshake(proof);
                (Some(k), Some(r))
            }
            NodeRole::Coordinator => (Some(KeeperRegistry::new()), None),
            NodeRole::Runner => (
                None,
                Some(RunnerClient::new(node_id.clone(), bind_addr.clone(), capabilities)),
            ),
        };

        Self {
            role,
            node_id,
            bind_addr,
            keeper,
            runner,
            is_running: true,
        }
    }

    /// Graceful shutdown of node lifecycle.
    pub fn shutdown(&mut self) {
        self.is_running = false;
        if let (Some(keeper), Some(runner)) = (&mut self.keeper, &self.runner) {
            let _ = keeper.deregister_runner(&runner.runner_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_role_capabilities_matrix() {
        let standalone = NodeRole::Standalone.capabilities();
        assert!(standalone.can_run_agent_actors);
        assert!(standalone.can_manage_leases);
        assert!(!standalone.is_distributed);

        let coordinator = NodeRole::Coordinator.capabilities();
        assert!(!coordinator.can_run_agent_actors);
        assert!(coordinator.can_manage_leases);
        assert!(coordinator.can_accept_registrations);
        assert!(coordinator.is_distributed);

        let runner = NodeRole::Runner.capabilities();
        assert!(runner.can_run_agent_actors);
        assert!(!runner.can_manage_leases);
        assert!(!runner.can_accept_registrations);
        assert!(runner.is_distributed);
    }

    #[test]
    fn test_keeper_runner_handshake_and_lease() {
        let mut keeper = KeeperRegistry::new();
        let runner_caps = NodeRole::Runner.capabilities();

        // 1. Handshake
        let proof = keeper
            .register_runner(
                "runner-1".into(),
                "10.0.0.2:4430".into(),
                runner_caps,
            )
            .expect("registration failed");

        assert_eq!(proof.lease_epoch, 1);
        assert_eq!(proof.holder, ExecutionTargetId::worker("runner-1"));

        // 2. Heartbeat renewal
        let renewed = keeper
            .record_heartbeat("runner-1", &proof)
            .expect("heartbeat failed");
        assert_eq!(renewed.lease_epoch, 2);

        // 3. Stale epoch fails
        let stale = keeper.record_heartbeat("runner-1", &proof);
        assert!(matches!(stale, Err(KeeperError::InvalidLeaseProof { .. })));
    }

    #[test]
    fn test_keeper_audit_detects_stale_heartbeats_and_triggers_failover() {
        let mut keeper = KeeperRegistry::new();
        let runner_caps = NodeRole::Runner.capabilities();

        keeper
            .register_runner(
                "runner-flakey".into(),
                "10.0.0.5:4430".into(),
                runner_caps,
            )
            .expect("registration");

        let agent_a = AgentId("agent-alpha".into());
        keeper
            .assign_agent("runner-flakey", agent_a.clone())
            .expect("assign agent");

        assert_eq!(keeper.runner_for_agent(&agent_a), Some("runner-flakey"));

        // Simulate 35 seconds elapsed without heartbeat
        let now = Instant::now() + Duration::from_secs(35);
        let events = keeper.audit_heartbeats(now, Duration::from_secs(30));

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].evicted_runner_id, "runner-flakey");
        assert_eq!(events[0].orphaned_agents, vec![agent_a.clone()]);

        // Agent location is released for failover reassignment
        assert_eq!(keeper.runner_for_agent(&agent_a), None);
    }

    #[test]
    fn test_runner_graceful_deregistration() {
        let mut keeper = KeeperRegistry::new();
        let runner_caps = NodeRole::Runner.capabilities();

        keeper
            .register_runner("runner-clean".into(), "10.0.0.6:4430".into(), runner_caps)
            .expect("registration");

        let agent = AgentId("agent-beta".into());
        keeper.assign_agent("runner-clean", agent.clone()).expect("assign");

        let released = keeper.deregister_runner("runner-clean").expect("deregister");
        assert_eq!(released, vec![agent.clone()]);
        assert_eq!(keeper.runner_for_agent(&agent), None);
    }

    #[test]
    fn test_standalone_node_lifecycle_self_registers() {
        let lifecycle = NodeLifecycle::init(NodeRole::Standalone, "local-0", "127.0.0.1:4430");
        assert!(lifecycle.is_running);
        assert!(lifecycle.keeper.is_some());
        assert!(lifecycle.runner.is_some());
        assert!(lifecycle.runner.as_ref().unwrap().is_registered());
    }
}

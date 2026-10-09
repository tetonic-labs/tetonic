//! Additional workspace/team/agent restrictions over existing host authority.
//! Presets never grant a tool, a path, a connection, or an execution identity.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tetonic_domain::{ActionKind, ActionPolicyOutcome, ApprovalRequirement, PolicyDecision};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyTier {
    #[default]
    Automatic,
    ReviewChanges,
    ReadOnly,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    FileRead,
    FileWrite,
    Shell,
    Connections,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityDecision {
    #[default]
    Allow,
    Ask,
    Deny,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityPolicy {
    pub tier: AutonomyTier,
    #[serde(default)]
    pub overrides: BTreeMap<Capability, CapabilityDecision>,
    /// A communication ceiling, never a grant or an expansion of work scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub communication: Option<CommunicationScope>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommunicationScope {
    AssignedWork,
    Blocked,
    SelectedAgents { agent_ids: Vec<String> },
}

impl CommunicationScope {
    pub fn allows(&self, peer: &str) -> bool {
        match self {
            Self::AssignedWork => true,
            Self::Blocked => false,
            Self::SelectedAgents { agent_ids } => agent_ids.iter().any(|id| id == peer),
        }
    }
}
impl CapabilityPolicy {
    pub fn allows_communication(&self, peer: &str) -> bool {
        self.communication
            .as_ref()
            .map(|scope| scope.allows(peer))
            .unwrap_or(self.tier != AutonomyTier::ReadOnly)
    }
    pub fn decision(&self, capability: Capability) -> CapabilityDecision {
        use CapabilityDecision::*;
        *self
            .overrides
            .get(&capability)
            .unwrap_or(&match (self.tier, capability) {
                (_, Capability::FileRead) | (AutonomyTier::Automatic, _) => Allow,
                (AutonomyTier::ReadOnly, _) => Deny,
                (AutonomyTier::ReviewChanges, _) => Ask,
            })
    }
}
pub fn action_capability(kind: &ActionKind) -> Option<Capability> {
    match kind {
        ActionKind::ReadFile | ActionKind::ReadEnvironment => Some(Capability::FileRead),
        ActionKind::WriteFile | ActionKind::DeleteFile | ActionKind::GitOperation => {
            Some(Capability::FileWrite)
        }
        ActionKind::ExecuteShell
        | ActionKind::ExecuteProcess
        | ActionKind::StartInternalService => Some(Capability::Shell),
        ActionKind::NetworkRequest => Some(Capability::Connections),
        _ => None,
    }
}
/// Every explicit level is a ceiling: lower levels can narrow but cannot relax it.
pub fn capability_outcome(policies: &[CapabilityPolicy], kind: &ActionKind) -> ActionPolicyOutcome {
    let decision = action_capability(kind)
        .map(|cap| {
            policies
                .iter()
                .map(|p| p.decision(cap))
                .max()
                .unwrap_or_default()
        })
        .unwrap_or_default();
    ActionPolicyOutcome {
        decision: if decision == CapabilityDecision::Deny {
            PolicyDecision::deny("This capability is blocked by workspace, team or agent rules.")
        } else {
            PolicyDecision::Allow
        },
        approval: if decision == CapabilityDecision::Ask {
            ApprovalRequirement::Interactive
        } else {
            ApprovalRequirement::None
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn communication_defaults_to_assigned_work_except_readonly_and_requires_selected_identity() {
        let mut policy = CapabilityPolicy::default();
        assert!(policy.allows_communication("sam"));
        policy.tier = AutonomyTier::ReadOnly;
        assert!(!policy.allows_communication("sam"));
        policy.communication = Some(CommunicationScope::SelectedAgents {
            agent_ids: vec!["sam".into()],
        });
        assert!(policy.allows_communication("sam"));
        assert!(!policy.allows_communication("other"));
        policy.communication = Some(CommunicationScope::Blocked);
        assert!(!policy.allows_communication("sam"));
    }
    #[test]
    fn scoped_tiers_and_exceptions_never_relax_another_levels_limit() {
        let mut workspace = CapabilityPolicy::default();
        workspace
            .overrides
            .insert(Capability::Shell, CapabilityDecision::Deny);
        let mut agent = CapabilityPolicy {
            tier: AutonomyTier::ReviewChanges,
            ..Default::default()
        };
        agent
            .overrides
            .insert(Capability::FileWrite, CapabilityDecision::Allow);
        assert!(!capability_outcome(
            &[workspace.clone(), agent.clone()],
            &ActionKind::ExecuteShell
        )
        .decision
        .allowed());
        assert_eq!(
            capability_outcome(&[agent.clone()], &ActionKind::WriteFile).approval,
            ApprovalRequirement::None
        );
        let team = CapabilityPolicy {
            tier: AutonomyTier::ReviewChanges,
            ..Default::default()
        };
        assert_eq!(
            capability_outcome(&[workspace, team, agent], &ActionKind::WriteFile).approval,
            ApprovalRequirement::Interactive
        );
    }
    #[test]
    fn readonly_blocks_effects_but_preserves_file_reads() {
        let p = CapabilityPolicy {
            tier: AutonomyTier::ReadOnly,
            ..Default::default()
        };
        assert!(
            capability_outcome(std::slice::from_ref(&p), &ActionKind::ReadFile)
                .decision
                .allowed()
        );
        for kind in [
            ActionKind::WriteFile,
            ActionKind::ExecuteShell,
            ActionKind::NetworkRequest,
        ] {
            assert!(!capability_outcome(std::slice::from_ref(&p), &kind)
                .decision
                .allowed());
        }
    }
}

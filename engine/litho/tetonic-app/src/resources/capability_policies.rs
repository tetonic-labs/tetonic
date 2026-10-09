//! Authenticated settings and live restrictions for the existing action broker.
use super::*;
use tetonic_domain::{
    sinks::PolicyEvaluator, ActionPolicyOutcome, ApprovalRequirement, ExecutionScope,
    PolicyDecision, ProposedAction,
};
use tetonic_memory::{SaveCapabilityPolicy, ScopedCapabilityPolicy};

impl ResourceService {
    pub async fn capability_policies(
        &self,
        credential: &str,
        org: String,
        team: String,
    ) -> Result<Vec<ScopedCapabilityPolicy>, ResourceError> {
        let action = ResourceAction::ReadTeam {
            org_id: org.clone(),
            team_id: team.clone(),
        };
        let actor = self.authority.authorize(credential, &action).await?;
        let rows = self
            .store
            .read(move |db| db.capability_policies(&actor.principal_id, &org, &team))
            .await??;
        self.authority.authorize(credential, &action).await?;
        Ok(rows)
    }
    pub async fn save_capability_policy(
        &self,
        credential: &str,
        org: String,
        team: String,
        request: SaveCapabilityPolicy,
    ) -> Result<ScopedCapabilityPolicy, ResourceError> {
        let actor = self
            .authority
            .authorize(
                credential,
                &ResourceAction::ManageTeam {
                    org_id: org.clone(),
                    team_id: team.clone(),
                },
            )
            .await?;
        Ok(self
            .store
            .write(move |db| db.save_capability_policy(&actor.principal_id, &org, &team, &request))
            .await??)
    }
}

pub(super) struct WorkCapabilityPolicy {
    pub store: SharedStore,
    pub scope: ExecutionScope,
    pub work: Option<(String, String)>,
    pub agent: String,
}
impl PolicyEvaluator for WorkCapabilityPolicy {
    fn evaluate(&self, action: &ProposedAction) -> ActionPolicyOutcome {
        let Some((team, work)) = &self.work else {
            // Non-workspace hosts retain their existing host policy.
            return tetonic_policy::capabilities::capability_outcome(&[], &action.kind);
        };
        match self.store.read_sync(|db| {
            db.work_capability_policies(
                &self.scope.principal_id,
                &self.scope.organization_id,
                team,
                work,
                &self.agent,
            )
        }) {
            Ok(Ok(policies)) => {
                tetonic_policy::capabilities::capability_outcome(&policies, &action.kind)
            }
            _ => ActionPolicyOutcome {
                decision: PolicyDecision::deny(
                    "Current capability permissions could not be checked.",
                ),
                approval: ApprovalRequirement::None,
            },
        }
    }
}

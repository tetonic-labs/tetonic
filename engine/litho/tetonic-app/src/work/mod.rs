//! Scoped work shaping, admission, coordination and projections over existing resources.
use crate::errors::AppError;
use crate::resources::TeamWorkLaunch;
use crate::workspace::{
    resource, validate_request_id, WorkspaceServices, AGENT, DEFAULT_WORK_TOKENS, INPUT_LIMIT,
};
pub use crate::workspace::{
    CreateLocalAgent, ImportSkill, LocalAgent, LocalAgentCatalog, LocalModelCatalog, LocalProvider,
    RemoveProviderKey, RevokeSkill, SaveMcpConnection, SaveProviderKey, UpdateLocalAgent,
};
use serde::{Deserialize, Serialize};
use tetonic_domain::{ExecutionScope, RunState, TaskState};
mod types;
pub use types::*;
mod capabilities;
mod conversations;
mod director;
mod inspection;
pub(crate) mod plan_execution;
mod plan_human;
mod plan_recovery;
mod plans;
mod shaping;
mod submission;
mod work_teams;
mod workroom;
pub use plan_execution::{PlanExecutionView, PlanTaskLink, StartPlan};
pub use plan_human::{AmendPlanAssignment, AnswerPlanQuestion};
pub use plan_recovery::ContinuePlan;
pub use plans::{PlanCommand, PlanView};
pub use shaping::{SaveWorkBrief, WorkPurpose};
pub use work_teams::{SaveWorkTeam, WorkTeam, WorkTeamSelection};
pub use workroom::BudgetSettingsRequest;
#[derive(Clone)]
pub struct WorkService {
    pub(crate) services: WorkspaceServices,
}
#[cfg(test)]
use crate::local_workspace::{LocalWorkspace, ORG, OWNER, TEAM};
#[cfg(test)]
use inspection::task_failure_message;
#[cfg(test)]
mod tests;

#[cfg(test)]
use crate::workspace::LOCAL_TOKEN_CEILING;

#[cfg(test)]
mod scope_tests;

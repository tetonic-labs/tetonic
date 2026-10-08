//! Control records and work intent. Transactional permission and revision checks stay here.

pub(crate) mod capability;
pub(crate) mod control_bootstrap;
pub(crate) mod control_credentials;
pub(crate) mod control_requests;
pub(crate) mod delegated_grants;
pub(crate) mod delegated_lifetime;
pub(crate) mod estate;
pub(crate) mod execution_grants;
pub(crate) mod huddle_execution;
pub(crate) mod huddle_plans;
pub(crate) mod human_controls;
pub(crate) mod identity_store;
pub(crate) mod local_provider_keys;
pub(crate) mod local_work_notes;
pub(crate) mod membership_admin;
pub(crate) mod membership_store;
#[cfg(test)]
pub(crate) mod organization_agent_edit_tests;
pub(crate) mod organization_agent_edits;
pub(crate) mod organization_agent_revisions;
pub(crate) mod organization_agents;
pub(crate) mod plan_continuation;
pub(crate) mod plan_human;
pub(crate) mod policy;
pub(crate) mod secret_overrides;
pub(crate) mod team_admin;
pub(crate) mod team_store;
pub(crate) mod team_work;
pub(crate) mod trust;
pub(crate) mod work_briefs;
pub(crate) mod work_metadata;
pub(crate) mod work_teams;
pub(crate) mod workspace_mcp;
pub(crate) mod workspace_skills;

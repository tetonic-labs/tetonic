//! Session orchestrator host (D3) + briefing (D5).

mod briefing;
mod critic;
mod domain_pack;
mod handoff;
mod host;
mod router;
mod router_llm;
mod run;
mod spawn;
mod spawn_budget;
mod spawn_host;
mod spawn_session;
mod specialist;
mod turn;
pub mod fleet;
pub mod fleet_supervisor;

#[cfg(test)]
mod security_fixture_tests;

#[cfg(test)]
mod turn_tests;

pub use briefing::{
    build_session_briefing, fabric_hint_from_snapshot, BriefingInput, BriefingOptions,
};
pub use critic::{
    critic_prompt_from_tracker, critic_user_prompt, parse_critic_verdict, should_run_critic,
    should_run_critic_enhanced, CriticOutcome,
};
pub use domain_pack::{DomainPack, PackManifest};
pub use handoff::{carve_max_steps, SpawnHandoff, SpawnPointer};
pub use host::{SessionHost, SessionStartPlan, TurnHooks, VerifyResolver};
pub use router::{
    resolve_route, route_task, route_task_with_context, RouteContext, RouteDecision, RouteMode,
    RouteSource,
};
pub use router_llm::{llm_route_task, parse_llm_route_response};
pub use run::{
    critic_outcome_from_steps, next_child_agent_id, route_label, spawn_depth,
    specialist_agent_config, OrchestrationMode, SpawnLimits, TurnTracker,
};
pub use spawn::{spawn_agent_description, spawn_agent_parameters_schema, spawn_agent_tool_name};
pub use spawn_budget::{SpawnAdmitCtx, SpawnAdmitError, SpawnBudgetGate, SpawnReservationToken};
pub use spawn_host::SpawnHost;
pub use spawn_session::SpawnSessionTrack;
pub use specialist::{DynamicAgentSpec, RoleId, SpecialistPack};
pub use turn::{
    format_orchestration_log, format_router_log, run_orchestrated_turn, run_spawned_specialist,
    AgentBuildRequest, ChildAdmit, ChildJob, OrchestratedTurnInput, OrchestratedTurnOutcome,
    RootExecute, ROOT_AGENT,
};
pub use fleet::{BudgetQuota, Bulletin, FleetError, Organization, SharedWorkpad, Squad};
pub use fleet_supervisor::{
    AgentLifecycleState, AgentStatusSummary, FleetSnapshot, FleetSupervisor, ManagedAgent,
};

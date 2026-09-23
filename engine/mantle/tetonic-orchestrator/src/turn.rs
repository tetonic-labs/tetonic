//! Shared orchestrated user turn (D11/D12/A13) — CLI + daemon.

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use tetonic_core::{Agent, Conversation, SpawnHook, SpawnRequest, Step};
use tetonic_domain::{AgentInvocation, AttemptId, CandidateOutcome, ToolOutcome};

use crate::critic::{critic_prompt_from_tracker, should_run_critic_enhanced, CriticOutcome};
use crate::handoff::SpawnHandoff;
use crate::router::{resolve_route, RouteContext, RouteDecision, RouteMode, RouteSource};
use crate::run::OrchestrationMode;
use crate::run::{
    critic_outcome_from_steps, next_child_agent_id, route_label, spawn_depth, SpawnLimits,
    TurnTracker,
};
use crate::specialist::{RoleId, SpecialistPack};

pub const ROOT_AGENT: &str = "a0";

/// Request passed to the host's agent factory.
#[derive(Clone)]
pub struct AgentBuildRequest {
    pub agent_id: String,
    pub role: Option<RoleId>,
    pub dynamic_spec: Option<crate::specialist::DynamicAgentSpec>,
    pub use_hard_model: bool,
    /// Agent may call `spawn_agent` when orchestration is auto and nesting allowed.
    pub orchestration_tools: bool,
    /// Carved effort budget for this agent (A13 v5).
    pub max_steps: Option<usize>,
    /// Read-only / explain user turn (DF0).
    pub explain_turn: bool,
    /// Spawned specialist (SEC2-E2-013): least-privilege tool surface.
    pub spawned: bool,
    /// Inherited workspace version from parent agent (OPT-401).
    pub inherited_workspace_version: Option<tetonic_domain::WorkspaceVersion>,
}

pub struct OrchestratedTurnInput<'a> {
    pub user_text: &'a str,
    pub orchestration: OrchestrationMode,
    pub critic_enabled: bool,
    pub verify_gated: bool,
    pub workspace_root: &'a Path,
    pub index_db: Option<&'a Path>,
    pub code_index: Option<&'a dyn tetonic_domain::CodeIndexOpen>,
    pub pack: &'a dyn SpecialistPack,
    /// Pre-computed LLM route (v4+); keyword router used when `None`.
    pub llm_route: Option<RouteDecision>,
    /// Session started with `model_tier: hard` — force hard tier on routed turns.
    pub session_prefers_hard: bool,
    pub spawn_limits: SpawnLimits,
    /// Root effort cap for spawn budget carving (A13 v5).
    pub session_max_steps: usize,
    /// Persisted root Attempt this turn is bound to (WORK-02).
    pub root_attempt_id: AttemptId,
}

/// Adjacent consume hook so the manager binds root and child start without
/// `lokai-orchestrator` → `lokai-runtime`.
#[async_trait::async_trait]
pub trait RootExecute: Send + Sync {
    async fn execute(
        &self,
        attempt_id: AttemptId,
        agent: &mut Agent,
        conversation: &mut Conversation,
        invocation: AgentInvocation,
        on_step: &mut (dyn FnMut(Step) + Send),
    ) -> CandidateOutcome;
}

/// In-loop child admit/complete. Implemented in lokai-app. Orchestrator does
/// not import RunSupervisor.
pub struct ChildAdmit {
    pub agent_id: String,
    pub role: Option<RoleId>,
    pub job_input: String,
}

#[async_trait::async_trait]
pub trait ChildJob: Send + Sync {
    async fn admit_child(&self, req: ChildAdmit) -> Result<AttemptId, String>;
    async fn complete_child(
        &self,
        attempt_id: AttemptId,
        outcome: CandidateOutcome,
    ) -> Result<(), String>;
}

#[async_trait::async_trait]
impl ChildJob for Arc<dyn ChildJob> {
    async fn admit_child(&self, req: ChildAdmit) -> Result<AttemptId, String> {
        (**self).admit_child(req).await
    }

    async fn complete_child(
        &self,
        attempt_id: AttemptId,
        outcome: CandidateOutcome,
    ) -> Result<(), String> {
        (**self).complete_child(attempt_id, outcome).await
    }
}

#[async_trait::async_trait]
impl RootExecute for Arc<dyn RootExecute> {
    async fn execute(
        &self,
        attempt_id: AttemptId,
        agent: &mut Agent,
        conversation: &mut Conversation,
        invocation: AgentInvocation,
        on_step: &mut (dyn FnMut(Step) + Send),
    ) -> CandidateOutcome {
        (**self)
            .execute(attempt_id, agent, conversation, invocation, on_step)
            .await
    }
}

pub struct OrchestratedTurnOutcome {
    pub route: RouteDecision,
    pub primary_agent_id: String,
    pub critic_ran: bool,
    pub revision_ran: bool,
    pub spawn_agent_calls: u32,
    pub model_tier: &'static str,
    pub outcome: CandidateOutcome,
}

/// Run routing, specialist turn, optional problem-driven critic, optional revision.
#[allow(clippy::too_many_arguments)]
pub async fn run_orchestrated_turn<F, G, R, E, J>(
    conversation: &mut Conversation,
    input: &OrchestratedTurnInput<'_>,
    spawn_serial: Arc<AtomicU32>,
    turn_spawn_count: Arc<AtomicU32>,
    mut build_agent: F,
    mut on_step: G,
    on_route: Option<R>,
    root_execute: E,
    child_job: J,
) -> Result<OrchestratedTurnOutcome, String>
where
    F: FnMut(AgentBuildRequest, &str) -> Result<(Agent, AgentInvocation), String> + Send,
    G: FnMut(&str, Step) + Send,
    R: FnOnce(RouteDecision, &'static str),
    E: RootExecute,
    J: ChildJob,
{
    conversation.begin_turn();
    turn_spawn_count.store(0, Ordering::SeqCst);

    let ctx = RouteContext {
        workspace_root: input.workspace_root,
        index_db: input.index_db,
        code_index: input.code_index,
        pack: input.pack,
    };
    let decision = resolve_route(
        input.user_text,
        input.orchestration == OrchestrationMode::Auto,
        ctx,
        input.llm_route.clone(),
        input.session_prefers_hard,
    );

    let model_tier = if decision.use_hard_tier {
        "hard"
    } else {
        "fast"
    };

    if let Some(f) = on_route {
        f(decision.clone(), model_tier);
    }

    let (role, agent_id) = match &decision.mode {
        RouteMode::Single => (None, ROOT_AGENT.to_string()),
        RouteMode::Specialist(r) => {
            let n = spawn_serial.fetch_add(1, Ordering::SeqCst);
            (Some(r.clone()), next_child_agent_id(ROOT_AGENT, n))
        }
    };

    let orchestration_tools = input.orchestration == OrchestrationMode::Auto && role.is_none();

    let (mut agent, invocation) = build_agent(
        AgentBuildRequest {
            agent_id: agent_id.clone(),
            role: role.clone(),
            dynamic_spec: None,
            use_hard_model: decision.use_hard_tier,
            orchestration_tools,
            max_steps: Some(input.session_max_steps),
            explain_turn: input.pack.root_explain_turn(input.user_text),
            spawned: false,
            inherited_workspace_version: None,
        },
        input.user_text,
    )?;

    let mut tracker = TurnTracker::default();
    let result = {
        let aid = agent_id.clone();
        let mut step_cb = |step: Step| {
            tracker.on_step(&step);
            on_step(&aid, step);
        };
        root_execute
            .execute(
                input.root_attempt_id.clone(),
                &mut agent,
                conversation,
                invocation,
                &mut step_cb,
            )
            .await
    };

    let mut critic_ran = false;
    let mut revision_ran = false;
    let mut terminal = result;

    if terminal.is_completed() {
        let critic_role = role.clone().unwrap_or_else(|| input.pack.default_role());
        if should_run_critic_enhanced(
            input.pack,
            &critic_role,
            &tracker,
            input.verify_gated,
            input.critic_enabled,
        ) {
            critic_ran = true;
            let n = spawn_serial.fetch_add(1, Ordering::SeqCst);
            let critic_id = next_child_agent_id(ROOT_AGENT, n);
            let prompt = critic_prompt_from_tracker(&tracker);
            let (mut critic, critic_invocation) = build_agent(
                AgentBuildRequest {
                    agent_id: critic_id.clone(),
                    role: Some(input.pack.critic_role()),
                    dynamic_spec: None,
                    use_hard_model: false,
                    orchestration_tools: false,
                    max_steps: None,
                    explain_turn: false,
                    spawned: false,
                    inherited_workspace_version: None,
                },
                &prompt,
            )?;
            let mut critic_tracker = TurnTracker::default();
            let cid = critic_id.clone();
            let critic_attempt = child_job
                .admit_child(ChildAdmit {
                    agent_id: critic_id.clone(),
                    role: Some(input.pack.critic_role()),
                    job_input: prompt.clone(),
                })
                .await?;
            let mut critic_step = |step: Step| {
                critic_tracker.on_step(&step);
                on_step(&cid, step);
            };
            let critic_outcome = root_execute
                .execute(
                    critic_attempt.clone(),
                    &mut critic,
                    conversation,
                    critic_invocation,
                    &mut critic_step,
                )
                .await;
            child_job
                .complete_child(critic_attempt, critic_outcome.clone())
                .await?;
            if !critic_outcome.is_completed() {
                terminal = critic_outcome;
            } else if let CriticOutcome::Revise(feedback) =
                critic_outcome_from_steps(&critic_tracker)
            {
                revision_ran = true;
                let n2 = spawn_serial.fetch_add(1, Ordering::SeqCst);
                let coder_id = next_child_agent_id(ROOT_AGENT, n2);
                let msg = format!(
                    "Address critic feedback and complete the task:\n{feedback}\n\nOriginal task:\n{}",
                    input.user_text
                );
                let (mut coder, coder_invocation) = build_agent(
                    AgentBuildRequest {
                        agent_id: coder_id.clone(),
                        role: Some(input.pack.revision_role()),
                        dynamic_spec: None,
                        use_hard_model: decision.use_hard_tier,
                        orchestration_tools: false,
                        max_steps: None,
                        explain_turn: false,
                        spawned: false,
                        inherited_workspace_version: None,
                    },
                    &msg,
                )?;
                let cid = coder_id.clone();
                let coder_attempt = child_job
                    .admit_child(ChildAdmit {
                        agent_id: coder_id.clone(),
                        role: Some(input.pack.revision_role()),
                        job_input: msg.clone(),
                    })
                    .await?;
                let mut coder_step = |step: Step| on_step(&cid, step);
                terminal = root_execute
                    .execute(
                        coder_attempt.clone(),
                        &mut coder,
                        conversation,
                        coder_invocation,
                        &mut coder_step,
                    )
                    .await;
                child_job
                    .complete_child(coder_attempt, terminal.clone())
                    .await?;
            }
        }
    }

    Ok(OrchestratedTurnOutcome {
        route: decision,
        primary_agent_id: agent_id,
        critic_ran,
        revision_ran,
        spawn_agent_calls: turn_spawn_count.load(Ordering::SeqCst),
        model_tier,
        outcome: terminal,
    })
}

/// Handle an in-loop `spawn_agent` tool call (A13). Folds child transcript into compact handoff.
#[allow(clippy::too_many_arguments)]
pub async fn run_spawned_specialist<F, G, J, E>(
    conversation: &mut Conversation,
    req: SpawnRequest,
    spawn_serial: Arc<AtomicU32>,
    turn_spawn_count: Arc<AtomicU32>,
    limits: SpawnLimits,
    mut build_agent: F,
    mut on_step: G,
    spawn_hook: Option<SpawnHook>,
    role: RoleId,
    spawn_track: Option<Arc<crate::spawn_session::SpawnSessionTrack>>,
    session_id: String,
    admit_spawn: Option<Arc<dyn crate::spawn_budget::SpawnBudgetGate>>,
    child_job: J,
    root_execute: E,
    preadmitted: Option<AttemptId>,
) -> ToolOutcome
where
    F: FnMut(AgentBuildRequest, &str) -> Result<(Agent, AgentInvocation), String> + Send,
    G: FnMut(&str, Step) + Send,
    J: ChildJob,
    E: RootExecute,
{
    let depth = spawn_depth(&req.parent_agent_id);
    if depth > limits.max_depth {
        return ToolOutcome::fail(
            format!("spawn depth {depth} exceeds limit {}", limits.max_depth),
            "other",
        );
    }
    if turn_spawn_count.load(Ordering::SeqCst) >= limits.max_per_turn {
        return ToolOutcome::fail(
            format!(
                "spawn budget exhausted ({}/{})",
                turn_spawn_count.load(Ordering::SeqCst),
                limits.max_per_turn
            ),
            "other",
        );
    }

    // Allocate identity before admit so the ledger keys the child (H2-3 AC1/AC5).
    // Do not bump counters until reservation succeeds — refused spawn creates no context.
    let n = spawn_serial.load(Ordering::SeqCst);
    let agent_id = next_child_agent_id(&req.parent_agent_id, n);

    let reservation = if let Some(gate) = &admit_spawn {
        match gate.admit(&crate::spawn_budget::SpawnAdmitCtx {
            session_id: session_id.clone(),
            parent_agent_id: req.parent_agent_id.clone(),
            child_agent_id: agent_id.clone(),
            depth,
        }) {
            Ok(tok) => Some(tok),
            Err(e) => {
                return ToolOutcome::fail(e.tool_message(), "other");
            }
        }
    } else {
        None
    };

    spawn_serial.fetch_add(1, Ordering::SeqCst);
    turn_spawn_count.fetch_add(1, Ordering::SeqCst);
    if let Some(ref track) = spawn_track {
        track.register_agent(&agent_id);
    }

    let checkpoint = conversation.checkpoint();
    let spawn_task = req.task.clone();
    let built = match build_agent(
        AgentBuildRequest {
            agent_id: agent_id.clone(),
            role: Some(role.clone()),
            dynamic_spec: None,
            use_hard_model: false,
            orchestration_tools: spawn_hook.is_some(),
            max_steps: None,
            explain_turn: false,
            spawned: true,
            inherited_workspace_version: None,
        },
        &spawn_task,
    ) {
        Ok((mut a, invocation)) => {
            if let Some(hook) = spawn_hook {
                a = a.with_spawn(hook);
            }
            (a, invocation)
        }
        Err(e) => {
            if let (Some(gate), Some(tok)) = (admit_spawn.as_ref(), reservation.as_ref()) {
                gate.release(tok);
            }
            return ToolOutcome::fail(e, "other");
        }
    };
    let (mut agent, invocation) = built;

    let complete_here = preadmitted.is_none();
    let child_attempt = match preadmitted {
        Some(id) => id,
        None => match child_job
            .admit_child(ChildAdmit {
                agent_id: agent_id.clone(),
                role: Some(role.clone()),
                job_input: spawn_task.clone(),
            })
            .await
        {
            Ok(id) => id,
            Err(e) => {
                if let (Some(gate), Some(tok)) = (admit_spawn.as_ref(), reservation.as_ref()) {
                    gate.release(tok);
                }
                return ToolOutcome::fail(e, "other");
            }
        },
    };

    let mut tracker = TurnTracker::default();
    let aid = agent_id.clone();
    let mut step_cb = |step: Step| {
        tracker.on_step(&step);
        on_step(&aid, step);
    };
    let turn_result = root_execute
        .execute(
            child_attempt.clone(),
            &mut agent,
            conversation,
            invocation,
            &mut step_cb,
        )
        .await;
    if complete_here {
        if let Err(e) = child_job
            .complete_child(child_attempt, turn_result.clone())
            .await
        {
            if let (Some(gate), Some(tok)) = (admit_spawn.as_ref(), reservation.as_ref()) {
                gate.release(tok);
            }
            conversation.rollback_to(checkpoint);
            return ToolOutcome::fail(e, "other");
        }
    }

    if let (Some(gate), Some(tok)) = (admit_spawn.as_ref(), reservation.as_ref()) {
        gate.release(tok);
    }

    let spawn_ok = match &turn_result {
        CandidateOutcome::Completed { .. } => true,
        CandidateOutcome::Limited { .. } if tracker.last_finish_summary.is_some() => true,
        _ => false,
    };
    conversation.rollback_to(checkpoint);
    if let Some(ref track) = spawn_track {
        track.record_spawn_rollback(&agent_id);
    }
    if spawn_ok {
        let summary = tracker
            .last_finish_summary
            .as_deref()
            .unwrap_or("(spawn completed)")
            .to_string();
        let handoff = SpawnHandoff::from_tracker(&agent_id, summary, &tracker);
        let content = handoff.to_tool_content();
        ToolOutcome {
            ok: true,
            summary: format!("spawn finished ({})", handoff.agent_id),
            content,
            error_kind: None,
            change: None,
        }
    } else {
        let message = match turn_result {
            CandidateOutcome::Failed { message } => message,
            CandidateOutcome::Canceled { reason } => reason,
            CandidateOutcome::Limited { message, .. } => message,
            CandidateOutcome::Completed { summary, .. } => summary,
        };
        ToolOutcome::fail(message, "other")
    }
}

/// Format router line for logs (source + hard tier when set).
pub fn format_router_log(decision: &RouteDecision) -> String {
    let mut label = match decision.source {
        RouteSource::Llm => format!("llm→{}", route_label(decision)),
        _ => route_label(decision),
    };
    if decision.use_hard_tier {
        label.push_str(" (hard tier)");
    }
    label
}

/// Summarize post-turn orchestration telemetry for `event/log`.
pub fn format_orchestration_log(outcome: &OrchestratedTurnOutcome) -> String {
    let mut parts = vec![format!("model_tier={}", outcome.model_tier)];
    if outcome.critic_ran {
        parts.push("critic".into());
    }
    if outcome.revision_ran {
        parts.push("revision".into());
    }
    if outcome.spawn_agent_calls > 0 {
        parts.push(format!("spawn_calls={}", outcome.spawn_agent_calls));
    }
    parts.join(", ")
}

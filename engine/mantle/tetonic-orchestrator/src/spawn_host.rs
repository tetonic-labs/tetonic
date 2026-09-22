//! In-loop spawn driver (A13 v5) — reusable hook for nested spawns.
//! Integration coverage: `turn_tests::spawn_host_dispatch_carves_budget_and_respects_limits`.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use lokai_tools::ToolOutcome;
use tetonic_core::{Conversation, SpawnHook, SpawnRequest, Step};
use tetonic_domain::AgentInvocation;

use crate::handoff::carve_max_steps;
use crate::run::{spawn_depth, SpawnLimits};
use crate::spawn_budget::SpawnBudgetGate;
use crate::spawn_session::SpawnSessionTrack;
use crate::specialist::SpecialistPack;
use crate::turn::{run_spawned_specialist, AgentBuildRequest, ChildJob, RootExecute};

type AgentBuildFn = dyn FnMut(AgentBuildRequest, &str) -> Result<(tetonic_core::Agent, AgentInvocation), String>
    + Send;
type StepCallback = dyn for<'a> FnMut(&'a str, Step) + Send;

/// Shared spawn handler so nested agents reuse the same hook (no one-shot `take()`).
pub struct SpawnHost {
    pub limits: SpawnLimits,
    pub turn_spawn_count: Arc<AtomicU32>,
    pub spawn_serial: Arc<AtomicU32>,
    pub session_max_steps: Arc<AtomicUsize>,
    pub build: Arc<Mutex<AgentBuildFn>>,
    pub on_step: Arc<Mutex<StepCallback>>,
    pub spawn_track: Arc<SpawnSessionTrack>,
    /// Session id for ledger-keyed spawn reservations (H2-3).
    pub session_id: String,
    pub admit_spawn: Option<Arc<dyn SpawnBudgetGate>>,
    pub pack: Arc<dyn SpecialistPack>,
    pub child_job: Arc<dyn ChildJob>,
    pub root_execute: Arc<dyn RootExecute>,
}

impl SpawnHost {
    pub fn hook(self: &Arc<Self>) -> SpawnHook {
        let host = self.clone();
        Box::new(move |req, convo| {
            let host = host.clone();
            Box::pin(async move { host.dispatch(req, convo).await })
        })
    }

    pub async fn dispatch(
        self: &Arc<Self>,
        req: SpawnRequest,
        conversation: &mut Conversation,
    ) -> ToolOutcome {
        if self.turn_spawn_count.load(Ordering::SeqCst) >= self.limits.max_per_turn {
            return ToolOutcome::fail(
                format!(
                    "spawn budget exhausted ({}/{})",
                    self.turn_spawn_count.load(Ordering::SeqCst),
                    self.limits.max_per_turn
                ),
                "other",
            );
        }

        let Some(role) = self.pack.parse(&req.role) else {
            return ToolOutcome::fail(format!("unknown spawn role '{}'", req.role), "other");
        };

        let parent_agent_id = match self
            .spawn_track
            .resolve_spawn_parent(Some(&req.parent_agent_id))
        {
            Ok(p) => p,
            Err(e) => return ToolOutcome::fail(e, "other"),
        };
        let mut req = req;
        req.parent_agent_id = parent_agent_id;

        let depth = spawn_depth(&req.parent_agent_id);
        if depth > self.limits.max_depth {
            return ToolOutcome::fail(
                format!(
                    "spawn depth {depth} exceeds limit {}",
                    self.limits.max_depth
                ),
                "other",
            );
        }

        let carved = carve_max_steps(self.session_max_steps.load(Ordering::SeqCst), depth);
        let can_nest = depth < self.limits.max_depth;
        let nested_hook = if can_nest { Some(self.hook()) } else { None };

        let host = self.clone();
        let on_step = self.on_step.clone();
        let outcome = run_spawned_specialist(
            conversation,
            req,
            self.spawn_serial.clone(),
            self.turn_spawn_count.clone(),
            self.limits,
            move |build, user_input| {
                let mut b = build;
                b.max_steps = Some(carved);
                (host.build.lock().unwrap())(b, user_input)
            },
            move |aid, step| on_step.lock().unwrap()(aid, step),
            nested_hook,
            role,
            Some(self.spawn_track.clone()),
            self.session_id.clone(),
            self.admit_spawn.clone(),
            self.child_job.clone(),
            self.root_execute.clone(),
            None,
        )
        .await;
        outcome
    }
}

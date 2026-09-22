//! H2-3: ledger capacity refuses spawn before a child Agent is built.

#![allow(clippy::field_reassign_with_default)]

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use chrono::Utc;
use lokai_app::coding_pack::CodingPack;
use lokai_broker::{
    BudgetLimits, HierarchicalBudgetLedger, ReservationState, ReservationTarget, ResourceRequest,
};
use lokai_core::{Agent, AgentConfig, Conversation, SpawnRequest, Step};
use lokai_domain::ids::{AttemptId, ReservationId, RunId, TaskId};
use lokai_domain::{AgentInvocation, CandidateOutcome};
use lokai_orchestrator::{
    run_spawned_specialist, AgentBuildRequest, ChildAdmit, ChildJob, RoleId, RootExecute,
    SpawnAdmitCtx, SpawnAdmitError, SpawnBudgetGate, SpawnHost, SpawnLimits, SpawnReservationToken,
    SpawnSessionTrack, ROOT_AGENT,
};
use lokai_tools::{Tools, Workspace};

type AgentBuildFn =
    dyn FnMut(AgentBuildRequest, &str) -> Result<(Agent, AgentInvocation), String> + Send;

struct TestRootExecute;

#[async_trait::async_trait]
impl RootExecute for TestRootExecute {
    async fn execute(
        &self,
        _attempt_id: AttemptId,
        agent: &mut Agent,
        conversation: &mut Conversation,
        invocation: AgentInvocation,
        on_step: &mut (dyn FnMut(Step) + Send),
    ) -> CandidateOutcome {
        agent.turn(conversation, invocation, on_step).await
    }
}

struct TestChildJob;

#[async_trait::async_trait]
impl ChildJob for TestChildJob {
    async fn admit_child(&self, req: ChildAdmit) -> Result<AttemptId, String> {
        Ok(AttemptId::new(format!("att_child_{}", req.agent_id)))
    }

    async fn complete_child(
        &self,
        _attempt_id: AttemptId,
        _outcome: CandidateOutcome,
    ) -> Result<(), String> {
        Ok(())
    }
}

struct RecordingGate {
    budgets: Arc<HierarchicalBudgetLedger>,
    run_id: RunId,
}

impl SpawnBudgetGate for RecordingGate {
    fn admit(&self, ctx: &SpawnAdmitCtx) -> Result<SpawnReservationToken, SpawnAdmitError> {
        let task_id = TaskId::new(format!("task_spawn_{}", ctx.child_agent_id));
        let attempt_id = AttemptId::new(format!("att_spawn_{}", ctx.child_agent_id));
        match self.budgets.reserve(
            &self.run_id,
            &task_id,
            &attempt_id,
            None,
            ReservationTarget::Local,
            &ResourceRequest::spawned_agent_default(),
            false,
            Utc::now(),
        ) {
            Ok(r) => Ok(SpawnReservationToken {
                id: r.reservation_id.0,
            }),
            Err(_) => Err(SpawnAdmitError::Refused {
                reason: "ledger at capacity (inference concurrency)".into(),
            }),
        }
    }

    fn release(&self, token: &SpawnReservationToken) {
        self.budgets.release(
            &ReservationId::new(token.id.clone()),
            ReservationState::Released,
        );
    }

    fn release_session(&self) {
        let _ = self
            .budgets
            .release_for_run(&self.run_id, ReservationState::Canceled);
    }
}

fn tight_ledger() -> Arc<HierarchicalBudgetLedger> {
    let mut limits = BudgetLimits::default();
    limits.max_concurrent_inference = 1;
    Arc::new(HierarchicalBudgetLedger::new(limits))
}

#[tokio::test(flavor = "multi_thread")]
async fn ledger_full_refuses_before_child_is_built() {
    let budgets = tight_ledger();
    budgets
        .reserve(
            &RunId::new("run_h23"),
            &TaskId::new("task_hold"),
            &AttemptId::new("att_hold"),
            None,
            ReservationTarget::Local,
            &ResourceRequest::spawned_agent_default(),
            false,
            Utc::now(),
        )
        .unwrap();

    let built = Arc::new(AtomicUsize::new(0));
    let gate: Arc<dyn SpawnBudgetGate> = Arc::new(RecordingGate {
        budgets: budgets.clone(),
        run_id: RunId::new("run_h23"),
    });

    let built2 = built.clone();
    let mut conversation = Conversation::new();
    let outcome = run_spawned_specialist(
        &mut conversation,
        SpawnRequest {
            parent_agent_id: ROOT_AGENT.to_string(),
            role: "coder".into(),
            task: "should not run".into(),
        },
        Arc::new(AtomicU32::new(0)),
        Arc::new(AtomicU32::new(0)),
        SpawnLimits::default(),
        |_build: AgentBuildRequest, _user_input: &str| {
            built2.fetch_add(1, Ordering::SeqCst);
            Err("must not build".into())
        },
        |_aid, _step: Step| {},
        None,
        RoleId::new("coder"),
        None,
        "sess_h23".into(),
        Some(gate),
        TestChildJob,
        TestRootExecute,
        None,
    )
    .await;

    assert!(!outcome.ok, "summary={}", outcome.summary);
    assert!(
        outcome.summary.contains("spawn_refused"),
        "got {}",
        outcome.summary
    );
    assert_eq!(
        built.load(Ordering::SeqCst),
        0,
        "child Agent must not be built"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn child_build_failure_releases_reservation() {
    let budgets = tight_ledger();
    let gate: Arc<dyn SpawnBudgetGate> = Arc::new(RecordingGate {
        budgets: budgets.clone(),
        run_id: RunId::new("run_fail"),
    });

    let mut conversation = Conversation::new();
    let outcome = run_spawned_specialist(
        &mut conversation,
        SpawnRequest {
            parent_agent_id: ROOT_AGENT.to_string(),
            role: "coder".into(),
            task: "x".into(),
        },
        Arc::new(AtomicU32::new(0)),
        Arc::new(AtomicU32::new(0)),
        SpawnLimits::default(),
        |_build: AgentBuildRequest, _user_input: &str| Err("build boom".into()),
        |_aid, _step| {},
        None,
        RoleId::new("coder"),
        None,
        "sess_fail".into(),
        Some(gate),
        TestChildJob,
        TestRootExecute,
        None,
    )
    .await;

    assert!(!outcome.ok);
    assert_eq!(
        budgets.active_count(),
        0,
        "spawn reservation must not leak after build failure"
    );
}

#[test]
fn cancel_releases_descendant_reservations() {
    let budgets = tight_ledger();
    let gate = RecordingGate {
        budgets: budgets.clone(),
        run_id: RunId::new("run_cancel"),
    };
    let tok = gate
        .admit(&SpawnAdmitCtx {
            session_id: "sess_cancel".into(),
            parent_agent_id: "a0".into(),
            child_agent_id: "a0_s0".into(),
            depth: 1,
        })
        .unwrap();
    assert_eq!(budgets.active_count(), 1);
    gate.release_session();
    assert_eq!(budgets.active_count(), 0);
    gate.release(&tok);
    assert_eq!(budgets.active_count(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn spawn_limits_still_cap_when_ledger_would_allow() {
    let budgets = Arc::new(HierarchicalBudgetLedger::new(BudgetLimits::default()));
    let built = Arc::new(AtomicUsize::new(0));
    let gate: Arc<dyn SpawnBudgetGate> = Arc::new(RecordingGate {
        budgets,
        run_id: RunId::new("run_limits"),
    });

    let turn_spawn_count = Arc::new(AtomicU32::new(0));
    let spawn_serial = Arc::new(AtomicU32::new(0));
    let dir = tempfile::tempdir().unwrap();
    let ws_path = dir.path().to_path_buf();
    let built2 = built.clone();

    // Provider that immediately finishes so the first spawn completes.
    let provider = Arc::new(FinishOnceProvider::default());
    let provider2 = provider.clone();
    let build: Arc<Mutex<AgentBuildFn>> = Arc::new(Mutex::new(
        move |build: AgentBuildRequest, user_input: &str| {
            built2.fetch_add(1, Ordering::SeqCst);
            let ws = Workspace::new(&ws_path).map_err(|e| e.to_string())?;
            let tools = Tools::new(ws, true);
            let invocation = AgentInvocation {
                instructions: "You are a test agent in the workspace.".into(),
                user_input: user_input.to_string(),
                explain_turn: false,
                empty_tool_nudge: false,
                max_steps: 2,
                completion_tool: "finish".into(),
                discipline: lokai_domain::LoopDiscipline::default(),
            };
            Ok((
                Agent::new(
                    provider2.clone(),
                    tools,
                    AgentConfig {
                        agent_id: build.agent_id,
                        max_steps: 2,
                        ..AgentConfig::default()
                    },
                ),
                invocation,
            ))
        },
    ));

    let host = Arc::new(SpawnHost {
        limits: SpawnLimits {
            max_per_turn: 1,
            max_depth: 2,
        },
        turn_spawn_count: turn_spawn_count.clone(),
        spawn_serial,
        session_max_steps: Arc::new(AtomicUsize::new(8)),
        build,
        on_step: Arc::new(Mutex::new(
            Box::new(|_a: &str, _s: Step| {}) as Box<dyn for<'a> FnMut(&'a str, Step) + Send>
        )),
        spawn_track: SpawnSessionTrack::new(),
        session_id: "sess_limits".into(),
        admit_spawn: Some(gate),
        pack: CodingPack::arc(),
        child_job: Arc::new(TestChildJob),
        root_execute: Arc::new(TestRootExecute),
    });

    let mut conversation = Conversation::new();
    let _first = host
        .dispatch(
            SpawnRequest {
                parent_agent_id: ROOT_AGENT.to_string(),
                role: "coder".into(),
                task: "one".into(),
            },
            &mut conversation,
        )
        .await;

    let second = host
        .dispatch(
            SpawnRequest {
                parent_agent_id: ROOT_AGENT.to_string(),
                role: "coder".into(),
                task: "two".into(),
            },
            &mut conversation,
        )
        .await;
    assert!(
        !second.ok && second.summary.contains("spawn budget exhausted"),
        "SpawnLimits must still bind; got {}",
        second.summary
    );
    assert_eq!(turn_spawn_count.load(Ordering::SeqCst), 1);
}

/// Minimal provider: one assistant `finish` tool call then empty.
struct FinishOnceProvider {
    done: Mutex<bool>,
}

impl Default for FinishOnceProvider {
    fn default() -> Self {
        Self {
            done: Mutex::new(false),
        }
    }
}

#[async_trait::async_trait]
impl lokai_inference::InferenceProvider for FinishOnceProvider {
    async fn chat(
        &self,
        _req: lokai_inference::ChatRequest,
        _on_token: &mut lokai_inference::TokenSink<'_>,
    ) -> Result<lokai_inference::ChatResponse, lokai_inference::InferenceError> {
        let mut done = self.done.lock().unwrap();
        if !*done {
            *done = true;
            return Ok(lokai_inference::ChatResponse {
                message: lokai_inference::Message::assistant("").with_tool_calls(vec![
                    lokai_inference::ToolCall {
                        function: lokai_inference::FunctionCall {
                            name: "finish".into(),
                            arguments: serde_json::json!({"summary": "ok"}),
                        },
                    },
                ]),
                usage: Default::default(),
                provenance: Default::default(),
            });
        }
        Ok(lokai_inference::ChatResponse {
            message: lokai_inference::Message::assistant("done"),
            usage: Default::default(),
            provenance: Default::default(),
        })
    }
}

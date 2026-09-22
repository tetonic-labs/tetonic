//! Live session registry owned by `lokai-app` (R1-1).
//!
//! Conversation, cancel, and spawn tracking live here.
//! Transport adapters (`lokaid`, CLI) hold RPC/stdio handles only.
//! Approval parking lives on `ApprovalService` (PORTAL-01).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use tetonic_core::Conversation;
use tetonic_domain::{RunId, TaskId, WorkspaceBinding};
use tetonic_memory::RecoverMutex;
use tetonic_orchestrator::{OrchestrationMode, SessionStartPlan, SpawnSessionTrack};

use crate::errors::AppError;

struct TurnState {
    context_compilers: Vec<(
        tetonic_domain::SessionId,
        std::sync::Weak<dyn tetonic_domain::ContextCompiler>,
    )>,
    in_flight: bool,
    closing: bool,
    inference: crate::inference_selection::InferenceSelection,
}

pub struct LiveSession {
    conversation: Mutex<Option<Conversation>>,
    pub cancel: Arc<AtomicBool>,
    turn_state: Mutex<TurnState>,
    pub orchestration: OrchestrationMode,
    pub critic_enabled: bool,
    pub llm_router: bool,
    pub session_max_steps: usize,
    pub explicit_hard_tier: bool,
    pub allow_shell: bool,
    pub force_explain: bool,
    pub auto_grant_approvals: bool,
    plan: Mutex<SessionStartPlan>,
    pub tool_workspace: PathBuf,
    /// How this session's workspace is materialized (M1 seam). Today always a local
    /// filesystem root; `tool_workspace` is that root as a bare path.
    pub workspace_binding: WorkspaceBinding,
    pub spawn_serial: AtomicU32,
    pub spawn_track: Arc<SpawnSessionTrack>,
    turn_spawn_count: AtomicU32,
    current_run_id: Mutex<Option<RunId>>,
    last_run_id: Mutex<Option<RunId>>,
    root_task_id: Mutex<Option<TaskId>>,
}

impl LiveSession {
    pub fn inference_selection(&self) -> crate::inference_selection::InferenceSelection {
        self.turn_state.lock_recover().inference.clone()
    }

    pub(crate) fn change_inference(
        &self,
        command: crate::inference_selection::ChangeInferenceCommand,
    ) -> Result<crate::inference_selection::InferenceSelection, AppError> {
        let mut state = self.turn_state.lock_recover();
        if state.closing || state.in_flight {
            return Err(AppError::InvalidRequest("inference can only change while the session is idle; finish or cancel the active turn first".into()));
        }
        let revision = state.inference.revision;
        if revision != command.expected_revision {
            return Err(AppError::InvalidRequest(
                "inference selection changed; refresh before retrying".into(),
            ));
        }
        let selection = crate::inference_selection::InferenceSelection {
            profile: command.profile,
            model_fast: command.model_fast,
            model_hard: command.model_hard,
            revision: revision
                .checked_add(1)
                .ok_or_else(|| AppError::InvalidRequest("inference revision exhausted".into()))?,
        };
        // Token counts are model/tokenizer-specific. Preserve content and identity,
        // but never carry cached counts across an inference change.
        let mut conversation = self.conversation.lock_recover();
        let conversation = conversation.as_mut().ok_or(AppError::SessionConflict)?;
        conversation.invalidate_token_counts();
        state.inference = selection.clone();
        Ok(selection)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        conversation: Conversation,
        orchestration: OrchestrationMode,
        critic_enabled: bool,
        llm_router: bool,
        session_max_steps: usize,
        model_fast: String,
        model_hard: String,
        explicit_hard_tier: bool,
        plan: SessionStartPlan,
        tool_workspace: PathBuf,
        allow_shell: bool,
        force_explain: bool,
        auto_grant_approvals: bool,
    ) -> Arc<Self> {
        let cancel = conversation.cancel_handle();
        Arc::new(Self {
            conversation: Mutex::new(Some(conversation)),
            cancel,
            turn_state: Mutex::new(TurnState {
                context_compilers: Vec::new(),
                in_flight: false,
                closing: false,
                inference: crate::inference_selection::InferenceSelection {
                    profile: "default".into(),
                    model_fast,
                    model_hard,
                    revision: 0,
                },
            }),
            orchestration,
            critic_enabled,
            llm_router,
            session_max_steps,
            explicit_hard_tier,
            allow_shell,
            force_explain,
            auto_grant_approvals,
            plan: Mutex::new(plan),
            workspace_binding: WorkspaceBinding::local(tool_workspace.clone()),
            tool_workspace,
            spawn_serial: AtomicU32::new(0),
            spawn_track: SpawnSessionTrack::new(),
            turn_spawn_count: AtomicU32::new(0),
            current_run_id: Mutex::new(None),
            last_run_id: Mutex::new(None),
            root_task_id: Mutex::new(None),
        })
    }

    pub fn plan(&self) -> SessionStartPlan {
        self.plan.lock_recover().clone()
    }

    pub fn set_plan_data_class(&self, class: tetonic_domain::DataClass) {
        self.plan.lock_recover().data_class = class;
    }

    pub fn take_conversation(&self) -> Result<Conversation, AppError> {
        tetonic_memory::mutex_lock(&self.conversation)
            .take()
            .ok_or(AppError::SessionConflict)
    }

    pub fn restore_conversation(&self, conversation: Conversation) {
        *self.conversation.lock_recover() = Some(conversation);
    }

    pub(crate) fn register_context_compiler(
        &self,
        session: tetonic_domain::SessionId,
        compiler: &Arc<dyn tetonic_domain::ContextCompiler>,
    ) -> Result<(), AppError> {
        let mut state = self.turn_state.lock_recover();
        if state.closing {
            compiler
                .invalidate_session(&session)
                .map_err(AppError::InvalidRequest)?;
            return Err(AppError::InvalidRequest("session is closing".into()));
        }
        state
            .context_compilers
            .retain(|(_, compiler)| compiler.strong_count() > 0);
        if state.context_compilers.len() >= 128 {
            return Err(AppError::InvalidRequest(
                "session context compiler limit reached".into(),
            ));
        }
        state
            .context_compilers
            .push((session, Arc::downgrade(compiler)));
        Ok(())
    }

    pub(crate) fn begin_close(&self) -> Result<(), AppError> {
        let mut state = self.turn_state.lock_recover();
        state.closing = true;
        for (session, compiler) in &state.context_compilers {
            if let Some(compiler) = compiler.upgrade() {
                compiler
                    .invalidate_session(session)
                    .map_err(AppError::InvalidRequest)?;
            }
        }
        state.context_compilers.clear();
        Ok(())
    }

    pub fn try_begin_turn(&self) -> Result<(), AppError> {
        let mut state = self.turn_state.lock_recover();
        if state.closing {
            return Err(AppError::InvalidRequest("session is closing".into()));
        }
        if state.in_flight {
            return Err(AppError::InvalidRequest(
                "a turn is already running for this session".into(),
            ));
        }
        // Reset the previous turn's cancellation before admitting the new turn.
        // Serialize with request_cancel so a new cancellation cannot be lost.
        self.cancel.store(false, Ordering::SeqCst);
        state.in_flight = true;
        Ok(())
    }

    pub fn end_turn(&self) {
        self.turn_state.lock_recover().in_flight = false;
    }

    pub fn turn_in_flight(&self) -> bool {
        self.turn_state.lock_recover().in_flight
    }

    pub fn request_cancel(&self) {
        let _state = self.turn_state.lock_recover();
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn set_current_run(&self, run_id: RunId, root_task_id: TaskId) {
        *self.current_run_id.lock_recover() = Some(run_id.clone());
        *self.last_run_id.lock_recover() = Some(run_id);
        *self.root_task_id.lock_recover() = Some(root_task_id);
    }

    pub fn clear_current_run(&self) {
        *self.current_run_id.lock_recover() = None;
    }

    pub fn current_run_id(&self) -> Option<RunId> {
        self.current_run_id.lock_recover().clone()
    }

    pub fn root_task_id(&self) -> Option<TaskId> {
        self.root_task_id.lock_recover().clone()
    }

    pub fn last_run_id(&self) -> Option<RunId> {
        self.last_run_id.lock_recover().clone()
    }

    pub fn turn_spawn_count(&self) -> u32 {
        self.turn_spawn_count.load(Ordering::Relaxed)
    }

    pub fn store_turn_spawn_count(&self, n: u32) {
        self.turn_spawn_count.store(n, Ordering::Relaxed);
    }

    #[cfg(test)]
    pub(crate) fn poison_conversation_for_test(&self) {
        let _g = self.conversation.lock_recover();
        panic!("intentional poison");
    }
}

/// Process-wide live session map. `Application` is `Send + Sync`.
pub struct SessionLiveStore {
    inner: Mutex<HashMap<String, Arc<LiveSession>>>,
}

impl SessionLiveStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    pub fn insert(&self, session_id: String, live: Arc<LiveSession>) -> Result<(), AppError> {
        let mut map = self.inner.lock_recover();
        if map.contains_key(&session_id) {
            return Err(AppError::SessionConflict);
        }
        map.insert(session_id, live);
        Ok(())
    }

    pub fn get(&self, session_id: &str) -> Option<Arc<LiveSession>> {
        self.inner.lock_recover().get(session_id).cloned()
    }

    pub fn remove(&self, session_id: &str) -> Option<Arc<LiveSession>> {
        self.inner.lock_recover().remove(session_id)
    }

    pub fn contains(&self, session_id: &str) -> bool {
        self.inner.lock_recover().contains_key(session_id)
    }

    pub fn len(&self) -> usize {
        self.inner.lock_recover().len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock_recover().is_empty()
    }

    pub fn any_turn_in_flight(&self) -> bool {
        self.inner
            .lock_recover()
            .values()
            .any(|s| s.turn_in_flight())
    }

    pub fn all(&self) -> Vec<Arc<LiveSession>> {
        self.inner.lock_recover().values().cloned().collect()
    }

    pub fn all_pairs(&self) -> Vec<(String, Arc<LiveSession>)> {
        self.inner
            .lock_recover()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

impl Default for SessionLiveStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Chat-turn admission (R1-1 / H2-1). Draining and optimize-in-progress are
/// transport flags; VRAM doctor is a kernel warning; in-flight is live-session
/// authority.
///
/// **Degraded capacity policy (H2-1):** warn and proceed. Inference still
/// fail-closes on actual VRAM spill. Optimize-in-progress stays a hard reject
/// (`CapacityBusy`), distinct from doctor gates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnAdmitError {
    Draining,
    CapacityBusy,
    TurnInFlight,
    /// Hard refuse on doctor gates. Production never emits this (warn-and-proceed).
    CapacityGate {
        doctor: String,
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapacityGateWarning {
    pub doctor: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnAdmitDecision {
    Admit {
        capacity_warning: Option<CapacityGateWarning>,
    },
    Reject(TurnAdmitError),
}

/// Build the H2-1 capacity warning from saved doctor state (no live Ollama probe).
pub fn capacity_gate_warning(
    session_model: &str,
    status: &tetonic_capacity::CapacityStatus,
) -> Option<CapacityGateWarning> {
    if status.profile_model.is_none() && status.profile_base_model.is_none() {
        return None;
    }
    let doctor = tetonic_capacity::doctor_status_str(status.doctor);
    let same = tetonic_capacity::session_matches_profile(
        session_model,
        status.profile_model.as_deref(),
        status.profile_base_model.as_deref(),
    );
    if !same {
        let profile = status.profile_model.as_deref()?;
        return Some(CapacityGateWarning {
            doctor: doctor.to_string(),
            detail: format!(
                "capacity: this chat uses `{session_model}`; saved profile still defaults to `{profile}`. Not blocking the turn."
            ),
        });
    }
    if !status.gates_ok {
        return Some(CapacityGateWarning {
            doctor: doctor.to_string(),
            detail: format!(
                "capacity: saved profile for `{session_model}` is {doctor}. Chat continues; inference will abort if this model spills VRAM."
            ),
        });
    }
    None
}

pub fn admit_chat_turn(
    draining: bool,
    capacity_busy: bool,
    live: &LiveSession,
    capacity: Option<&tetonic_capacity::CapacityStatus>,
) -> TurnAdmitDecision {
    if draining {
        return TurnAdmitDecision::Reject(TurnAdmitError::Draining);
    }
    if capacity_busy {
        return TurnAdmitDecision::Reject(TurnAdmitError::CapacityBusy);
    }
    let capacity_warning = capacity
        .and_then(|status| capacity_gate_warning(&live.inference_selection().model_fast, status));
    if live.try_begin_turn().is_err() {
        return TurnAdmitDecision::Reject(TurnAdmitError::TurnInFlight);
    }
    TurnAdmitDecision::Admit { capacity_warning }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tetonic_domain::{DataClass, DisclosureTier};
    use tetonic_orchestrator::SessionStartPlan;

    fn empty_plan() -> SessionStartPlan {
        SessionStartPlan {
            data_class: DataClass::default(),
            disclosure_tier: DisclosureTier::default(),
            briefing: None,
            project_context: None,
            verify_cmd: None,
        }
    }

    fn live() -> Arc<LiveSession> {
        LiveSession::new(
            Conversation::new(),
            OrchestrationMode::Single,
            false,
            false,
            16,
            "fast".into(),
            "hard".into(),
            false,
            empty_plan(),
            PathBuf::from("."),
            false,
            false,
            false,
        )
    }

    #[test]
    fn workspace_binding_is_local_materialization_of_tool_workspace() {
        let s = live();
        assert_eq!(
            s.workspace_binding,
            WorkspaceBinding::local(PathBuf::from("."))
        );
        assert_eq!(s.workspace_binding.root(), s.tool_workspace.as_path());
    }

    #[test]
    fn inference_switch_is_idle_atomic_and_revision_checked() {
        use crate::inference_selection::ChangeInferenceCommand;
        let session = live();
        let other = live();
        let change = ChangeInferenceCommand {
            session_id: "test".into(),
            profile: "default".into(),
            model_fast: "new-fast".into(),
            model_hard: "new-hard".into(),
            expected_revision: 0,
        };
        session.try_begin_turn().unwrap();
        assert!(session.change_inference(change.clone()).is_err());
        assert_eq!(session.inference_selection().revision, 0);
        session.end_turn();
        let updated = session.change_inference(change.clone()).unwrap();
        assert_eq!(updated.model_fast, "new-fast");
        assert_eq!(updated.revision, 1);
        assert!(session.change_inference(change.clone()).is_err());
        assert_eq!(other.inference_selection().revision, 0);
        session.begin_close().unwrap();
        assert!(session
            .change_inference(ChangeInferenceCommand {
                expected_revision: 1,
                ..change
            })
            .is_err());
    }

    #[test]
    fn closing_revokes_registered_compilers_and_rejects_late_registration() {
        struct Compiler(Arc<AtomicU32>);
        #[async_trait::async_trait]
        impl tetonic_domain::ContextCompiler for Compiler {
            fn invalidate_session(
                &self,
                session: &tetonic_domain::SessionId,
            ) -> Result<(), String> {
                assert_eq!(session.0, "session-owner");
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
            async fn compile(
                &self,
                _: tetonic_domain::ContextCompileRequest,
            ) -> Result<tetonic_domain::CompiledContext, String> {
                Err("not used".into())
            }
        }
        let session = live();
        let calls = Arc::new(AtomicU32::new(0));
        let compiler: Arc<dyn tetonic_domain::ContextCompiler> = Arc::new(Compiler(calls.clone()));
        session
            .register_context_compiler(tetonic_domain::SessionId::new("session-owner"), &compiler)
            .unwrap();
        session.begin_close().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(session
            .register_context_compiler(tetonic_domain::SessionId::new("session-owner"), &compiler)
            .is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(session.try_begin_turn().is_err());
    }

    #[test]
    fn two_in_flight_turns_rejected() {
        let s = live();
        assert!(matches!(
            admit_chat_turn(false, false, &s, None),
            TurnAdmitDecision::Admit {
                capacity_warning: None
            }
        ));
        assert_eq!(
            admit_chat_turn(false, false, &s, None),
            TurnAdmitDecision::Reject(TurnAdmitError::TurnInFlight)
        );
        s.end_turn();
        assert!(matches!(
            admit_chat_turn(false, false, &s, None),
            TurnAdmitDecision::Admit {
                capacity_warning: None
            }
        ));
    }

    #[test]
    fn poisoned_conversation_mutex_does_not_deny_other_session() {
        let store = SessionLiveStore::new();
        let a = live();
        let b = live();
        store.insert("a".into(), a.clone()).unwrap();
        store.insert("b".into(), b.clone()).unwrap();
        let a_poison = a.clone();
        let join = std::thread::spawn(move || {
            a_poison.poison_conversation_for_test();
        });
        assert!(join.join().is_err());
        assert!(b.take_conversation().is_ok());
        assert!(store.get("b").is_some());
    }

    #[test]
    fn draining_and_capacity_deny_before_cas() {
        let s = live();
        assert_eq!(
            admit_chat_turn(true, false, &s, None),
            TurnAdmitDecision::Reject(TurnAdmitError::Draining)
        );
        assert_eq!(
            admit_chat_turn(false, true, &s, None),
            TurnAdmitDecision::Reject(TurnAdmitError::CapacityBusy)
        );
        assert!(!s.turn_in_flight());
    }

    fn degraded_status(profile: &str, base: &str) -> tetonic_capacity::CapacityStatus {
        tetonic_capacity::CapacityStatus {
            completed: true,
            stale: false,
            doctor: tetonic_capacity::CapacityDoctorStatus::Degraded,
            active_profile_id: Some("p1".into()),
            active_profile_label: Some("lab".into()),
            hardware_summary: None,
            gates_ok: false,
            last_setup_at: None,
            profile_model: Some(profile.into()),
            profile_base_model: Some(base.into()),
        }
    }

    #[test]
    fn degraded_profile_warns_and_admits() {
        let s = live();
        let status = degraded_status("qwen3.6-estate", "qwen3.6:latest");
        let a = admit_chat_turn(false, false, &s, Some(&status));
        s.end_turn();
        let b = admit_chat_turn(false, false, &s, Some(&status));
        assert_eq!(a, b);
        match a {
            TurnAdmitDecision::Admit {
                capacity_warning: Some(ref w),
            } => {
                assert_eq!(w.doctor, "degraded");
                assert!(w.detail.contains("Not blocking"));
            }
            other => panic!("expected admit-with-warning, got {other:?}"),
        }
        assert!(!matches!(
            a,
            TurnAdmitDecision::Reject(TurnAdmitError::CapacityBusy)
        ));
    }

    #[test]
    fn same_model_degraded_still_admits() {
        let s = LiveSession::new(
            Conversation::new(),
            OrchestrationMode::Single,
            false,
            false,
            16,
            "qwen3.6:latest".into(),
            "hard".into(),
            false,
            empty_plan(),
            PathBuf::from("."),
            false,
            false,
            false,
        );
        let status = degraded_status("qwen3.6-estate", "qwen3.6:latest");
        match admit_chat_turn(false, false, &s, Some(&status)) {
            TurnAdmitDecision::Admit {
                capacity_warning: Some(w),
            } => {
                assert!(w.detail.contains("spills VRAM"));
            }
            other => panic!("expected warn-and-proceed, got {other:?}"),
        }
    }

    #[test]
    fn healthy_profile_admits_without_warning() {
        let s = live();
        let status = tetonic_capacity::CapacityStatus {
            completed: true,
            stale: false,
            doctor: tetonic_capacity::CapacityDoctorStatus::Healthy,
            active_profile_id: Some("p1".into()),
            active_profile_label: Some("lab".into()),
            hardware_summary: None,
            gates_ok: true,
            last_setup_at: None,
            profile_model: Some("fast-estate".into()),
            profile_base_model: Some("fast".into()),
        };
        assert_eq!(
            admit_chat_turn(false, false, &s, Some(&status)),
            TurnAdmitDecision::Admit {
                capacity_warning: None
            }
        );
    }

    #[test]
    fn no_profile_does_not_warn() {
        let s = live();
        let status = tetonic_capacity::CapacityStatus::default();
        assert_eq!(
            admit_chat_turn(false, false, &s, Some(&status)),
            TurnAdmitDecision::Admit {
                capacity_warning: None
            }
        );
    }

    #[test]
    fn capacity_busy_is_not_a_doctor_warning() {
        let s = live();
        let status = degraded_status("qwen3.6-estate", "qwen3.6:latest");
        assert_eq!(
            admit_chat_turn(false, true, &s, Some(&status)),
            TurnAdmitDecision::Reject(TurnAdmitError::CapacityBusy)
        );
        let gate = TurnAdmitError::CapacityGate {
            doctor: "degraded".into(),
            detail: "refused".into(),
        };
        assert_ne!(TurnAdmitError::CapacityBusy, gate);
    }
}

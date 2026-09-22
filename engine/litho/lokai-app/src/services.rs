//! Application service implementations (lokai-app).
//!
//! Session authority (R1): `DefaultSessionService` + `SessionLiveStore` own
//! conversation, cancel, pending approvals, and spawn tracking.
//! `RunSupervisor` is the sole authority for run/task/attempt transitions.
//! Transport adapters (`lokaid`, CLI) keep in-flight RPC bookkeeping only.
//!
//! Approvals: grant/deny is recorded in the audit store (`lokai.db`) via
//! `DefaultApprovalService` — the single store both CLI and daemon read.
//! Run journal records task/attempt progress, not approval prompts.

#[path = "run_service.rs"]
mod run_service;
#[path = "run_service_hooks.rs"]
mod run_service_hooks;

pub use run_service::{
    DefaultRunService, FinalizationEffectDriver, FinalizationPolicy, RunService, RunTurnPlan,
};

use crate::approval::ApprovalService;
use crate::resume::{rehydrate_messages, RESUME_MESSAGE_CAP};
use crate::{commands::*, errors::AppError, events::*};
use async_trait::async_trait;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Slice 1 — Initialization
// ---------------------------------------------------------------------------

#[async_trait]
pub trait InitializationService: Send + Sync {
    async fn initialize_workspace(
        &self,
        cmd: InitializeCommand,
    ) -> Result<InitializeResultPayload, AppError>;
    fn bootstrap_runtime(
        &self,
        cmd: &InitializeCommand,
        store: &Option<lokai_memory::SharedStore>,
    ) -> Result<InitializeBootstrapPayload, AppError>;
}

pub struct DefaultInitializationService {}

impl Default for DefaultInitializationService {
    fn default() -> Self {
        Self::new()
    }
}

impl DefaultInitializationService {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl InitializationService for DefaultInitializationService {
    async fn initialize_workspace(
        &self,
        cmd: InitializeCommand,
    ) -> Result<InitializeResultPayload, AppError> {
        let path = std::path::Path::new(&cmd.workspace_root)
            .canonicalize()
            .map_err(|e| AppError::InvalidRequest(format!("workspace: {}", e)))?;
        Ok(InitializeResultPayload {
            workspace_root: path.display().to_string(),
            index_db_path: None,
        })
    }

    fn bootstrap_runtime(
        &self,
        cmd: &InitializeCommand,
        store: &Option<lokai_memory::SharedStore>,
    ) -> Result<InitializeBootstrapPayload, AppError> {
        let path = std::path::Path::new(&cmd.workspace_root)
            .canonicalize()
            .map_err(|e| AppError::InvalidRequest(format!("workspace: {}", e)))?;
        let policy = match store {
            Some(s) => s
                .read_sync(|db| lokai_runtime::load_policy_engine(Some(db)))
                .unwrap_or_else(|_| lokai_runtime::load_policy_engine(None)),
            None => lokai_runtime::load_policy_engine(None),
        };
        let local_artifacts = lokai_artifact::LocalArtifactStore::new(
            path.join(".lokai").join("artifacts"),
            crate::secret_scanner_factory::artifact_scan_policy(store),
        )
        .map_err(|e| AppError::PersistenceFailed(format!("artifact store: {e}")))?;
        // R4-2 AC4: GC/quota runs on CLI and daemon initialize (shared bootstrap).
        if let Err(e) =
            lokai_artifact::enforce_at_startup(&local_artifacts, local_artifacts.quota())
        {
            tracing::warn!("artifact GC at bootstrap: {e}");
        }
        let artifact_store = Arc::new(local_artifacts);

        let runtime = Arc::new(
            lokai_runtime::EngineRuntime::new_with_capability_store(
                policy.clone(),
                None,
                artifact_store,
                store.as_ref().map(|s| Arc::new(s.clone())),
            )
            .map_err(|e| AppError::PersistenceFailed(format!("capability store: {e}")))?,
        );
        let inference_defaults = match store {
            Some(s) => s
                .read_sync(|db| {
                    lokai_capacity::load_inference_defaults(db, lokai_capacity::LOCAL_NODE_ID)
                })
                .unwrap_or_else(|_| lokai_capacity::InferenceDefaults::fallback()),
            None => lokai_capacity::InferenceDefaults::fallback(),
        };
        Ok(InitializeBootstrapPayload {
            workspace_root: path.display().to_string(),
            policy,
            runtime,
            inference_defaults,
        })
    }
}

// ---------------------------------------------------------------------------
// Slice 2 — Session lifecycle
// ---------------------------------------------------------------------------

#[async_trait]
pub trait SessionService: Send + Sync + SessionLifecycle {
    async fn start_session(
        &self,
        cmd: StartSessionCommand,
    ) -> Result<StartSessionResultPayload, AppError>;
    fn live(&self, session_id: &str) -> Result<Arc<crate::session_live::LiveSession>, AppError>;
    fn has_live(&self, session_id: &str) -> bool;
    fn live_count(&self) -> usize;
    fn any_turn_in_flight(&self) -> bool;
    async fn cancel_all(&self);
}

#[async_trait]
pub trait SessionLifecycle: Send + Sync {
    fn end_session(&self, cmd: EndSessionCommand) -> Result<(), AppError>;
    async fn cancel_session(&self, cmd: CancelRunCommand) -> Result<(), AppError>;
    fn consolidate_session(
        &self,
        cmd: ConsolidateSessionCommand,
    ) -> Result<ConsolidateSessionResultPayload, AppError>;
}

pub struct DefaultSessionService {
    policy: Arc<lokai_policy::PolicyEngine>,
    store: Option<lokai_memory::SharedStore>,
    index_db: Option<std::path::PathBuf>,
    fabric_hint: Option<String>,
    events: Arc<dyn ApplicationEventSink>,
    live: Arc<crate::session_live::SessionLiveStore>,
    #[allow(dead_code)]
    supervisor: Arc<dyn lokai_run::RunSupervisor>,
    runs: Arc<DefaultRunService>,
    approvals: Arc<dyn ApprovalService>,
}

impl DefaultSessionService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        policy: Arc<lokai_policy::PolicyEngine>,
        store: Option<lokai_memory::SharedStore>,
        index_db: Option<std::path::PathBuf>,
        fabric_hint: Option<String>,
        events: Arc<dyn ApplicationEventSink>,
        live: Arc<crate::session_live::SessionLiveStore>,
        supervisor: Arc<dyn lokai_run::RunSupervisor>,
        runs: Arc<DefaultRunService>,
        approvals: Arc<dyn ApprovalService>,
    ) -> Self {
        Self {
            policy,
            store,
            index_db,
            fabric_hint,
            events,
            live,
            supervisor,
            runs,
            approvals,
        }
    }
}

#[async_trait]
impl SessionService for DefaultSessionService {
    async fn start_session(
        &self,
        cmd: StartSessionCommand,
    ) -> Result<StartSessionResultPayload, AppError> {
        let resume = cmd.resume.unwrap_or(false);
        let explicit_hard_tier = cmd.model_tier.as_deref() == Some("hard");

        // ---- session-id resolution and optional resume hydration ----
        let (session_id, resumed, messages_loaded, messages, resume_state) = if resume {
            let shared = self.store.as_ref().ok_or_else(|| {
                AppError::InvalidRequest("session resume requires an audit store".into())
            })?;
            let cmd_session_id = cmd.session_id.clone();
            let cmd_workspace_root = cmd.workspace_root.clone();
            let (sid, messages_loaded, messages, resume_state, total) = shared
                .write(move |db| {
                    let sid = if let Some(ref id) = cmd_session_id {
                        id.clone()
                    } else {
                        db.find_latest_session_for_workspace(&cmd_workspace_root)
                            .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?
                            .ok_or_else(|| {
                                AppError::InvalidRequest("no prior session to resume".into())
                            })?
                    };

                    let ws = db
                        .session_workspace(&sid)
                        .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?
                        .ok_or_else(|| {
                            AppError::InvalidRequest(format!("unknown session_id `{sid}`"))
                        })?;
                    let active_ws = db.normalize_workspace(&cmd_workspace_root);
                    if ws != active_ws {
                        return Err(AppError::InvalidRequest(
                            "session workspace does not match initialized workspace".into(),
                        ));
                    }

                    let prior_status = db
                        .session_status(&sid)
                        .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?;
                    let resume_state = match prior_status.as_deref() {
                        Some("running") => {
                            let recovery = db
                                .get_turn_operation(&sid)
                                .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?
                                .and_then(|op| lokai_core::TurnState::from_str(&op.state))
                                .map(|s| s.requires_recovery())
                                .unwrap_or(false);
                            if recovery {
                                "recovery_required"
                            } else {
                                "incomplete"
                            }
                            .to_string()
                        }
                        _ => "continued".to_string(),
                    };
                    db.reopen_session(&sid)
                        .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?;
                    let total = db
                        .count_messages_for_resume(&sid)
                        .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?;
                    let rows = db
                        .list_messages_for_resume(&sid, RESUME_MESSAGE_CAP)
                        .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?;
                    let messages_loaded = rows.len() as u32;
                    let messages = rehydrate_messages(&rows, total);
                    Ok::<_, AppError>((sid, messages_loaded, messages, resume_state, total))
                })
                .await
                .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))??;
            tracing::info!(session_id = %sid, messages_loaded, total_eligible = total, %resume_state, "session resumed from audit");
            (sid, true, messages_loaded, messages, resume_state)
        } else {
            let sid = match &self.store {
                Some(s) => {
                    let root = cmd.workspace_root.clone();
                    s.write(move |db| db.start_session(&root, "single-agent", "mock"))
                        .await
                        .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?
                }
                None => Ok(lokai_memory::new_id("sess")),
            }
            .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?;
            (sid, false, 0, vec![], "fresh".to_string())
        };

        // ---- session host: data classification + briefing ----
        let briefing_enabled = if resumed {
            false
        } else {
            cmd.briefing.unwrap_or(true)
        };
        let workspace_root_clone = cmd.workspace_root.clone();

        let fabric_hint_for_fresh = if resumed {
            None
        } else {
            self.fabric_hint.as_deref()
        };

        let plan = if let Some(s) = &self.store {
            let session_id_clone = session_id.clone();
            let goal = cmd.goal.clone();
            let data_class = cmd.data_class.clone();
            let verify_cmd = cmd.verify_cmd.clone();
            let index_db = self.index_db.clone();
            let fabric_hint = fabric_hint_for_fresh.map(|h| h.to_string());
            let policy_clone = self.policy.clone();
            let ws_root = workspace_root_clone.clone();

            s.read(move |db| {
                let session_host = crate::coding_pack::product_session_host(&ws_root, policy_clone);
                session_host.on_session_start(
                    &session_id_clone,
                    goal.as_deref(),
                    data_class.as_deref(),
                    verify_cmd.as_deref(),
                    briefing_enabled,
                    fabric_hint.as_deref(),
                    Some(db),
                    index_db.as_deref(),
                )
            })
            .await
        } else {
            let session_host = crate::coding_pack::product_session_host(
                &workspace_root_clone,
                self.policy.clone(),
            );
            Ok(session_host.on_session_start(
                &session_id,
                cmd.goal.as_deref(),
                cmd.data_class.as_deref(),
                cmd.verify_cmd.as_deref(),
                briefing_enabled,
                fabric_hint_for_fresh,
                None,
                self.index_db.as_deref(),
            ))
        }
        .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?;

        if !resumed {
            if let Some(ref brief) = plan.briefing {
                tracing::info!(session_id = %session_id, chars = brief.len(), "session briefing ready");
                if let Some(s) = &self.store {
                    let payload = serde_json::json!({
                        "text": "session briefing prepared",
                        "chars": brief.len()
                    })
                    .to_string();
                    let event_session_id = session_id.clone();
                    let _ = s
                        .write(move |db| {
                            db.append_event(&event_session_id, "note", "system", &payload)
                        })
                        .await;
                }
            }
            if let Some(ref cmd_str) = plan.verify_cmd {
                tracing::info!("verify-before-finish: `{cmd_str}`");
            }
        }

        // ---- orchestration mode decision ----
        let orchestration_mode = cmd
            .orchestration
            .as_deref()
            .map(lokai_orchestrator::OrchestrationMode::parse)
            .unwrap_or(lokai_orchestrator::OrchestrationMode::Single);
        let critic_enabled = cmd
            .critic
            .unwrap_or(orchestration_mode == lokai_orchestrator::OrchestrationMode::Auto);
        let llm_router = cmd.llm_router.unwrap_or_else(|| {
            std::env::var("LOKAI_LLM_ROUTER")
                .ok()
                .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        });

        // ---- worktree + live session slot ----
        let tool_workspace = lokai_tools::ensure_session_worktree(
            std::path::Path::new(&cmd.workspace_root),
            &session_id,
        )
        .map_err(|e| AppError::InvalidRequest(format!("worktree: {e}")))?;

        let conversation = if resumed {
            lokai_core::Conversation::from_audit_messages(messages.clone())
        } else {
            lokai_core::Conversation::new()
        };
        const DEFAULT_SESSION_MAX_STEPS: usize = 16;
        let live = crate::session_live::LiveSession::new(
            conversation,
            orchestration_mode,
            critic_enabled,
            llm_router,
            cmd.session_max_steps.unwrap_or(DEFAULT_SESSION_MAX_STEPS),
            cmd.model_fast.clone().unwrap_or_default(),
            cmd.model_hard.clone().unwrap_or_default(),
            explicit_hard_tier,
            plan.clone(),
            tool_workspace,
            cmd.allow_shell.unwrap_or(false),
            cmd.force_explain.unwrap_or(false),
            cmd.auto_grant_approvals.unwrap_or(false),
        );
        self.live.insert(session_id.to_string(), live)?;

        let data_class = lokai_policy::data_class_name(plan.data_class).to_string();

        // R02: bind live session_id at session start (CLI + daemon share this path).
        lokai_telemetry::inject_session_context(&session_id);

        emit(
            &self.events,
            ApplicationEvent::SessionStarted {
                session_id: session_id.to_string(),
            },
        );

        Ok(StartSessionResultPayload {
            session_id: session_id.to_string(),
            data_class,
            resumed,
            messages_loaded,
            resume_state,
            plan,
            explicit_hard_tier,
            orchestration_mode,
            critic_enabled,
            llm_router,
            messages,
        })
    }

    fn live(&self, session_id: &str) -> Result<Arc<crate::session_live::LiveSession>, AppError> {
        self.live
            .get(session_id)
            .ok_or_else(|| AppError::SessionNotFound(session_id.to_string()))
    }

    fn has_live(&self, session_id: &str) -> bool {
        self.live.contains(session_id)
    }

    fn live_count(&self) -> usize {
        self.live.len()
    }

    fn any_turn_in_flight(&self) -> bool {
        self.live.any_turn_in_flight()
    }

    async fn cancel_all(&self) {
        for (session_id, live) in self.live.all_pairs() {
            live.request_cancel();
            self.approvals.fail_session_waits(&session_id);
            if let Some(run_id) = self.runs.active_run_for_session(&session_id) {
                let _ = self
                    .runs
                    .cancel_run(CancelByRunCommand {
                        run_id: run_id.to_string(),
                    })
                    .await;
            }
        }
    }
}

#[async_trait]
impl SessionLifecycle for DefaultSessionService {
    fn end_session(&self, cmd: EndSessionCommand) -> Result<(), AppError> {
        if let Some(live) = self.live.get(&cmd.session_id) {
            live.begin_close()?;
            if live.turn_in_flight() {
                return Err(AppError::InvalidRequest(
                    "session has active execution; use Application::close_session to cancel and drain it".into(),
                ));
            }
        }
        if let Some(live) = self.live.remove(&cmd.session_id) {
            live.request_cancel();
        }
        self.approvals.fail_session_waits(&cmd.session_id);
        let status = cmd.status.as_deref().unwrap_or("ok");
        if let Some(store) = &self.store {
            let session_id = cmd.session_id.clone();
            let status = status.to_string();
            let error = cmd.error.clone();
            let workspace_root = cmd.workspace_root.clone();
            let policy = self.policy.clone();
            store
                .write_sync(move |db| {
                    db.end_session(&session_id, &status, error.as_deref())
                        .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?;
                    let session_host = lokai_orchestrator::SessionHost::new(workspace_root, policy);
                    session_host.on_session_end(&session_id, Some(db));
                    Ok::<_, AppError>(())
                })
                .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))??;
        }
        lokai_tools::remove_session_worktree(
            std::path::Path::new(&cmd.workspace_root),
            &cmd.session_id,
        );
        let status = cmd.status.as_deref().unwrap_or("ok").to_string();
        emit(
            &self.events,
            ApplicationEvent::SessionEnded {
                session_id: cmd.session_id.clone(),
                status,
            },
        );
        Ok(())
    }

    async fn cancel_session(&self, cmd: CancelRunCommand) -> Result<(), AppError> {
        let live = self
            .live
            .get(&cmd.session_id)
            .ok_or_else(|| AppError::SessionNotFound(cmd.session_id.clone()))?;
        live.request_cancel();
        self.approvals.fail_session_waits(&cmd.session_id);
        let run_id = self.runs.active_run_for_session(&cmd.session_id);
        if let Some(ref r) = run_id {
            self.runs
                .cancel_run(CancelByRunCommand {
                    run_id: r.to_string(),
                })
                .await?;
        }
        emit(
            &self.events,
            ApplicationEvent::Cancellation {
                session_id: cmd.session_id.clone(),
                run_id: run_id.map(|r| r.to_string()),
                attempt_id: None,
            },
        );
        if cmd.pooled_cancel {
            // Pooled provider cancel is invoked by the transport adapter (infrastructure).
        }
        Ok(())
    }

    fn consolidate_session(
        &self,
        cmd: ConsolidateSessionCommand,
    ) -> Result<ConsolidateSessionResultPayload, AppError> {
        let store = self.store.as_ref().ok_or_else(|| {
            AppError::InvalidRequest("consolidate requires lokai.db store".into())
        })?;
        let digest_chars = store
            .write_sync({
                let session_id = cmd.session_id.clone();
                let workspace_root = cmd.workspace_root.clone();
                move |db| {
                    db.consolidate_session_explicit(&session_id)
                        .map_err(|e| AppError::PersistenceFailed(format!("consolidate: {e}")))?;
                    let digest_chars = db
                        .project_status(std::path::Path::new(&workspace_root))
                        .ok()
                        .flatten()
                        .map(|s| s.digest_chars);
                    if let Ok(payload) =
                        serde_json::to_string(&serde_json::json!({ "digest_chars": digest_chars }))
                    {
                        let _ = db.append_event(&session_id, "consolidate", "system", &payload);
                    }
                    Ok::<_, AppError>(digest_chars)
                }
            })
            .map_err(|e| AppError::PersistenceFailed(format!("consolidate: {e}")))??;
        Ok(ConsolidateSessionResultPayload {
            digest_chars: digest_chars.map(|n| n as u32),
        })
    }
}

// ---------------------------------------------------------------------------
// Slice 5 — Policy operations
// ---------------------------------------------------------------------------

#[async_trait]
pub trait PolicyService: Send + Sync {
    async fn get_policy(&self, cmd: GetPolicyCommand) -> Result<GetPolicyResultPayload, AppError>;
    async fn set_policy(&self, cmd: SetPolicyCommand) -> Result<(), AppError>;
    async fn reclassify_session(
        &self,
        cmd: ReclassifySessionCommand,
    ) -> Result<ReclassifySessionResultPayload, AppError>;
}

pub struct DefaultPolicyService {
    policy: Arc<lokai_policy::PolicyEngine>,
    store: Option<lokai_memory::SharedStore>,
    events: Arc<dyn ApplicationEventSink>,
}

impl DefaultPolicyService {
    pub fn new(
        policy: Arc<lokai_policy::PolicyEngine>,
        store: Option<lokai_memory::SharedStore>,
        events: Arc<dyn ApplicationEventSink>,
    ) -> Self {
        Self {
            policy,
            store,
            events,
        }
    }
}

#[async_trait]
impl PolicyService for DefaultPolicyService {
    async fn get_policy(&self, cmd: GetPolicyCommand) -> Result<GetPolicyResultPayload, AppError> {
        let floor = lokai_policy::classify_session(std::path::Path::new(&cmd.workspace_root), None);
        Ok(GetPolicyResultPayload {
            mode: self.policy.mode().as_str().to_string(),
            default_data_class: lokai_policy::data_class_name(floor.class).to_string(),
            verify_allowed: self.policy.verify_allowed(),
            mutations_allowed: self.policy.mutations_allowed(),
            allow_sensitive_to_owner_estate: self
                .policy
                .project_placement_policy()
                .allow_sensitive_to_owner_estate,
            allow_repository_to_admin_managed: self
                .policy
                .project_placement_policy()
                .allow_repository_to_admin_managed,
        })
    }

    async fn set_policy(&self, cmd: SetPolicyCommand) -> Result<(), AppError> {
        let mut changed = false;
        let mode_for_event = cmd.mode.clone();

        if let Some(mode_str) = cmd.mode {
            let mode = lokai_policy::PolicyMode::parse(&mode_str).ok_or_else(|| {
                AppError::InvalidRequest(format!("invalid policy mode: {mode_str}"))
            })?;
            self.policy.set_mode(mode);
            if let Some(s) = &self.store {
                s.write(move |db| db.set_policy_mode(mode.as_str()))
                    .await
                    .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?
                    .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?;
            }
            changed = true;
        }

        if let Some(verify_allowed) = cmd.verify_allowed {
            self.policy.set_verify_allowed(verify_allowed);
            if let Some(s) = &self.store {
                s.write(move |db| db.set_policy_verify_allowed(verify_allowed))
                    .await
                    .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?
                    .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?;
            }
            changed = true;
        }

        if let Some(mutations_allowed) = cmd.mutations_allowed {
            self.policy.set_mutations_allowed(mutations_allowed);
            if let Some(s) = &self.store {
                s.write(move |db| db.set_policy_mutations_allowed(mutations_allowed))
                    .await
                    .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?
                    .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?;
            }
            changed = true;
        }

        if cmd.allow_sensitive_to_owner_estate.is_some()
            || cmd.allow_repository_to_admin_managed.is_some()
        {
            let mut placement = self.policy.project_placement_policy();
            if let Some(v) = cmd.allow_sensitive_to_owner_estate {
                placement.allow_sensitive_to_owner_estate = v;
            }
            if let Some(v) = cmd.allow_repository_to_admin_managed {
                placement.allow_repository_to_admin_managed = v;
            }
            self.policy.set_project_placement_policy(placement.clone());
            if let Some(s) = &self.store {
                s.write(move |db| db.set_project_placement_policy(&placement))
                    .await
                    .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?
                    .map_err(|e| AppError::PersistenceFailed(format!("audit: {e}")))?;
            }
            changed = true;
        }

        if !changed {
            return Err(AppError::InvalidRequest(
                "policy/set requires at least one of: mode, verify_allowed, mutations_allowed, allow_sensitive_to_owner_estate, allow_repository_to_admin_managed"
                    .into(),
            ));
        }

        emit(
            &self.events,
            ApplicationEvent::PolicyUpdated {
                mode: mode_for_event,
            },
        );

        Ok(())
    }

    async fn reclassify_session(
        &self,
        cmd: ReclassifySessionCommand,
    ) -> Result<ReclassifySessionResultPayload, AppError> {
        let Some(class) = lokai_policy::parse_data_class(&cmd.data_class) else {
            return Err(AppError::InvalidRequest(format!(
                "invalid data_class: {}",
                cmd.data_class
            )));
        };
        if cmd.reason.trim().is_empty() {
            return Err(AppError::InvalidRequest(
                "reclassification reason is required".into(),
            ));
        }
        let previous = if let Some(s) = &self.store {
            let session_id = cmd.session_id.clone();
            let reason = cmd.reason.clone();
            s.write(move |db| {
                let prev = db.session_data_class(&session_id).ok().flatten();
                let _canonical = db
                    .reclassify_session(
                        &session_id,
                        lokai_policy::data_class_name(class),
                        &reason,
                        "explicit_reclassification",
                    )
                    .map_err(|e| AppError::PersistenceFailed(format!("reclassify: {e}")))?;
                Ok::<_, AppError>(prev)
            })
            .await
            .map_err(|e| AppError::PersistenceFailed(format!("reclassify: {e}")))??
        } else {
            None
        };
        emit(
            &self.events,
            ApplicationEvent::LogDiagnostic {
                session_id: Some(cmd.session_id.clone()),
                agent_id: None,
                message: format!(
                    "classification reclassified to {} (reason: {})",
                    lokai_policy::data_class_name(class),
                    cmd.reason
                ),
            },
        );
        Ok(ReclassifySessionResultPayload {
            data_class: lokai_policy::data_class_name(class).to_string(),
            previous_data_class: previous,
        })
    }
}
// ---------------------------------------------------------------------------
// Slice 6 — Estate
// ---------------------------------------------------------------------------

#[async_trait]
pub trait EstateService: Send + Sync {
    fn get_estate_status(
        &self,
        cmd: GetEstateStatusCommand,
    ) -> Result<EstateStatusResultPayload, AppError>;
    async fn enroll_worker(
        &self,
        cmd: EnrollWorkerCommand,
    ) -> Result<EnrollWorkerResultPayload, AppError>;
    async fn remove_worker(
        &self,
        cmd: RemoveWorkerCommand,
    ) -> Result<RemoveWorkerResultPayload, AppError>;
}

pub struct DefaultEstateService {
    store: Option<lokai_memory::SharedStore>,
    policy: Option<Arc<lokai_policy::PolicyEngine>>,
}

impl DefaultEstateService {
    pub fn new(
        store: Option<lokai_memory::SharedStore>,
        policy: Option<Arc<lokai_policy::PolicyEngine>>,
    ) -> Self {
        Self { store, policy }
    }
}

#[async_trait]
impl EstateService for DefaultEstateService {
    fn get_estate_status(
        &self,
        cmd: GetEstateStatusCommand,
    ) -> Result<EstateStatusResultPayload, AppError> {
        let workers = self
            .store
            .as_ref()
            .map(|s| {
                s.read_sync(|db| db.list_worker_enrollments())
                    .unwrap_or_else(|_| Ok(vec![]))
                    .unwrap_or_default()
                    .len() as u32
            })
            .unwrap_or(0);
        let policy_mode = self
            .policy
            .as_ref()
            .map(|p| p.mode().as_str().to_string())
            .unwrap_or_default();
        Ok(EstateStatusResultPayload {
            policy_mode,
            workers_enrolled: workers,
            fabric_pooled: cmd.fabric_pooled,
        })
    }

    async fn enroll_worker(
        &self,
        cmd: EnrollWorkerCommand,
    ) -> Result<EnrollWorkerResultPayload, AppError> {
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| AppError::InvalidRequest("enrollment requires lokai.db store".into()))?;
        crate::estate_enrollment::enroll_worker(store, cmd).await
    }

    async fn remove_worker(
        &self,
        cmd: RemoveWorkerCommand,
    ) -> Result<RemoveWorkerResultPayload, AppError> {
        let store = self.store.as_ref().ok_or_else(|| {
            AppError::InvalidRequest("worker removal requires lokai.db store".into())
        })?;
        crate::estate_enrollment::remove_worker(store, cmd).await
    }
}

pub use crate::capacity_service::{CapacityService, DefaultCapacityService};

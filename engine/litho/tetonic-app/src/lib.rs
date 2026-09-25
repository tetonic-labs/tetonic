pub mod approval;
pub mod capacity_service;
pub mod cli_bootstrap;
pub mod cli_estate;
pub mod cli_facade;
pub mod cli_index;
pub mod coding_pack;
pub mod commands;
pub mod compute_plane;
pub mod daemon_bootstrap;
pub mod definition;
pub mod errors;
pub mod estate_enrollment;
pub mod events;
pub mod fabric_run_bridge;
pub mod fleet_api;
pub mod inference_selection;
pub mod lsp_launcher;
pub mod lsp_session;
pub mod node_worker;
pub mod operator_control;
pub mod product_submit;
pub mod redaction_audit;
pub mod resources;
pub mod resume;
pub mod secret_scanner_factory;
pub mod semantic_effect;
pub mod services;
pub mod session_live;
pub mod spawn_budget;
pub mod store_audit;
pub mod test_harness;
pub mod thought_stream;
pub mod turn_attestation;
pub mod turn_execution;

#[cfg(test)]
mod inspect_api_tests;
#[cfg(test)]
mod r12_secret_override_tests;
#[cfg(test)]
mod r4_3_scanner_tests;
#[cfg(test)]
mod recovery_contract_tests;
#[cfg(test)]
mod session_authority_tests;
#[cfg(test)]
mod turn_attestation_tests;

pub use cli_bootstrap::{open_default_audit_store, CliBootstrapOutput, CliBootstrapParams};
pub use cli_estate::WorkerTrustAuditSummary;
pub use cli_facade::{
    CheckpointInfo, CheckpointsReport, ProjectStatusInfo, RestoreFileChange, RestoreSummary,
    SessionSummaryInfo,
};
pub use commands::TurnFinish;
pub use compute_plane::{build_compute_plane, ComputePlane, ComputePlaneRequest};
pub use daemon_bootstrap::{
    inference_port_from_base, load_coordinator_key, DaemonBootstrapOutput, DaemonBootstrapParams,
};
pub use resume::{rehydrate_messages, RESUME_MESSAGE_CAP};
pub use secret_scanner_factory::{
    install_shared_scanner_from_store, scanner_engine_from_store, scanner_from_shared_store,
    to_store_scope,
};
pub use session_live::{
    admit_chat_turn, capacity_gate_warning, CapacityGateWarning, LiveSession, SessionLiveStore,
    TurnAdmitDecision, TurnAdmitError,
};
pub use test_harness::{turn as test_turn, MockProvider, ScriptTurn};
pub use turn_attestation::{encode_candidate_bytes, seal_output_set, SealedTurn};

pub use tetonic_capacity::{
    parse_tier_role, CapacityDoctorStatus, CapacityStatus, DiagnosisCode, InferenceDefaults,
    JobState, OptimizeDepth, OptimizeOutcome, LOCAL_NODE_ID as CAPACITY_LOCAL_NODE_ID,
};
pub use tetonic_core::ConfinementWarning;
pub use tetonic_inference::{
    DispatchPlacementReport, DispatchPlacementSink, FabricSnapshot, FunctionCall,
    InferenceProvider, NodeInfo, ToolCall, DEFAULT_MODEL, LOCAL_NODE_ID,
};
pub use tetonic_memory::SharedStore;
pub use tetonic_orchestrator::{next_child_agent_id, ROOT_AGENT};
pub use tetonic_policy::data_class_name;
pub use tetonic_secrets::ScannerEngine;
pub use tetonic_telemetry;

pub use fleet_api::{
    AgentResponse, CreateAgentRequest, CreateOrgRequest, CreateSquadRequest, FleetApiError,
    FleetManager, OrgResponse, SquadResponse,
};
pub use operator_control::{
    EstopRequest, EstopResponse, OperatorControlError, OperatorController, OperatorDashboardView,
    SteerAgentRequest, SteerResponse,
};
pub use thought_stream::{TelemetryEvent, ThoughtStreamHub};

use crate::services::*;
use std::sync::Arc;

struct RunCommitEventHook;

impl tetonic_run::RunEventHook for RunCommitEventHook {
    fn on_run_committed(&self, result: &tetonic_domain::RunCommandResult) {
        tracing::debug!(
            run_id = %result.run_id,
            sequence = result.sequence,
            "run command committed"
        );
    }
}

pub(crate) fn build_supervisor(
    store: Option<tetonic_memory::SharedStore>,
) -> Arc<dyn tetonic_run::RunSupervisor> {
    // Startup storage failures enter Safe Mode inside the manager. An individual
    // interrupted run stays quarantined by its own RecoveryRequired state.
    let sup = tetonic_run::DurableRunSupervisor::new(store).with_hook(Arc::new(RunCommitEventHook));
    Arc::new(sup)
}

/// Central Application Kernel — owns service implementations and their shared
/// dependencies.  Transport adapters (`lokaid`, CLI) hold an `Arc<Application>`
/// and delegate orchestration decisions to its services.
pub struct Application {
    pub init: Arc<dyn InitializationService>,
    pub sessions: Arc<dyn SessionService>,
    pub runs: Arc<dyn RunService>,
    pub(crate) run_manager: Arc<services::DefaultRunService>,
    pub(crate) supervisor: Arc<dyn tetonic_run::RunSupervisor>,
    pub policies: Arc<dyn PolicyService>,
    pub approvals: Arc<dyn approval::ApprovalService>,
    pub estate: Arc<dyn EstateService>,
    pub capacity: Arc<dyn CapacityService>,
    pub turn: Arc<product_submit::TurnBind>,
}

/// Explicit constructor dependencies — avoids global/process state inside lokai-app.
pub struct ApplicationDependencies {
    pub runtime: Arc<tetonic_runtime::EngineRuntime>,
    pub store: Option<tetonic_memory::SharedStore>,
    pub policy: Arc<tetonic_policy::PolicyEngine>,
    pub event_sink: Arc<dyn events::ApplicationEventSink>,
    pub index_db: Option<std::path::PathBuf>,
    pub fabric_hint: Option<String>,
}

impl Application {
    /// Bootstrap workspace + runtime, returning a fully wired `Application`.
    pub async fn bootstrap(
        init_cmd: commands::InitializeCommand,
        store: Option<tetonic_memory::SharedStore>,
        event_sink: Arc<dyn events::ApplicationEventSink>,
        index_db: Option<std::path::PathBuf>,
        fabric_hint: Option<String>,
    ) -> Result<
        (
            Self,
            commands::InitializeResultPayload,
            commands::InitializeBootstrapPayload,
        ),
        errors::AppError,
    > {
        let init: Arc<dyn InitializationService> =
            Arc::new(services::DefaultInitializationService::new());
        let init_result = init.initialize_workspace(init_cmd.clone()).await?;
        let bootstrap = init.bootstrap_runtime(&init_cmd, &store)?;
        let app = Self::from_bootstrap(init, &bootstrap, store, event_sink, index_db, fabric_hint);
        Ok((app, init_result, bootstrap))
    }

    /// Wire services from a completed bootstrap using the same init service instance.
    pub fn from_bootstrap(
        init: Arc<dyn InitializationService>,
        bootstrap: &commands::InitializeBootstrapPayload,
        store: Option<tetonic_memory::SharedStore>,
        event_sink: Arc<dyn events::ApplicationEventSink>,
        index_db: Option<std::path::PathBuf>,
        fabric_hint: Option<String>,
    ) -> Self {
        Self::from_bootstrap_with_supervisor(
            init,
            bootstrap,
            store.clone(),
            event_sink,
            index_db,
            fabric_hint,
            build_supervisor(store),
        )
    }

    /// Same as [`Self::from_bootstrap`] but reuses an existing RunSupervisor
    /// (so the compute plane can bind it before index/submit).
    pub fn from_bootstrap_with_supervisor(
        init: Arc<dyn InitializationService>,
        bootstrap: &commands::InitializeBootstrapPayload,
        store: Option<tetonic_memory::SharedStore>,
        event_sink: Arc<dyn events::ApplicationEventSink>,
        index_db: Option<std::path::PathBuf>,
        fabric_hint: Option<String>,
        supervisor: Arc<dyn tetonic_run::RunSupervisor>,
    ) -> Self {
        let live = Arc::new(session_live::SessionLiveStore::new());
        let run_manager = Arc::new(
            services::DefaultRunService::new(
                store.clone(),
                bootstrap.policy.clone(),
                event_sink.clone(),
                supervisor.clone(),
                live.clone(),
                bootstrap.runtime.artifact_store().clone(),
            )
            .with_execution_policy(Arc::new(definition::validate_coding_execution))
            .with_identity_supplier(Arc::new(|input| {
                definition::coding_identity_and_job_spec(input)
            })),
        );
        let runs: Arc<dyn services::RunService> = run_manager.clone();
        let approvals: Arc<dyn approval::ApprovalService> = Arc::new(
            approval::DefaultApprovalService::new(store.clone(), event_sink.clone()),
        );
        run_manager.attach_approvals(approvals.clone());
        run_manager.attach_runtime(bootstrap.runtime.clone());
        Self {
            init,
            sessions: Arc::new(services::DefaultSessionService::new(
                bootstrap.policy.clone(),
                store.clone(),
                index_db.clone(),
                fabric_hint.clone(),
                event_sink.clone(),
                live,
                supervisor.clone(),
                run_manager.clone(),
                approvals.clone(),
            )),
            runs,
            run_manager,
            supervisor,
            policies: Arc::new(services::DefaultPolicyService::new(
                bootstrap.policy.clone(),
                store.clone(),
                event_sink.clone(),
            )),
            approvals,
            estate: Arc::new(services::DefaultEstateService::new(
                store.clone(),
                Some(bootstrap.policy.clone()),
            )),
            capacity: Arc::new(services::DefaultCapacityService::new(
                store.clone(),
                event_sink.clone(),
            )),
            turn: product_submit::TurnBind::new(
                bootstrap.runtime.clone(),
                store,
                index_db,
                bootstrap.workspace_root.clone(),
                bootstrap.inference_defaults.num_ctx,
                event_sink,
                bootstrap.policy.clone(),
            ),
        }
    }

    pub fn new(deps: ApplicationDependencies) -> Self {
        let supervisor = build_supervisor(deps.store.clone());
        let live = Arc::new(session_live::SessionLiveStore::new());
        let run_manager = Arc::new(
            services::DefaultRunService::new(
                deps.store.clone(),
                deps.policy.clone(),
                deps.event_sink.clone(),
                supervisor.clone(),
                live.clone(),
                deps.runtime.artifact_store().clone(),
            )
            .with_execution_policy(Arc::new(definition::validate_coding_execution))
            .with_identity_supplier(Arc::new(|input| {
                definition::coding_identity_and_job_spec(input)
            })),
        );
        let runs: Arc<dyn services::RunService> = run_manager.clone();
        let approvals: Arc<dyn approval::ApprovalService> = Arc::new(
            approval::DefaultApprovalService::new(deps.store.clone(), deps.event_sink.clone()),
        );
        run_manager.attach_approvals(approvals.clone());
        run_manager.attach_runtime(deps.runtime.clone());
        Self {
            init: Arc::new(services::DefaultInitializationService::new()),
            sessions: Arc::new(services::DefaultSessionService::new(
                deps.policy.clone(),
                deps.store.clone(),
                deps.index_db.clone(),
                deps.fabric_hint,
                deps.event_sink.clone(),
                live,
                supervisor.clone(),
                run_manager.clone(),
                approvals.clone(),
            )),
            runs,
            run_manager,
            supervisor,
            policies: Arc::new(services::DefaultPolicyService::new(
                deps.policy.clone(),
                deps.store.clone(),
                deps.event_sink.clone(),
            )),
            approvals,
            estate: Arc::new(services::DefaultEstateService::new(
                deps.store.clone(),
                Some(deps.policy.clone()),
            )),
            capacity: Arc::new(services::DefaultCapacityService::new(
                deps.store.clone(),
                deps.event_sink.clone(),
            )),
            turn: product_submit::TurnBind::new(
                deps.runtime,
                deps.store,
                deps.index_db,
                String::new(),
                tetonic_capacity::InferenceDefaults::fallback().num_ctx,
                deps.event_sink,
                deps.policy.clone(),
            ),
        }
    }

    /// In-process subscribe door (`00` §14). Daemon public events still go
    /// through `OutboundQueue` + redactor; this returns the bootstrap sink.
    pub fn event_sink(&self) -> Arc<dyn events::ApplicationEventSink> {
        self.runs.event_sink()
    }

    /// Register a user regex detector. Scanner is a collaborator; Application is the door.
    pub fn add_secret_rule(
        &self,
        scanner: &tetonic_secrets::ScannerEngine,
        pattern: &str,
    ) -> Result<(), errors::AppError> {
        let regex = regex::Regex::new(pattern)
            .map_err(|_| errors::AppError::InvalidRequest("Invalid regex pattern".into()))?;
        let detector = tetonic_secrets::detectors::RegexDetector {
            rule_id: format!("user_rule_{}", tetonic_memory::new_id("rule")),
            version: 1,
            regex,
            kind: tetonic_secrets::types::SecretKind::ProviderToken,
            confidence: tetonic_secrets::types::FindingConfidence::Confirmed,
        };
        scanner.add_user_rule(Arc::new(detector));
        Ok(())
    }

    pub async fn allow_secret_fingerprint(
        &self,
        scanner: &tetonic_secrets::ScannerEngine,
        fingerprint: &str,
        scope_kind: &str,
        scope_id: Option<&str>,
        durable: bool,
    ) -> Result<commands::AllowSecretFingerprintResult, errors::AppError> {
        let scope = parse_secret_override_scope(scope_kind, scope_id)?;
        let mut override_id = None;
        if durable {
            let store = self.turn.store.as_ref().ok_or_else(|| {
                errors::AppError::PersistenceFailed("durable override requires audit store".into())
            })?;
            let fp = fingerprint.to_string();
            let store_scope = to_store_scope(&scope);
            let row = store
                .write(move |db| db.grant_secret_override(&fp, store_scope, true, Some("rpc")))
                .await
                .map_err(|e| errors::AppError::PersistenceFailed(e.to_string()))?
                .map_err(|e| errors::AppError::PersistenceFailed(e.to_string()))?;
            override_id = Some(row.id);
        }
        scanner.grant_override(fingerprint, scope.clone());
        Ok(commands::AllowSecretFingerprintResult {
            durable,
            scope_kind: scope.kind().to_string(),
            scope_id: scope.id().map(str::to_string),
            override_id,
        })
    }

    pub async fn revoke_secret_fingerprint(
        &self,
        scanner: &tetonic_secrets::ScannerEngine,
        fingerprint: &str,
        scope_kind: &str,
        scope_id: Option<&str>,
    ) -> Result<commands::RevokeSecretFingerprintResult, errors::AppError> {
        let scope = parse_secret_override_scope(scope_kind, scope_id)?;
        let mut revoked = false;
        if let Some(store) = self.turn.store.as_ref() {
            let fp = fingerprint.to_string();
            let store_scope = to_store_scope(&scope);
            revoked = store
                .write(move |db| db.revoke_secret_override(&fp, store_scope, Some("rpc")))
                .await
                .map_err(|e| errors::AppError::PersistenceFailed(e.to_string()))?
                .map_err(|e| errors::AppError::PersistenceFailed(e.to_string()))?;
        }
        scanner.revoke_override(fingerprint, scope.clone());
        Ok(commands::RevokeSecretFingerprintResult {
            revoked_durable: revoked,
            scope_kind: scope.kind().to_string(),
            scope_id: scope.id().map(str::to_string),
        })
    }
}

pub fn generate_rpc_token() -> String {
    let kp = tetonic_enroll::KeyPair::generate();
    kp.public().0.iter().map(|b| format!("{b:02x}")).collect()
}

fn parse_secret_override_scope(
    kind: &str,
    id: Option<&str>,
) -> Result<tetonic_secrets::OverrideScope, errors::AppError> {
    match kind {
        "global" | "" => Ok(tetonic_secrets::OverrideScope::Global),
        "session" => {
            let id = id.filter(|s| !s.is_empty()).ok_or_else(|| {
                errors::AppError::InvalidRequest("session scope requires scope_id".into())
            })?;
            Ok(tetonic_secrets::OverrideScope::Session(id.to_string()))
        }
        "project" => {
            let id = id.filter(|s| !s.is_empty()).ok_or_else(|| {
                errors::AppError::InvalidRequest("project scope requires scope_id".into())
            })?;
            Ok(tetonic_secrets::OverrideScope::Project(id.to_string()))
        }
        other => Err(errors::AppError::InvalidRequest(format!(
            "unknown scope_kind '{other}'"
        ))),
    }
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

mod turn_delivery;

#[cfg(test)]
mod tui_mvp_tests;

mod recovery_api;

mod session_control;

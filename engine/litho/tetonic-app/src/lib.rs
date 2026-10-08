pub mod approval;
pub mod capacity_service;
pub mod cli_estate;
pub mod coding_pack;
pub mod commands;
pub mod compute_plane;
pub mod definition;
pub mod errors;
pub mod estate_enrollment;
pub mod events;
pub mod fabric_run_bridge;
pub mod host;
pub mod job_launch;
pub mod local_workspace;
pub mod node_worker;
pub mod redaction_audit;
pub mod resources;
pub mod secret_scanner_factory;
pub mod services;
pub mod store_audit;
mod team_work_controller;
pub mod test_harness;
pub mod turn_attestation;
pub mod turn_execution;
pub mod work;
pub(crate) mod workspace;

#[cfg(test)]
mod inspect_api_tests;
#[cfg(test)]
mod r12_secret_override_tests;
#[cfg(test)]
mod r4_3_scanner_tests;

pub use cli_estate::WorkerTrustAuditSummary;
pub use commands::TurnFinish;
pub use compute_plane::{build_compute_plane, ComputePlane, ComputePlaneRequest};
pub use job_launch::{
    host_settings_from_json, launch_registered_job, launch_team_work, RegisteredLaunchHost,
    RegisteredLaunchReceipt,
};
pub use secret_scanner_factory::{
    install_shared_scanner_from_store, scanner_engine_from_store, scanner_from_shared_store,
    to_store_scope,
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

use crate::services::*;
use std::sync::Arc;

/// Central Application Kernel — owns service implementations and their shared
/// dependencies.  The local workspace host and operator CLI hold an `Arc<Application>`
/// and delegate orchestration decisions to its services.
pub struct Application {
    pub init: Arc<dyn InitializationService>,
    pub runs: Arc<dyn RunService>,
    pub(crate) run_manager: Arc<services::DefaultRunService>,
    pub(crate) supervisor: Arc<dyn tetonic_run::RunSupervisor>,
    pub policies: Arc<dyn PolicyService>,
    pub approvals: Arc<dyn approval::ApprovalService>,
    pub estate: Arc<dyn EstateService>,
    pub capacity: Arc<dyn CapacityService>,
    pub host: Arc<host::HostServices>,
}

/// Explicit constructor dependencies — avoids global/process state inside lokai-app.
pub struct ApplicationDependencies {
    pub runtime: Arc<tetonic_runtime::EngineRuntime>,
    pub store: Option<tetonic_memory::SharedStore>,
    pub policy: Arc<tetonic_policy::PolicyEngine>,
    pub event_sink: Arc<dyn events::ApplicationEventSink>,
    pub index_db: Option<std::path::PathBuf>,
}

impl Application {
    /// Install the definition validator for this kernel's identity-job door.
    /// Without one, the door fails closed. Product launches pin a harness policy
    /// per admission instead (see `resources::activation`).
    pub fn with_execution_policy(mut self, policy: tetonic_run::ExecutionPolicy) -> Self {
        let runs = Arc::new((*self.run_manager).clone().with_execution_policy(policy));
        self.runs = runs.clone();
        self.run_manager = runs;
        self
    }

    /// Durable audit store backing this kernel, when configured.
    pub fn store(&self) -> Option<&tetonic_memory::SharedStore> {
        self.host.store.as_ref()
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
            let store = self.host.store.as_ref().ok_or_else(|| {
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
        if let Some(store) = self.host.store.as_ref() {
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

#[cfg(test)]
mod tui_mvp_tests;

pub mod mcp;
pub mod skills;

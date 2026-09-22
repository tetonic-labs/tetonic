//! Agent RPC (v1) — the single, auditable boundary between the Rust daemon and
//! the editor. See `docs/implementation/contracts/agent-rpc-v1.md`.
//!
//! This crate is intentionally engine-free: it owns only the **transport**
//! (LSP-style `Content-Length` framing over stdio) and the **protocol types**
//! (JSON-RPC 2.0 envelope + the v1 method params/results and notification
//! payloads). The daemon (`lokaid`) wires these to the actual agent loop; a
//! future editor generates its TypeScript client from the same `JsonSchema`
//! derives, so the two sides cannot drift.
//!
//! Privacy note: the contract travels over a pipe (stdin/stdout), never a
//! socket — the richest data channel in the system physically cannot leave the
//! machine.

#![recursion_limit = "256"]

pub mod framing;
pub mod inference;
pub use inference::*;
pub mod outbound;
pub mod protocol;
pub mod server;

/// The protocol version this build speaks. `initialize` negotiates it; v1 is
/// additive (unknown fields/notifications are tolerated).
pub const PROTOCOL_VERSION: u32 = 1;

pub use outbound::{classify_outbound, OutboundClass, OutboundQueue, DEFAULT_OUTBOUND_CAPACITY};
pub use protocol::*;
pub use server::{channel_pair, writer_task, Notifier};

/// Emit the full v1 protocol as a single JSON Schema bundle: one shared
/// `definitions` map holding every request param/result and notification payload
/// type. This is the **source of truth** the editor's TypeScript client is
/// generated from (`lokaid --print-schema` → `scripts/gen_ts_protocol.py`).
/// Deriving it from the Rust types means the two sides cannot drift.
pub fn schema_bundle() -> serde_json::Value {
    let mut generator = schemars::gen::SchemaGenerator::default();
    macro_rules! register {
        ($($t:ty),* $(,)?) => { $( let _ = generator.subschema_for::<$t>(); )* };
    }
    register!(
        inference::SessionInferenceParams,
        inference::SessionSelectModelParams,
        inference::SessionModelsResult,
        inference::ModelChoice,
        inference::SessionSetInferenceParams,
        inference::SessionInferenceResult,
        protocol::InitializeParams,
        protocol::InitializeResult,
        protocol::ClientInfo,
        protocol::DaemonInfo,
        protocol::Capabilities,
        protocol::CapacitySummary,
        protocol::SessionStartParams,
        protocol::SessionStartResult,
        protocol::SessionEndParams,
        protocol::SessionEndResult,
        protocol::SessionReclassifyParams,
        protocol::SessionReclassifyResult,
        protocol::AgentSpawnParams,
        protocol::AgentSpawnResult,
        protocol::ChatSendParams,
        protocol::Attachment,
        protocol::Accepted,
        protocol::SessionCancelParams,
        protocol::Canceled,
        protocol::ProjectConsolidateParams,
        protocol::ProjectConsolidateResult,
        protocol::ApprovalDecision,
        protocol::ApprovalRespondParams,
        protocol::Ack,
        protocol::ModelListResult,
        protocol::PolicyGetResult,
        protocol::PolicySetParams,
        protocol::PolicySetResult,
        protocol::EstateStatusResult,
        protocol::FabricNodeCapacityHealth,
        protocol::FabricNodeInfo,
        protocol::FabricStatusResult,
        protocol::CapacityDoctorResult,
        protocol::CapacityOptimizeParams,
        protocol::CapacityOptimizeResult,
        protocol::CapacityCancelParams,
        protocol::CapacityCancelResult,
        protocol::CapacityProgressParams,
        protocol::CapacityProfilesListParams,
        protocol::CapacityProfileListItem,
        protocol::CapacityProfilesListResult,
        protocol::CapacityProfilesActivateParams,
        protocol::CapacityProfilesActivateResult,
        protocol::CapacityProfilesRollbackParams,
        protocol::CapacityProfilesRollbackResult,
        protocol::CapacityProfilesExportParams,
        protocol::CapacityProfilesExportResult,
        protocol::CapacityJobsGetParams,
        protocol::CapacityJobsGetResult,
        protocol::EgressPolicy,
        protocol::EgressAllowRule,
        protocol::EgressPolicySetParams,
        protocol::EgressPolicySetResult,
        protocol::RunSnapshotParams,
        protocol::RunSnapshotResult,
        protocol::RunResumeParams,
        protocol::RunResumeGap,
        protocol::RunResumeResult,
        protocol::RunCancelParams,
        protocol::RunStatusParams,
        protocol::TokenParams,
        protocol::ToolCallParams,
        protocol::ToolResultParams,
        protocol::DiffParams,
        protocol::ApprovalRequestParams,
        protocol::MissingControlParams,
        protocol::EgressEventParams,
        protocol::ContextParams,
        protocol::DispatchPlacementParams,
        protocol::LogParams,
    );
    use protocol::{events, methods, ErrorCode};
    serde_json::json!({
        "protocol_version": PROTOCOL_VERSION,
        "definitions": generator.definitions(),
        "methods": {
            "INITIALIZE": methods::INITIALIZE,
            "SESSION_START": methods::SESSION_START,
            "SESSION_RECLASSIFY": methods::SESSION_RECLASSIFY,
            "SESSION_INFERENCE": methods::SESSION_INFERENCE,
            "SESSION_MODELS": methods::SESSION_MODELS,
            "SESSION_SELECT_MODEL": methods::SESSION_SELECT_MODEL,
            "SESSION_SET_INFERENCE": methods::SESSION_SET_INFERENCE,
            "SESSION_END": methods::SESSION_END,
            "CHAT_SEND": methods::CHAT_SEND,
            "SESSION_CANCEL": methods::SESSION_CANCEL,
            "APPROVAL_RESPOND": methods::APPROVAL_RESPOND,
            "MODEL_LIST": methods::MODEL_LIST,
            "EGRESS_POLICY_GET": methods::EGRESS_POLICY_GET,
            "EGRESS_POLICY_SET": methods::EGRESS_POLICY_SET,
            "POLICY_GET": methods::POLICY_GET,
            "POLICY_SET": methods::POLICY_SET,
            "ESTATE_STATUS": methods::ESTATE_STATUS,
            "ESTATE_CAPACITY_STATUS": methods::ESTATE_CAPACITY_STATUS,
            "ESTATE_CAPACITY_DOCTOR": methods::ESTATE_CAPACITY_DOCTOR,
            "ESTATE_CAPACITY_OPTIMIZE": methods::ESTATE_CAPACITY_OPTIMIZE,
            "ESTATE_CAPACITY_CANCEL": methods::ESTATE_CAPACITY_CANCEL,
            "ESTATE_CAPACITY_PROFILES_LIST": methods::ESTATE_CAPACITY_PROFILES_LIST,
            "ESTATE_CAPACITY_PROFILES_ACTIVATE": methods::ESTATE_CAPACITY_PROFILES_ACTIVATE,
            "ESTATE_CAPACITY_PROFILES_ROLLBACK": methods::ESTATE_CAPACITY_PROFILES_ROLLBACK,
            "ESTATE_CAPACITY_PROFILES_EXPORT": methods::ESTATE_CAPACITY_PROFILES_EXPORT,
            "ESTATE_CAPACITY_JOBS_GET": methods::ESTATE_CAPACITY_JOBS_GET,
            "ESTATE_CAPACITY_JOBS_CANCEL": methods::ESTATE_CAPACITY_JOBS_CANCEL,
            "FABRIC_STATUS": methods::FABRIC_STATUS,
            "FABRIC_WORKER_TRUST_SET": methods::FABRIC_WORKER_TRUST_SET,
            "FABRIC_WORKER_TRUST_GET": methods::FABRIC_WORKER_TRUST_GET,
            "AGENT_SPAWN": methods::AGENT_SPAWN,
            "PROJECT_CONSOLIDATE": methods::PROJECT_CONSOLIDATE,
            "RUN_SNAPSHOT": methods::RUN_SNAPSHOT,
            "RUN_RESUME": methods::RUN_RESUME,
            "RUN_CANCEL": methods::RUN_CANCEL,
            "SECRET_RULE_ADD": methods::SECRET_RULE_ADD,
            "SECRET_FINGERPRINT_ALLOW": methods::SECRET_FINGERPRINT_ALLOW,
            "SECRET_FINGERPRINT_REVOKE": methods::SECRET_FINGERPRINT_REVOKE,
            "SHUTDOWN": methods::SHUTDOWN,
        },
        "events": {
            "RUN_STATUS": events::RUN_STATUS,
            "TOKEN": events::TOKEN,
            "TOOL_CALL": events::TOOL_CALL,
            "TOOL_RESULT": events::TOOL_RESULT,
            "DIFF": events::DIFF,
            "APPROVAL_REQUEST": events::APPROVAL_REQUEST,
            "EGRESS": events::EGRESS,
            "CONTEXT": events::CONTEXT,
            "LOG": events::LOG,
            "CAPACITY_PROGRESS": events::CAPACITY_PROGRESS,
            "DISPATCH_PLACEMENT": events::DISPATCH_PLACEMENT,
        },
        "error_codes": {
            "ParseError": ErrorCode::ParseError.code(),
            "InvalidRequest": ErrorCode::InvalidRequest.code(),
            "MethodNotFound": ErrorCode::MethodNotFound.code(),
            "InvalidParams": ErrorCode::InvalidParams.code(),
            "InternalError": ErrorCode::InternalError.code(),
            "NotImplemented": ErrorCode::NotImplemented.code(),
            "UnknownSession": ErrorCode::UnknownSession.code(),
            "NotReady": ErrorCode::NotReady.code(),
        },
    })
}

#[cfg(test)]
mod schema_tests {
    use super::protocol::SCHEMA_DEFINITIONS;

    #[test]
    fn bundle_contains_every_registered_type() {
        let bundle = super::schema_bundle();
        assert_eq!(bundle["protocol_version"], super::PROTOCOL_VERSION);
        let defs = bundle["definitions"]
            .as_object()
            .expect("definitions object");
        for name in SCHEMA_DEFINITIONS {
            assert!(defs.contains_key(*name), "schema bundle missing `{name}`");
        }
        assert_eq!(
            defs.len(),
            SCHEMA_DEFINITIONS.len(),
            "schema bundle has extra definitions — add them to SCHEMA_DEFINITIONS"
        );
    }

    #[test]
    fn bundle_lists_all_wire_methods_and_events() {
        let bundle = super::schema_bundle();
        let methods = bundle["methods"].as_object().expect("methods");
        let events = bundle["events"].as_object().expect("events");
        assert_eq!(methods.len(), 39);
        assert_eq!(events.len(), 11);
    }
}

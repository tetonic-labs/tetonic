//! Fabric topology and transport types (N0.0).
//!
//! See `docs/implementation/contracts/inference-fabric-v1.md` and
//! `fabric-transport-v1.md`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
pub use tetonic_domain::{DataClass, DisclosureTier};

use crate::{GenUsage, Message, ToolSchema};

/// Stable id for the co-located loopback runtime (not an enrolled worker).
pub const LOCAL_NODE_ID: &str = "node_local";

// ---- Topology (inference-fabric-v1) ----------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FabricSnapshot {
    pub nodes: Vec<NodeInfo>,
    /// Max parallel inference jobs the fabric can accept right now.
    pub effective_concurrency: u32,
    pub generated_at: DateTime<Utc>,
}

impl FabricSnapshot {
    pub fn empty() -> Self {
        Self {
            nodes: Vec::new(),
            effective_concurrency: 0,
            generated_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeCapacityHealth {
    /// `healthy` | `degraded` | `unknown` | `no_profile`
    pub doctor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_profile_id: Option<String>,
    pub gates_ok: bool,
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeInfo {
    /// `WorkerEnrollment.id` for remote nodes; [`LOCAL_NODE_ID`] for loopback.
    pub id: String,
    pub label: String,
    pub vram_total_mb: u32,
    pub vram_free_mb: u32,
    pub resident_models: Vec<String>,
    pub queue_depth: u32,
    pub healthy: bool,
    /// True when `resident_models` came from a successful capabilities probe (not a failed fetch).
    #[serde(default)]
    pub models_verified: bool,
    /// Local capacity profile health (ES5-3). Omitted on remote nodes until ES5-4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity: Option<NodeCapacityHealth>,
    /// Legacy `/v1/chat` workers cannot enforce lease/cancel semantics (M5-1).
    #[serde(default)]
    pub legacy_v1_chat_only: bool,
    /// Agreed fabric protocol version from `/v1/negotiate` (R7-3). Absent until probe succeeds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negotiated_protocol_version: Option<u32>,
}

// ---- Transport (fabric-transport-v1) ---------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FabricJob {
    pub job_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<String>,
    pub estate_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub agent_id: String,
    pub step_index: u32,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSchema>,
    pub options: SampleOptions,
    pub priority: JobPriority,
    pub data_class: DataClass,
    pub disclosure_tier: DisclosureTier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_envelope: Option<AuditEnvelope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub circle_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumer_peer_id: Option<String>,
    pub policy_epoch: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_affinity: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SampleOptions {
    #[serde(default)]
    pub temperature: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub num_ctx: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<serde_json::Value>,
    /// When true, worker may stream NDJSON token lines on `/v1/chat` (AR1-2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum JobPriority {
    #[default]
    OwnerInteractive,
    OwnerBackground,
    CircleInteractive,
    CircleBackground,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditEnvelope {
    pub envelope_id: String,
    pub job_id: String,
    pub disclosure_tier: DisclosureTier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_sealed: Option<String>,
    pub sealed_for: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumer_signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_until: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FabricJobResult {
    pub job_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<String>,
    pub message: Message,
    pub usage: GenUsageSerde,
    pub status: JobStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Signed M5-4 result envelope. Required for authoritative remote acceptance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_envelope: Option<serde_json::Value>,
}

/// Serializable mirror of [`GenUsage`] for wire types and receipts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct GenUsageSerde {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_eval_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval_ms: Option<f64>,
}

impl From<&GenUsage> for GenUsageSerde {
    fn from(u: &GenUsage) -> Self {
        Self {
            prompt_tokens: u.prompt_tokens,
            eval_tokens: u.eval_tokens,
            prompt_eval_ms: u.prompt_eval_ms,
            eval_ms: u.eval_ms,
        }
    }
}

impl From<GenUsageSerde> for GenUsage {
    fn from(u: GenUsageSerde) -> Self {
        Self {
            prompt_tokens: u.prompt_tokens,
            eval_tokens: u.eval_tokens,
            prompt_eval_ms: u.prompt_eval_ms,
            eval_ms: u.eval_ms,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    #[default]
    Ok,
    Error,
    Canceled,
    Preempted,
    Denied,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptKind {
    Local,
    Estate,
    CircleOut,
    CircleIn,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptDirection {
    Outbound,
    Inbound,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComputeReceipt {
    pub job_id: String,
    pub direction: ReceiptDirection,
    pub kind: ReceiptKind,
    pub disclosure_tier: DisclosureTier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    pub status: JobStatus,
    pub started_at: DateTime<Utc>,
    pub settled_at: DateTime<Utc>,
}

/// Unique fabric job id (monotonic suffix — safe under same-ms bursts, AR1-2).
pub fn new_fabric_job_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("job_{}_{seq}", Utc::now().timestamp_millis())
}

/// Unique attempt id for a fabric job try (AC2-7).
pub fn new_fabric_attempt_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("att_{}_{seq}", Utc::now().timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FunctionCall, ToolCall, ToolSchema};
    use serde_json::json;

    #[test]
    fn fabric_job_serde_round_trip() {
        let job = FabricJob {
            job_id: "job_test".into(),
            attempt_id: Some("att_test".into()),
            estate_id: "estate_abc".into(),
            session_id: Some("sess_1".into()),
            agent_id: "a0".into(),
            step_index: 2,
            model: "qwen3.5:latest".into(),
            tier: Some("coder".into()),
            messages: vec![Message::user("hi")],
            tools: vec![ToolSchema::function(
                "read_file",
                "read",
                json!({"type": "object"}),
            )],
            options: SampleOptions {
                temperature: 0.2,
                num_ctx: Some(8192),
                ..Default::default()
            },
            priority: JobPriority::OwnerInteractive,
            data_class: DataClass::RepositorySource,
            disclosure_tier: DisclosureTier::Auditable,
            audit_envelope: None,
            circle_id: None,
            consumer_peer_id: None,
            policy_epoch: 1,
            turn_affinity: Some(LOCAL_NODE_ID.into()),
        };
        let json = serde_json::to_string(&job).unwrap();
        let back: FabricJob = serde_json::from_str(&json).unwrap();
        assert_eq!(back.job_id, job.job_id);
        assert_eq!(back.model, job.model);
        assert_eq!(back.messages.len(), 1);
    }

    #[test]
    fn fabric_snapshot_serde_round_trip() {
        let snap = FabricSnapshot {
            nodes: vec![NodeInfo {
                id: LOCAL_NODE_ID.into(),
                label: "Local Ollama".into(),
                vram_total_mb: 24_576,
                vram_free_mb: 12_000,
                resident_models: vec!["qwen3.5:latest".into()],
                queue_depth: 0,
                healthy: true,
                models_verified: true,
                capacity: None,
                legacy_v1_chat_only: false,
                negotiated_protocol_version: None,
            }],
            effective_concurrency: 1,
            generated_at: Utc::now(),
        };
        let json = serde_json::to_string(&snap).unwrap();
        let back: FabricSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snap, back);
    }

    #[test]
    fn fabric_job_result_and_receipt_round_trip() {
        let result = FabricJobResult {
            job_id: "job_1".into(),
            attempt_id: Some("att_1".into()),
            message: Message::assistant("ok").with_tool_calls(vec![ToolCall {
                function: FunctionCall {
                    name: "finish".into(),
                    arguments: json!({"summary": "done"}),
                },
            }]),
            usage: GenUsageSerde {
                prompt_tokens: Some(100),
                eval_tokens: Some(20),
                ..Default::default()
            },
            status: JobStatus::Ok,
            error: None,
            result_envelope: None,
        };
        let _: FabricJobResult =
            serde_json::from_str(&serde_json::to_string(&result).unwrap()).unwrap();

        let receipt = ComputeReceipt {
            job_id: "job_1".into(),
            direction: ReceiptDirection::Outbound,
            kind: ReceiptKind::Local,
            disclosure_tier: DisclosureTier::MetadataOnly,
            session_id: Some("sess".into()),
            prompt_tokens: Some(100),
            eval_tokens: Some(20),
            duration_ms: Some(1500),
            status: JobStatus::Ok,
            started_at: Utc::now(),
            settled_at: Utc::now(),
        };
        let _: ComputeReceipt =
            serde_json::from_str(&serde_json::to_string(&receipt).unwrap()).unwrap();
    }
}

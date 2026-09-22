use serde::{Deserialize, Serialize};

use lokai_domain::classify::DataClass;
use lokai_domain::run::Timestamp;
use lokai_domain::workspace::ContentDigest;

pub type FindingId = String;
pub type SecretRuleId = String;
pub type RedactionId = String;
pub type ScannerVersion = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceReference {
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingLocation {
    pub byte_start: usize,
    pub byte_end: usize,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SecretKind {
    ProviderToken,
    PrivateKey,
    ConnectionString,
    Password,
    HighEntropy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum FindingConfidence {
    Confirmed,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretFingerprint(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedactedPreview(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretFinding {
    pub finding_id: FindingId,
    pub rule_id: SecretRuleId,
    pub rule_version: u32,
    pub source: SourceReference,
    pub location: FindingLocation,
    pub secret_kind: SecretKind,
    pub confidence: FindingConfidence,
    pub resulting_class: DataClass,
    pub fingerprint: SecretFingerprint,
    pub preview: RedactedPreview,
    pub detected_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RedactionTransformation {
    SpanMasked { start: usize, end: usize },
    LineOmitted { line_number: usize },
    FileOmitted,
    StructuredFieldRemoved { field: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RedactionDecision {
    ConfirmSecret,
    ReclassifyFinding,
    AllowForPathRule,
    AllowForExactFingerprint,
    KeepRedacted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedactionRecord {
    pub redaction_id: RedactionId,
    pub source: SourceReference,
    pub original_content_digest: ContentDigest,
    pub redacted_content_digest: ContentDigest,
    pub finding_ids: Vec<FindingId>,
    pub transformations: Vec<RedactionTransformation>,
    pub resulting_data_class: DataClass,
    pub scanner_version: ScannerVersion,
    pub created_at: Timestamp,
    pub user_decision: Option<RedactionDecision>,
}

//! Secret-scanning contract.
//!
//! The trait lives in the domain layer, not in `lokai-context`, so that
//! redaction on the outbound inference boundary does not depend on the context
//! pipeline. `lokai-secrets` implements it; `lokai-broker` consumes it at the
//! Infer chokepoint; `lokai-context` re-exports it for the seal stage.

use crate::workspace::ContentDigest;
use serde::{Deserialize, Serialize};

/// Proof that content was redacted, without storing the secret itself.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RedactionRecordReference {
    pub original_digest: ContentDigest,
    pub redacted_digest: ContentDigest,
}

/// `None` means no secrets found and the caller should use the original text.
/// `Some((records, text))` means secrets were found; `text` is the safe
/// replacement and may be empty when the content had to be omitted entirely.
pub type ScanOutcome = Option<(Vec<RedactionRecordReference>, String)>;

/// Immutable authority supplied with one scan, never installed on a shared scanner.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScanContext<'a> {
    pub session_id: Option<&'a str>,
    pub project_id: Option<&'a str>,
}

#[async_trait::async_trait]
pub trait SecretScanner: Send + Sync {
    /// Scans text for secrets. Returns `None` if no secrets are found,
    /// otherwise returns the redaction records and the redacted text.
    async fn scan_and_redact(
        &self,
        content: &str,
        path: Option<&str>,
    ) -> Result<ScanOutcome, String>;

    /// Scan with request-local authority. Scanners without scoped exceptions
    /// conservatively use their ordinary scanning behavior.
    async fn scan_and_redact_in_context(
        &self,
        content: &str,
        path: Option<&str>,
        _context: ScanContext<'_>,
    ) -> Result<ScanOutcome, String> {
        self.scan_and_redact(content, path).await
    }
}

/// One redaction applied to outbound inference content, before dispatch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundRedaction {
    pub session_id: Option<String>,
    pub model: String,
    /// Role of the message that carried the secret (`user`, `tool`, …).
    pub role: String,
    pub message_index: usize,
    /// True when the content could not be safely redacted and was dropped
    /// entirely rather than sent.
    pub omitted: bool,
    pub records: Vec<RedactionRecordReference>,
}

/// Durable audit trail for redactions on the outbound inference boundary.
///
/// A failed write must fail closed at the Infer chokepoint: never send (or
/// continue a turn with secrets) if the redaction cannot be recorded.
pub trait OutboundRedactionSink: Send + Sync {
    fn record(&self, event: &OutboundRedaction) -> Result<(), String>;
}

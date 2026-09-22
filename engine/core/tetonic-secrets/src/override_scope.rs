//! Override scope types for secret fingerprint allows (R12).

use serde::{Deserialize, Serialize};

/// Where a fingerprint allow applies.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "id")]
pub enum OverrideScope {
    Global,
    Session(String),
    Project(String),
}

impl OverrideScope {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Session(_) => "session",
            Self::Project(_) => "project",
        }
    }

    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Global => None,
            Self::Session(s) | Self::Project(s) => Some(s.as_str()),
        }
    }

    /// Whether an override with `self` applies during a scan under `scan`.
    /// Global always applies. Session/project require an exact match on the
    /// active scan context (out-of-scope overrides are ineffective).
    pub fn applies_during(&self, scan: Option<&OverrideScope>) -> bool {
        match self {
            Self::Global => true,
            Self::Session(s) => matches!(scan, Some(OverrideScope::Session(x)) if x == s),
            Self::Project(p) => matches!(scan, Some(OverrideScope::Project(x)) if x == p),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScopedFingerprint {
    pub fingerprint: String,
    pub scope: OverrideScope,
}

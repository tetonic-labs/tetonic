//! Domain pack abstraction (M9-DP).
//!
//! A DomainPack defines what "work" and "verification" mean for a specialized domain
//! (coding, research, operations, analysis). The orchestrator and supervisor interact
//! with packs through this trait, ensuring that no domain-specific knowledge leaks into
//! the generic substrate.

use std::path::Path;
use std::sync::Arc;

use crate::specialist::SpecialistPack;

/// Manifest declaring domain pack identity, capabilities, and metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackManifest {
    pub id: String,
    pub display_name: String,
    pub version: String,
    pub description: String,
    pub allowed_capabilities: Vec<String>,
}

impl PackManifest {
    pub fn new(
        id: impl Into<String>,
        display_name: impl Into<String>,
        version: impl Into<String>,
        description: impl Into<String>,
        allowed_capabilities: Vec<String>,
    ) -> Self {
        Self {
            id: id.into(),
            display_name: display_name.into(),
            version: version.into(),
            description: description.into(),
            allowed_capabilities,
        }
    }
}

/// A DomainPack defines the domain's roles, capability rules, and verification semantics.
pub trait DomainPack: Send + Sync {
    /// Return the static manifest for this pack.
    fn manifest(&self) -> &PackManifest;

    /// Return the conversational specialist pack for role overlays and step limits.
    fn specialist_pack(&self) -> Arc<dyn SpecialistPack>;

    /// Resolve the default verification command for this domain and workspace.
    /// E.g. for coding: detect cargo/pytest/npm test runners.
    fn default_verification_command(
        &self,
        workspace: &Path,
        explicit_override: Option<&str>,
    ) -> Option<String>;

    /// Validate whether a candidate artifact conforms to domain requirements.
    fn validate_artifact(&self, _kind: &str, _content: &[u8]) -> Result<(), String> {
        Ok(())
    }
}

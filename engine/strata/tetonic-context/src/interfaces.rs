use async_trait::async_trait;

use crate::types::{
    ContextEvidence, ContextRelationship, DiffContext, MemoryReference, RepositorySummary,
    TestReference, WorkspacePath,
};

#[async_trait]
pub trait ContextSourceProvider: Send + Sync {
    async fn repository_file_inventory(&self) -> Result<Vec<WorkspacePath>, String>;
    async fn search_text(&self, query: &str) -> Result<Vec<ContextEvidence>, String>;
    async fn search_symbols(&self, query: &str) -> Result<Vec<ContextEvidence>, String>;
    async fn get_definitions(&self, symbol: &str) -> Result<Vec<ContextEvidence>, String>;
    async fn get_references(&self, symbol: &str) -> Result<Vec<ContextEvidence>, String>;
    async fn get_lsp_relationships(&self, symbol: &str)
        -> Result<Vec<ContextRelationship>, String>;
    async fn get_current_diff(&self) -> Result<Option<DiffContext>, String>;
    async fn get_git_status(&self) -> Result<Vec<WorkspacePath>, String>;
    async fn get_relevant_tests(&self, query: &str) -> Result<Vec<TestReference>, String>;
    async fn get_project_memory(&self) -> Result<Vec<MemoryReference>, String>;
    async fn get_architecture_summary(&self) -> Result<Option<RepositorySummary>, String>;
    async fn get_file_content(&self, path: &WorkspacePath) -> Result<String, String>;

    /// Optional coding skeletonizer. Default is identity (no FS read).
    fn skeletonize_text(&self, path: &str, text: &str) -> String {
        let _ = path;
        text.to_string()
    }
}

pub use tetonic_domain::secrets::SecretScanner;

/// Trusted host authorization, independent of model-supplied retrieval input.
/// Implementations recheck current grants; errors must not contain protected data.
#[async_trait]
pub trait ContextAccessGate: Send + Sync {
    async fn authorize(&self, session: &tetonic_domain::SessionId) -> Result<(), ()>;
}

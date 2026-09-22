use serde::{Deserialize, Serialize};

use lokai_domain::{
    classify::DataClass,
    ids::{ArtifactId, EvidenceId, ExpansionHandleId, RunId, SessionId, TaskId},
    run::{Timestamp, TraceContext},
    workspace::{ContentDigest, WorkspaceVersion},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PathPolicy {
    pub rules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenBudget {
    pub max_tokens: usize,
    pub safety_reserve: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetrievalProfile {
    pub version: String,
    pub max_candidates: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextRequest {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub task_id: TaskId,
    pub objective: String,
    pub workspace_version: WorkspaceVersion,
    pub data_class_ceiling: DataClass,
    pub allowed_paths: PathPolicy,
    pub excluded_paths: PathPolicy,
    pub token_budget: TokenBudget,
    pub retrieval_profile: RetrievalProfile,
    pub prior_artifacts: Vec<ArtifactId>,
    pub trace_context: TraceContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPack {
    pub context_pack_id: ArtifactId,
    pub schema_version: u32,
    pub run_id: RunId,
    pub task_id: TaskId,
    pub workspace_version: WorkspaceVersion,
    pub objective_digest: ContentDigest,
    pub repository_summary: RepositorySummary,
    pub evidence: Vec<ContextEvidence>,
    pub relationships: Vec<ContextRelationship>,
    pub current_diff: Option<DiffContext>,
    pub relevant_tests: Vec<TestReference>,
    pub prior_decisions: Vec<MemoryReference>,
    pub expansion_handles: Vec<ExpansionHandle>,
    pub omissions: Vec<ContextOmission>,
    pub redactions: Vec<RedactionRecordReference>,
    pub token_usage: TokenUsage,
    pub data_class: DataClass,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContextSource {
    RepositoryFile,
    SymbolIndex,
    Lsp,
    GitDiff,
    ProjectMemory,
    Artifact,
    UserProvided,
    ToolResult,
}

pub type WorkspacePath = String;
pub type SymbolId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ByteRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RetrievalMethod {
    LexicalSearch,
    SymbolSearch,
    Definition,
    Reference,
    LspCall,
    GitDiff,
    TestHeuristic,
    ProjectMemory,
    DependencyGraph,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankingReason {
    pub description: String,
    pub score_contribution: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextEvidence {
    pub evidence_id: EvidenceId,
    pub source: ContextSource,
    pub repository_path: Option<WorkspacePath>,
    pub symbol_id: Option<SymbolId>,
    pub byte_range: Option<ByteRange>,
    pub line_range: Option<LineRange>,
    pub content_digest: ContentDigest,
    pub workspace_version: WorkspaceVersion,
    pub index_generation: Option<u64>,
    pub retrieval_method: RetrievalMethod,
    pub relevance_score: f32,
    pub ranking_reasons: Vec<RankingReason>,
    pub data_class: DataClass,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContextRelationshipKind {
    Defines,
    References,
    Calls,
    Implements,
    Imports,
    DependsOn,
    Tests,
    ModifiedWith,
    Supersedes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextRelationship {
    pub kind: ContextRelationshipKind,
    pub source_evidence_id: EvidenceId,
    pub target_evidence_id: EvidenceId,
    pub provenance: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffContext {
    pub unified_diff: String,
    pub files_changed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestReference {
    pub test_name: String,
    pub path: WorkspacePath,
    pub relevance_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryReference {
    pub memory_id: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpansionQuery {
    pub kind: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpansionScope {
    pub allowed_paths: PathPolicy,
    pub excluded_paths: PathPolicy,
    pub max_depth: usize,
}

pub type PolicyVersion = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpansionHandle {
    pub session_id: SessionId,
    pub handle_id: ExpansionHandleId,
    pub run_id: RunId,
    pub task_id: TaskId,
    pub workspace_version: WorkspaceVersion,
    pub query: ExpansionQuery,
    pub allowed_scope: ExpansionScope,
    pub data_class_ceiling: DataClass,
    pub expires_at: Timestamp,
    pub max_uses: u32,
    pub policy_version: PolicyVersion,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextOmission {
    pub reason: String,
    pub path: Option<WorkspacePath>,
}

pub use lokai_domain::secrets::RedactionRecordReference;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub objective: usize,
    pub repository_map: usize,
    pub evidence: usize,
    pub relationships: usize,
    pub tests: usize,
    pub diff: usize,
    pub prior_decisions: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositorySummary {
    pub languages: Vec<String>,
    pub major_modules: Vec<String>,
    pub entry_points: Vec<String>,
    pub build_systems: Vec<String>,
    pub test_systems: Vec<String>,
    pub current_branch: String,
    pub is_dirty: bool,
    pub architectural_boundaries: Vec<String>,
    pub target_subsystem: Option<String>,
    pub neighboring_subsystems: Vec<String>,
}

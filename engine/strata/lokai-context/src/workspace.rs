//! Production [`ContextSourceProvider`] backed by the live workspace (R4-1, CAP-01).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use sha2::{Digest, Sha256};

use crate::interfaces::ContextSourceProvider;
use crate::pipeline::ContextCompiler;
use crate::types::{
    ContextEvidence, ContextRelationship, ContextSource, DiffContext, MemoryReference,
    RepositorySummary, RetrievalMethod, TestReference, WorkspacePath,
};
use lokai_domain::artifact::ArtifactStore;
use lokai_domain::classify::DataClass;
use lokai_domain::code_index::{CodeIndexOpen, TextSkeleton};
use lokai_domain::ids::EvidenceId;
use lokai_domain::workspace::{
    ContentDigest, RepositoryId, WorkspaceVersion, WorkspaceVersionScheme,
};

/// Injected skip-symlink check (composition supplies `lokai-transaction`).
pub type SkipSymlink = Arc<dyn Fn(&Path) -> bool + Send + Sync>;
/// Injected jailed read: `(workspace_root, rel) -> contents`.
pub type JailedRead = Arc<dyn Fn(&Path, &str) -> Result<String, String> + Send + Sync>;
/// Injected sandboxed git: `(workspace_root, args) -> output`.
pub type RunGit = Arc<dyn Fn(&Path, &[&str]) -> Result<String, String> + Send + Sync>;

/// Filesystem hooks for [`WorkspaceContextProvider`]. No `lokai-tools` types.
#[derive(Clone)]
pub struct ContextFsHooks {
    pub skip_symlink: SkipSymlink,
    pub jailed_read: JailedRead,
    pub run_git: RunGit,
}

/// Filesystem + optional code-index + git inputs for [`ContextCompiler`].
pub struct WorkspaceContextProvider {
    root: PathBuf,
    index_db: Option<PathBuf>,
    memory_db: Option<PathBuf>,
    code_index: Option<Arc<dyn CodeIndexOpen>>,
    skeleton: Option<Arc<dyn TextSkeleton>>,
    hooks: ContextFsHooks,
}

impl WorkspaceContextProvider {
    pub fn new(
        root: impl Into<PathBuf>,
        index_db: Option<PathBuf>,
        memory_db: Option<PathBuf>,
        hooks: ContextFsHooks,
    ) -> Self {
        Self {
            root: root.into(),
            index_db,
            memory_db,
            code_index: None,
            skeleton: None,
            hooks,
        }
    }

    pub fn with_code_index(mut self, opener: Arc<dyn CodeIndexOpen>) -> Self {
        self.code_index = Some(opener);
        self
    }

    pub fn with_skeleton(mut self, skeleton: Arc<dyn TextSkeleton>) -> Self {
        self.skeleton = Some(skeleton);
        self
    }

    fn placeholder_version() -> WorkspaceVersion {
        WorkspaceVersion {
            repository_id: RepositoryId("workspace".into()),
            version_scheme: WorkspaceVersionScheme::Git,
            git_head: None,
            dirty_state_digest: ContentDigest("pending".into()),
            tracked_state_digest: ContentDigest("pending".into()),
            relevant_path_digests: BTreeMap::new(),
            index_generation: None,
        }
    }

    fn rel_path(&self, path: &Path) -> Option<String> {
        path.strip_prefix(&self.root)
            .ok()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
    }

    fn evidence_from_text(
        &self,
        text: String,
        path: Option<String>,
        source: ContextSource,
        method: RetrievalMethod,
        score: f32,
    ) -> ContextEvidence {
        let digest = ContentDigest(hex::encode(Sha256::digest(text.as_bytes())));
        ContextEvidence {
            evidence_id: EvidenceId::new(format!("ev_{}", &digest.0[..digest.0.len().min(12)])),
            source,
            repository_path: path,
            symbol_id: None,
            byte_range: None,
            line_range: None,
            content_digest: digest,
            workspace_version: Self::placeholder_version(),
            index_generation: None,
            retrieval_method: method,
            relevance_score: score,
            ranking_reasons: vec![],
            data_class: DataClass::RepositorySource,
            text,
        }
    }

    fn walk_files(&self, max: usize) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let walker = ignore::WalkBuilder::new(&self.root)
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .follow_links(false)
            .build();
        for entry in walker.flatten() {
            let path = entry.path();
            if (self.hooks.skip_symlink)(path) {
                continue;
            }
            if !path.is_file() {
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.ends_with(".db") {
                continue;
            }
            out.push(path.to_path_buf());
            if out.len() >= max {
                break;
            }
        }
        out
    }

    fn jailed_read_abs(&self, path: &Path) -> Result<String, String> {
        let rel = self
            .rel_path(path)
            .ok_or_else(|| format!("path outside workspace: {}", path.display()))?;
        self.jailed_read_rel(&rel)
    }

    fn jailed_read_rel(&self, rel: &str) -> Result<String, String> {
        (self.hooks.jailed_read)(&self.root, rel)
    }

    fn scan_text(&self, query: &str, limit: usize) -> Vec<ContextEvidence> {
        let tokens: Vec<String> = query
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|t| t.len() >= 3)
            .map(|t| t.to_lowercase())
            .collect();
        if tokens.is_empty() {
            return vec![];
        }
        let mut hits = Vec::new();
        for path in self.walk_files(400) {
            let Ok(content) = self.jailed_read_abs(&path) else {
                continue;
            };
            let lower = content.to_lowercase();
            if !tokens.iter().any(|t| lower.contains(t)) {
                continue;
            }
            let rel = self
                .rel_path(&path)
                .unwrap_or_else(|| path.display().to_string());
            let snippet: String = content.chars().take(2_000).collect();
            hits.push(self.evidence_from_text(
                snippet,
                Some(rel),
                ContextSource::RepositoryFile,
                RetrievalMethod::LexicalSearch,
                0.7,
            ));
            if hits.len() >= limit {
                break;
            }
        }
        hits
    }

    fn index_search(&self, query: &str, limit: u32) -> Result<Vec<ContextEvidence>, String> {
        let db = self
            .index_db
            .as_ref()
            .ok_or_else(|| "code index not configured".to_string())?;
        let opener = self
            .code_index
            .as_ref()
            .ok_or_else(|| "code index opener not injected".to_string())?;
        let idx = opener.open(db)?;
        let ws = idx.workspace_key(&self.root);
        let hits = idx.search(&ws, query, limit)?;
        let mut out = Vec::new();
        for h in hits {
            let text = if h.preview.is_empty() {
                format!("{}:{}-{}", h.rel, h.start_line, h.end_line)
            } else {
                h.preview.clone()
            };
            out.push(self.evidence_from_text(
                text,
                Some(h.rel),
                ContextSource::SymbolIndex,
                RetrievalMethod::LexicalSearch,
                h.score,
            ));
        }
        Ok(out)
    }

    fn run_git_blocking(&self, args: &[&str]) -> Result<String, String> {
        (self.hooks.run_git)(&self.root, args)
    }

    fn clone_for_git(&self) -> Self {
        Self {
            root: self.root.clone(),
            index_db: self.index_db.clone(),
            memory_db: self.memory_db.clone(),
            code_index: self.code_index.clone(),
            skeleton: self.skeleton.clone(),
            hooks: self.hooks.clone(),
        }
    }
}

#[async_trait]
impl ContextSourceProvider for WorkspaceContextProvider {
    async fn repository_file_inventory(&self) -> Result<Vec<WorkspacePath>, String> {
        Ok(self
            .walk_files(2_000)
            .into_iter()
            .filter_map(|p| self.rel_path(&p))
            .collect())
    }

    async fn search_text(&self, query: &str) -> Result<Vec<ContextEvidence>, String> {
        if self.index_db.is_some() && self.code_index.is_some() {
            match self.index_search(query, 20) {
                Ok(hits) if !hits.is_empty() => return Ok(hits),
                Ok(_) => {}
                Err(e) => tracing::warn!("index search degraded: {e}"),
            }
        }
        Ok(self.scan_text(query, 20))
    }

    async fn search_symbols(&self, query: &str) -> Result<Vec<ContextEvidence>, String> {
        if self.index_db.is_some() && self.code_index.is_some() {
            match self.index_search(query, 20) {
                Ok(hits) if !hits.is_empty() => return Ok(hits),
                Ok(_) => {}
                Err(e) => tracing::warn!("index symbol search degraded: {e}"),
            }
        }
        Ok(self.scan_text(query, 20))
    }

    async fn get_definitions(&self, symbol: &str) -> Result<Vec<ContextEvidence>, String> {
        self.search_symbols(symbol).await
    }

    async fn get_references(&self, symbol: &str) -> Result<Vec<ContextEvidence>, String> {
        self.search_symbols(symbol).await
    }

    async fn get_lsp_relationships(
        &self,
        _symbol: &str,
    ) -> Result<Vec<ContextRelationship>, String> {
        Ok(vec![])
    }

    async fn get_current_diff(&self) -> Result<Option<DiffContext>, String> {
        let provider = self.clone_for_git();
        let output = match tokio::task::spawn_blocking(move || {
            provider.run_git_blocking(&["diff", "--no-color", "-U3"])
        })
        .await
        {
            Ok(Ok(o)) => o,
            _ => return Ok(None),
        };
        let body = output
            .lines()
            .filter(|l| {
                let t = l.trim_start();
                !t.starts_with("exit code:")
                    && !t.starts_with("stdout:")
                    && !t.starts_with("stderr:")
                    && !t.starts_with("Process ran")
                    && !t.starts_with("WARNING:")
                    && !t.starts_with("warning:")
            })
            .collect::<Vec<_>>()
            .join("\n");
        let trimmed = body.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        let lower = trimmed.to_ascii_lowercase();
        if lower.contains("not a git repository") || lower.contains("usage: git diff") {
            return Ok(None);
        }
        let files_changed = trimmed
            .lines()
            .filter(|l| l.starts_with("diff --git"))
            .count();
        if files_changed == 0 {
            return Ok(None);
        }
        Ok(Some(DiffContext {
            unified_diff: trimmed.chars().take(20_000).collect(),
            files_changed,
        }))
    }

    async fn get_git_status(&self) -> Result<Vec<WorkspacePath>, String> {
        let provider = self.clone_for_git();
        let output = match tokio::task::spawn_blocking(move || {
            provider.run_git_blocking(&["status", "--porcelain"])
        })
        .await
        {
            Ok(Ok(o)) => o,
            _ => return Ok(vec![]),
        };
        let mut paths = Vec::new();
        for line in output.lines() {
            let t = line.trim();
            if t.len() < 4 {
                continue;
            }
            if let Some(path) = t.get(3..) {
                let path = path.trim().trim_matches('"');
                if !path.is_empty()
                    && !path.starts_with("exit")
                    && !path.contains("partial containment")
                {
                    paths.push(path.replace('\\', "/"));
                }
            }
        }
        Ok(paths)
    }

    async fn get_relevant_tests(&self, query: &str) -> Result<Vec<TestReference>, String> {
        let q = query.to_lowercase();
        let mut out = Vec::new();
        for path in self.walk_files(500) {
            let rel = match self.rel_path(&path) {
                Some(r) => r,
                None => continue,
            };
            let lower = rel.to_lowercase();
            if !(lower.contains("test") || lower.contains("spec")) {
                continue;
            }
            if !q.is_empty() {
                let Ok(content) = self.jailed_read_abs(&path) else {
                    continue;
                };
                if !lower.contains(&q) && !content.to_lowercase().contains(&q) {
                    continue;
                }
            }
            let test_name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("test")
                .to_string();
            out.push(TestReference {
                test_name,
                path: rel,
                relevance_score: 0.6,
            });
            if out.len() >= 10 {
                break;
            }
        }
        Ok(out)
    }

    async fn get_project_memory(&self) -> Result<Vec<MemoryReference>, String> {
        let mut out = Vec::new();
        for name in ["LOKAI.md", "AGENTS.md", ".lokai/project.md"] {
            if let Ok(text) = self.jailed_read_rel(name) {
                if !text.trim().is_empty() {
                    out.push(MemoryReference {
                        memory_id: name.into(),
                        summary: text.chars().take(1_500).collect(),
                    });
                }
            }
        }
        let _ = &self.memory_db;
        Ok(out)
    }

    async fn get_architecture_summary(&self) -> Result<Option<RepositorySummary>, String> {
        let mut languages = Vec::new();
        let mut entry_points = Vec::new();
        for path in self.walk_files(100) {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let lang = match ext {
                    "rs" => "rust",
                    "py" => "python",
                    "ts" | "tsx" => "typescript",
                    "js" | "jsx" => "javascript",
                    "go" => "go",
                    _ => continue,
                };
                if !languages.iter().any(|l| l == lang) {
                    languages.push(lang.to_string());
                }
            }
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if matches!(
                    name,
                    "main.rs" | "lib.rs" | "main.go" | "main.py" | "index.ts"
                ) {
                    if let Some(rel) = self.rel_path(&path) {
                        entry_points.push(rel);
                    }
                }
            }
        }
        if languages.is_empty() && entry_points.is_empty() {
            return Ok(None);
        }
        Ok(Some(RepositorySummary {
            languages,
            major_modules: vec![],
            entry_points,
            build_systems: vec![],
            test_systems: vec![],
            current_branch: String::new(),
            is_dirty: false,
            architectural_boundaries: vec![],
            target_subsystem: None,
            neighboring_subsystems: vec![],
        }))
    }

    async fn get_file_content(&self, path: &WorkspacePath) -> Result<String, String> {
        self.jailed_read_rel(path)
    }

    fn skeletonize_text(&self, path: &str, text: &str) -> String {
        match &self.skeleton {
            Some(sk) => sk.skeletonize(path, text),
            None => text.to_string(),
        }
    }
}

/// Build the production compiler used by CLI and daemon assembly (R4-1 / R4-3, CAP-01).
pub fn build_production_context_compiler(
    workspace_root: &Path,
    artifact_store: Arc<dyn ArtifactStore>,
    index_db: Option<PathBuf>,
    memory_db: Option<PathBuf>,
    hooks: ContextFsHooks,
) -> Arc<ContextCompiler> {
    build_production_context_compiler_injected(
        workspace_root,
        artifact_store,
        index_db,
        memory_db,
        None,
        None,
        hooks,
    )
}

pub fn build_production_context_compiler_injected(
    workspace_root: &Path,
    artifact_store: Arc<dyn ArtifactStore>,
    index_db: Option<PathBuf>,
    memory_db: Option<PathBuf>,
    code_index: Option<Arc<dyn CodeIndexOpen>>,
    skeleton: Option<Arc<dyn TextSkeleton>>,
    hooks: ContextFsHooks,
) -> Arc<ContextCompiler> {
    let mut provider = WorkspaceContextProvider::new(workspace_root, index_db, memory_db, hooks);
    if let Some(open) = code_index {
        provider = provider.with_code_index(open);
    }
    if let Some(sk) = skeleton {
        provider = provider.with_skeleton(sk);
    }
    let provider = Arc::new(provider);
    // Seal scanner is process-local; durable overrides hydrate on Infer/daemon/turn
    // scanners via SharedStore (opening a second Store on the live lokai.db path is unsafe).
    let scanner: Arc<dyn lokai_domain::secrets::SecretScanner> =
        Arc::new(lokai_secrets::scanner::ScannerEngine::default_engine());
    Arc::new(
        ContextCompiler::new(provider)
            .with_artifact_store(artifact_store)
            .with_secret_scanner(scanner),
    )
}

#[cfg(test)]
pub(crate) fn test_fs_hooks() -> ContextFsHooks {
    ContextFsHooks {
        skip_symlink: Arc::new(|path| {
            path.symlink_metadata()
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false)
        }),
        jailed_read: Arc::new(|root, rel| {
            if rel.contains("..") || rel.contains("escape") {
                return Err("path outside workspace".into());
            }
            let full = root.join(rel);
            std::fs::read_to_string(&full).map_err(|e| e.to_string())
        }),
        run_git: Arc::new(|_root, _args| Ok(String::new())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interfaces::ContextSourceProvider;

    #[tokio::test]
    async fn inventory_and_search_use_real_workspace_files() {
        let dir = std::env::temp_dir().join(format!("lokai-ctx-prov-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("unique_marker_r41.rs"),
            "fn r41_unique_symbol() {}\n",
        )
        .unwrap();
        let provider = WorkspaceContextProvider::new(&dir, None, None, test_fs_hooks());
        let inv = provider.repository_file_inventory().await.unwrap();
        assert!(
            inv.iter().any(|p| p.contains("unique_marker_r41")),
            "inventory={inv:?}"
        );
        let hits = provider.search_text("r41_unique_symbol").await.unwrap();
        assert!(!hits.is_empty(), "expected filesystem search hit");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn current_diff_none_outside_git_repo() {
        let dir = std::env::temp_dir().join(format!(
            "lokai-ctx-nogit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("README.md"), "# fixture\n").unwrap();
        let provider = WorkspaceContextProvider::new(&dir, None, None, test_fs_hooks());
        let diff = provider.get_current_diff().await.unwrap();
        assert!(
            diff.is_none(),
            "git usage/help must not become DiffContext: {diff:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    struct ShrinkSkeleton;
    impl TextSkeleton for ShrinkSkeleton {
        fn skeletonize(&self, _path: &str, _text: &str) -> String {
            "pub fn start(&self) { /* ... */ }".into()
        }
    }

    #[test]
    fn skeletonize_text_uses_injected_skeleton() {
        let dir = std::env::temp_dir().join(format!("lokai-ctx-skel-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let provider = WorkspaceContextProvider::new(&dir, None, None, test_fs_hooks())
            .with_skeleton(Arc::new(ShrinkSkeleton));
        let out = provider.skeletonize_text(
            "src/service.rs",
            "pub fn start(&self) {\n    let mut count = 0;\n    count += i;\n}\n",
        );
        assert!(out.contains("{ /* ... */ }"));
        assert!(!out.contains("count += i"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn compiler_quality_smoke() {
        use crate::pipeline::ContextCompiler;
        use crate::types::{ContextRequest, PathPolicy, RetrievalProfile, TokenBudget};
        use lokai_domain::classify::DataClass;
        use lokai_domain::ids::{RunId, SessionId, TaskId};
        use lokai_domain::workspace::{
            ContentDigest, RepositoryId, WorkspaceVersion, WorkspaceVersionScheme,
        };
        use std::collections::BTreeMap;

        let dir = std::env::temp_dir().join(format!(
            "lokai-r16-smoke-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("r16_smoke_fixture.rs"),
            "fn r16_smoke_target() { /* pinned fixture for R16 smoke gate */ }\n",
        )
        .unwrap();

        let provider = Arc::new(WorkspaceContextProvider::new(
            &dir,
            None,
            None,
            test_fs_hooks(),
        ));
        let compiler = ContextCompiler::new(provider);

        let ws_version = WorkspaceVersion {
            repository_id: RepositoryId("smoke".into()),
            version_scheme: WorkspaceVersionScheme::Git,
            git_head: None,
            dirty_state_digest: ContentDigest("smoke".into()),
            tracked_state_digest: ContentDigest("smoke".into()),
            relevant_path_digests: BTreeMap::new(),
            index_generation: None,
        };

        let request = ContextRequest {
            session_id: SessionId::new("smoke_session"),
            run_id: RunId::new("smoke_run"),
            task_id: TaskId::new("smoke_task"),
            objective: "Implement r16_smoke_target in r16_smoke_fixture.rs".into(),
            workspace_version: ws_version,
            data_class_ceiling: DataClass::RepositorySource,
            allowed_paths: PathPolicy { rules: vec![] },
            excluded_paths: PathPolicy { rules: vec![] },
            token_budget: TokenBudget {
                max_tokens: 8_000,
                safety_reserve: 200,
            },
            retrieval_profile: RetrievalProfile {
                version: "1.0".into(),
                max_candidates: 20,
            },
            prior_artifacts: vec![],
            trace_context: Default::default(),
        };

        let pack = compiler
            .compile(request)
            .await
            .expect("R16 smoke: compile must succeed");

        assert_ne!(
            pack.context_pack_id.0, "pending",
            "R16 AC1: context_pack_id must be sealed (not 'pending'), got: {}",
            pack.context_pack_id.0
        );

        assert!(
            pack.objective_digest.0.starts_with("sha256:"),
            "R16 AC2: objective_digest must start with 'sha256:', got: {}",
            pack.objective_digest.0
        );

        assert_eq!(
            pack.data_class,
            DataClass::RepositorySource,
            "R16 AC3: data_class must equal the ceiling"
        );

        let fixture_ev = pack.evidence.iter().find(|ev| {
            ev.repository_path
                .as_deref()
                .is_some_and(|p| p.contains("r16_smoke"))
        });
        assert!(
            fixture_ev.is_some(),
            "R16 AC4: expected >=1 evidence item from r16_smoke_fixture.rs, got: {:?}",
            pack.evidence
                .iter()
                .map(|e| e.repository_path.clone())
                .collect::<Vec<_>>()
        );
        let ev = fixture_ev.unwrap();
        assert!(
            !ev.content_digest.0.is_empty(),
            "R16 AC5: fixture evidence must have a non-empty content_digest"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn get_file_content_refuses_parentdir_and_symlink() {
        let dir = std::env::temp_dir().join(format!("lokai-ctx-jail-{}", std::process::id()));
        let outside =
            std::env::temp_dir().join(format!("lokai-ctx-jail-out-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), "secret-bytes").unwrap();
        let provider = WorkspaceContextProvider::new(&dir, None, None, test_fs_hooks());
        let err = provider
            .get_file_content(&"../secret.txt".to_string())
            .await
            .unwrap_err();
        assert!(
            !err.contains("secret-bytes"),
            "error must not include host bytes: {err}"
        );
        let link = dir.join("escape");
        let linked = {
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(&outside, &link).is_ok()
            }
            #[cfg(windows)]
            {
                std::os::windows::fs::symlink_dir(&outside, &link).is_ok()
            }
        };
        if linked {
            let err = provider
                .get_file_content(&"escape/secret.txt".to_string())
                .await
                .unwrap_err();
            assert!(!err.contains("secret-bytes"), "symlink leak: {err}");
        }
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
    }
}

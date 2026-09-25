mod admission;
pub mod stage1_normalize;
pub mod stage2_retrieve;
pub mod stage3_filter;
pub mod stage4_rank;
pub mod stage5_dedupe;
pub mod stage6_budget;
pub mod stage7_seal;

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use crate::interfaces::ContextSourceProvider;
use crate::types::*;
use std::sync::Arc;
use tetonic_domain::artifact::ArtifactStore;
use tetonic_domain::ids::EvidenceId;

/// Structured compilation failure — never silently pretended to succeed.
#[derive(Debug)]
pub enum CompilationFailure {
    /// Pipeline stage failed with an error message.
    StageError(String),
    /// Workspace state changed during compilation — pack rejected.
    StaleWorkspace { expected: String, actual: String },
    /// Budget configuration is invalid.
    BadBudget(String),
}

impl std::fmt::Display for CompilationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StageError(msg) => write!(f, "compilation stage error: {msg}"),
            Self::StaleWorkspace { expected, actual } => {
                write!(
                    f,
                    "workspace changed during compilation (expected={expected}, actual={actual})"
                )
            }
            Self::BadBudget(msg) => write!(f, "invalid budget configuration: {msg}"),
        }
    }
}

/// Metrics recorded by a shadow compile run (R15).
///
/// A shadow compile runs the full pipeline alongside the live turn path to
/// compare quality; it never changes what the model receives.
#[derive(Debug, Clone)]
pub struct ShadowCompileRecord {
    /// `context_pack_id` of the shadow pack, empty if compilation failed.
    pub pack_id: String,
    /// Objective digest (`sha256:…`) of the request, empty if compilation failed.
    pub objective_digest: String,
    /// Number of evidence excerpts in the shadow pack.
    pub excerpt_count: usize,
    /// Total token usage reported by the shadow pack.
    pub total_tokens: usize,
    /// Set when the shadow compile failed; the live turn is unaffected.
    pub error: Option<String>,
}

/// Expansion state tracked per handle.
#[derive(Debug, Clone)]
pub struct HandleState {
    pub handle: ExpansionHandle,
    pub uses: u32,
}

#[derive(Debug, Clone)]
pub struct ExpansionLimits {
    pub max_concurrent: usize,
    pub max_concurrent_per_owner: usize,
    pub max_live_handles: usize,
    pub max_evidence_items: usize,
    pub max_text_bytes: usize,
    pub timeout: std::time::Duration,
}

impl Default for ExpansionLimits {
    fn default() -> Self {
        Self {
            max_concurrent: 8,
            max_concurrent_per_owner: 2,
            max_live_handles: 1024,
            max_evidence_items: 256,
            max_text_bytes: 4 * 1024 * 1024,
            timeout: std::time::Duration::from_secs(30),
        }
    }
}

pub struct ContextCompiler {
    access_gate: Option<Arc<dyn crate::interfaces::ContextAccessGate>>,
    pub provider: Arc<dyn ContextSourceProvider>,
    pub artifact_store: Option<Arc<dyn ArtifactStore>>,
    pub secret_scanner: Option<Arc<dyn crate::interfaces::SecretScanner>>,
    pub handles: Mutex<HashMap<String, HandleState>>,
    expansion_limits: ExpansionLimits,
    expansion_admission: admission::Admission,
    // None fails the whole compiler closed after bounded revocation capacity.
    revoked_sessions: Mutex<Option<HashSet<tetonic_domain::SessionId>>>,
}

impl ContextCompiler {
    pub fn new(provider: Arc<dyn ContextSourceProvider>) -> Self {
        Self {
            provider,
            access_gate: None,
            artifact_store: None,
            secret_scanner: None,
            handles: Mutex::new(HashMap::new()),
            expansion_limits: ExpansionLimits::default(),
            expansion_admission: admission::Admission::default(),
            revoked_sessions: Mutex::new(Some(HashSet::new())),
        }
    }

    /// Additional membership gate, not a substitute for a scoped provider or
    /// workspace/artifact grants. Legacy local composition has no such gate.
    pub fn with_access_gate(mut self, gate: Arc<dyn crate::interfaces::ContextAccessGate>) -> Self {
        self.access_gate = Some(gate);
        self
    }

    async fn authorize_context(&self, session: &tetonic_domain::SessionId) -> Result<(), String> {
        if let Some(gate) = &self.access_gate {
            gate.authorize(session)
                .await
                .map_err(|_| "context access denied".to_string())?;
        }
        Ok(())
    }

    pub fn with_expansion_limits(mut self, limits: ExpansionLimits) -> Self {
        self.expansion_limits = limits;
        self
    }

    pub fn with_artifact_store(mut self, store: Arc<dyn ArtifactStore>) -> Self {
        self.artifact_store = Some(store);
        self
    }

    pub fn with_secret_scanner(
        mut self,
        scanner: Arc<dyn crate::interfaces::SecretScanner>,
    ) -> Self {
        self.secret_scanner = Some(scanner);
        self
    }

    /// Compile a full `ContextPack` for the given request.
    ///
    /// On failure returns a `CompilationFailure` — never silently pretends
    /// compilation succeeded, never uses stale cache, never downgrades classification.
    pub async fn compile(
        &self,
        request: ContextRequest,
    ) -> Result<ContextPack, CompilationFailure> {
        let session = request.session_id.clone();
        self.authorize_context(&session)
            .await
            .map_err(CompilationFailure::StageError)?;
        {
            let revoked = self.revoked_sessions.lock().map_err(|_| {
                CompilationFailure::StageError("revocation registry unavailable".into())
            })?;
            if revoked
                .as_ref()
                .map_or(true, |sessions| sessions.contains(&session))
            {
                return Err(CompilationFailure::StageError(
                    "context session revoked".into(),
                ));
            }
        }
        let _initial_fp = request.workspace_version.state_fingerprint();

        // Stage 1: Normalize
        let normalized =
            stage1_normalize::normalize(&request).map_err(CompilationFailure::StageError)?;

        // Stage 2: Retrieve candidates
        let candidates = stage2_retrieve::retrieve(&normalized, &request, self.provider.as_ref())
            .await
            .map_err(CompilationFailure::StageError)?;

        // Stage 3: Filter (path policy + data-class ceiling)
        let (filtered, filter_omissions) = stage3_filter::filter(
            candidates,
            &request.data_class_ceiling,
            &request.allowed_paths,
            &request.excluded_paths,
        )
        .map_err(CompilationFailure::StageError)?;

        // Stage 4: Rank
        let ranked =
            stage4_rank::rank(filtered, &normalized).map_err(CompilationFailure::StageError)?;

        // Stage 5: Deduplicate
        let deduped = stage5_dedupe::deduplicate(ranked).map_err(CompilationFailure::StageError)?;

        // Stage 6: Budget
        let (budgeted, budget_omissions, token_usage) =
            stage6_budget::apply_budget(deduped, &request.token_budget)
                .map_err(CompilationFailure::BadBudget)?;

        let all_omissions: Vec<ContextOmission> = filter_omissions
            .into_iter()
            .chain(budget_omissions)
            .collect();

        // Fetch repository summary (optional — degrade if unavailable)
        let repo_summary = match self.provider.get_architecture_summary().await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("architecture summary unavailable (degrading): {e}");
                None
            }
        };

        self.authorize_context(&session)
            .await
            .map_err(CompilationFailure::StageError)?;
        // Stage 7: Seal (includes workspace revalidation)
        let store_ref = self.artifact_store.as_ref().map(|s| s.as_ref());
        let scanner_ref = self.secret_scanner.as_ref().map(|s| s.as_ref());
        let pack = stage7_seal::seal(
            budgeted,
            request,
            all_omissions,
            token_usage,
            repo_summary,
            store_ref,
            scanner_ref,
        )
        .await
        .map_err(CompilationFailure::StageError)?;

        self.authorize_context(&session)
            .await
            .map_err(CompilationFailure::StageError)?;
        // Register expansion handles for future expand() calls
        {
            // Serialize admission with revocation; a compile that was awaiting
            // retrieval when the session closed must not recreate authority.
            let revoked = self.revoked_sessions.lock().map_err(|_| {
                CompilationFailure::StageError("revocation registry unavailable".into())
            })?;
            if revoked
                .as_ref()
                .map_or(true, |sessions| sessions.contains(&session))
            {
                return Err(CompilationFailure::StageError(
                    "context session revoked".into(),
                ));
            }
            let mut h = self.handles.lock().unwrap();
            h.retain(|_, state| {
                state.handle.expires_at > Utc::now() && state.uses < state.handle.max_uses
            });
            if h.len().saturating_add(pack.expansion_handles.len())
                > self.expansion_limits.max_live_handles
            {
                return Err(CompilationFailure::StageError(
                    "live expansion handle limit reached".into(),
                ));
            }
            for handle in &pack.expansion_handles {
                h.insert(
                    handle.handle_id.0.clone(),
                    HandleState {
                        handle: handle.clone(),
                        uses: 0,
                    },
                );
            }
        }

        Ok(pack)
    }

    /// Shadow-mode compile (R15): runs the full 7-stage pipeline alongside the live path,
    /// records digests/metrics, and **never** modifies the tokens sent to the model.
    ///
    /// Failures are caught and recorded in `ShadowCompileRecord::error`; they never
    /// propagate to the caller. The return value is always `Ok`.
    pub async fn compile_shadow(&self, request: ContextRequest) -> ShadowCompileRecord {
        match self.compile(request).await {
            Ok(pack) => ShadowCompileRecord {
                pack_id: pack.context_pack_id.0.clone(),
                objective_digest: pack.objective_digest.0.clone(),
                excerpt_count: pack.evidence.len(),
                total_tokens: pack.token_usage.total,
                error: None,
            },
            Err(e) => ShadowCompileRecord {
                pack_id: String::new(),
                objective_digest: String::new(),
                excerpt_count: 0,
                total_tokens: 0,
                error: Some(e.to_string()),
            },
        }
    }

    /// Expand a context handle.
    ///
    /// Enforces:
    /// - Handle validity (must have been issued by this compiler)
    /// - Exact session/run/task ownership supplied by the trusted caller
    /// - Expiry (`expires_at`)
    /// - Use count (`max_uses`)
    /// - Workspace staleness (workspace must not have changed since the handle was issued)
    /// - Scope (retrieved paths must be within `allowed_scope`)
    /// - Data-class ceiling
    pub async fn expand_pack(
        &self,
        request: &tetonic_domain::ContextExpansionRequest,
    ) -> Result<Vec<ContextEvidence>, String> {
        let _permit = self
            .expansion_admission
            .enter(request, &self.expansion_limits)?;
        tokio::time::timeout(
            self.expansion_limits.timeout,
            self.expand_within_limits(request),
        )
        .await
        .map_err(|_| "context expansion deadline exceeded".to_string())?
    }

    fn check_expansion_size(&self, evidence: &[ContextEvidence]) -> Result<(), String> {
        let bytes = evidence
            .iter()
            .fold(0usize, |sum, ev| sum.saturating_add(ev.text.len()));
        if evidence.len() > self.expansion_limits.max_evidence_items
            || bytes > self.expansion_limits.max_text_bytes
        {
            return Err("context expansion output exceeds configured limit".into());
        }
        Ok(())
    }

    async fn expand_within_limits(
        &self,
        request: &tetonic_domain::ContextExpansionRequest,
    ) -> Result<Vec<ContextEvidence>, String> {
        self.authorize_context(&request.session_id).await?;
        if request.session_id.0.trim().is_empty()
            || request.run_id.0.trim().is_empty()
            || request.task_id.0.trim().is_empty()
        {
            return Err("expansion requires a nonempty session, run and task".into());
        }
        let handle_id = request.handle_id.as_str();
        let current_workspace_fp = request.current_workspace_fp.as_str();
        // --- Look up and validate handle ---
        let handle = {
            let mut h = self.handles.lock().unwrap();
            if h.get(handle_id)
                .is_some_and(|state| Utc::now() >= state.handle.expires_at)
            {
                h.remove(handle_id);
                return Err(format!("expansion handle {handle_id} has expired"));
            }
            h.retain(|_, state| state.handle.expires_at > Utc::now());
            let state = h
                .get_mut(handle_id)
                .ok_or_else(|| "invalid or unknown expansion handle".to_string())?;

            if state.handle.session_id != request.session_id
                || state.handle.run_id != request.run_id
                || state.handle.task_id != request.task_id
            {
                return Err(
                    "expansion handle does not belong to this session, run and task".into(),
                );
            }

            // Expiry check
            if Utc::now() > state.handle.expires_at {
                return Err(format!("expansion handle {handle_id} has expired"));
            }

            // Use-count check
            if state.uses >= state.handle.max_uses {
                return Err(format!(
                    "expansion handle {handle_id} exhausted ({} / {} uses)",
                    state.uses, state.handle.max_uses
                ));
            }

            // Workspace staleness check
            let issued_fp = state.handle.workspace_version.state_fingerprint();
            if current_workspace_fp.is_empty() || issued_fp != current_workspace_fp {
                return Err(format!(
                    "expansion handle {handle_id} rejected: workspace changed since handle was \
                     issued (issued={issued_fp}, current={current_workspace_fp})"
                ));
            }

            state.uses += 1;
            state.handle.clone()
        };

        // --- Retrieve based on query kind ---
        let query = &handle.query.target;
        let results = match handle.query.kind.as_str() {
            "symbol" => self.provider.search_symbols(query).await?,
            "file" => {
                if !stage3_filter::path_is_allowed(
                    query,
                    &handle.allowed_scope.allowed_paths,
                    &handle.allowed_scope.excluded_paths,
                )? {
                    return Err("expansion target is outside the permitted scope".into());
                }
                let content = self.provider.get_file_content(query).await?;
                if content.is_empty() {
                    vec![]
                } else {
                    use sha2::{Digest, Sha256};
                    use tetonic_domain::workspace::ContentDigest;
                    let digest = {
                        let mut h = Sha256::new();
                        h.update(content.as_bytes());
                        ContentDigest(format!("{:x}", h.finalize()))
                    };
                    vec![ContextEvidence {
                        evidence_id: EvidenceId::new(uuid::Uuid::new_v4().to_string()),
                        source: ContextSource::RepositoryFile,
                        repository_path: Some(query.clone()),
                        symbol_id: None,
                        byte_range: None,
                        line_range: None,
                        content_digest: digest,
                        workspace_version: handle.workspace_version.clone(),
                        index_generation: handle.workspace_version.index_generation,
                        retrieval_method: RetrievalMethod::LexicalSearch,
                        relevance_score: 0.9,
                        ranking_reasons: vec![],
                        data_class: tetonic_domain::classify::DataClass::RepositorySource,
                        text: content,
                    }]
                }
            }
            _ => self.provider.search_text(query).await?,
        };

        // Share compile-time restrictions; expansion must not widen access.
        self.check_expansion_size(&results)?;
        let (filtered, _) = stage3_filter::filter(
            results,
            &handle.data_class_ceiling,
            &handle.allowed_scope.allowed_paths,
            &handle.allowed_scope.excluded_paths,
        )?;
        let mut results = Vec::new();
        for mut ev in filtered {
            if let Some(scanner) = &self.secret_scanner {
                if let Some((_, text)) = scanner
                    .scan_and_redact(&ev.text, ev.repository_path.as_deref())
                    .await?
                {
                    if text.is_empty() {
                        continue;
                    }
                    ev.text = text;
                }
            }
            // Digest describes the exact returned text, including redaction.
            use sha2::{Digest, Sha256};
            ev.content_digest = tetonic_domain::workspace::ContentDigest(format!(
                "{:x}",
                Sha256::digest(ev.text.as_bytes())
            ));
            results.push(ev);
        }

        self.check_expansion_size(&results)?;
        self.authorize_context(&request.session_id).await?;
        // Revocation may race provider/scanner awaits. Do not release the result
        // merely because the handle was valid when retrieval started.
        let handles = self
            .handles
            .lock()
            .map_err(|_| "expansion registry unavailable")?;
        if !handles.contains_key(handle_id) {
            return Err("context expansion authority was revoked".into());
        }
        Ok(results)
    }
}

#[async_trait::async_trait]
impl tetonic_domain::ContextCompiler for ContextCompiler {
    fn invalidate_session(&self, session: &tetonic_domain::SessionId) -> Result<(), String> {
        let mut revoked = self
            .revoked_sessions
            .lock()
            .map_err(|_| "revocation registry unavailable")?;
        let mut handles = self
            .handles
            .lock()
            .map_err(|_| "expansion registry unavailable")?;
        handles.retain(|_, state| &state.handle.session_id != session);
        if let Some(sessions) = revoked.as_mut() {
            if sessions.len() >= 1024 && !sessions.contains(session) {
                *revoked = None;
                handles.clear();
            } else {
                sessions.insert(session.clone());
            }
        }
        Ok(())
    }
    async fn compile(
        &self,
        req: tetonic_domain::ContextCompileRequest,
    ) -> Result<tetonic_domain::CompiledContext, String> {
        let request = ContextRequest {
            session_id: req.session_id,
            run_id: req.run_id,
            task_id: req.task_id,
            objective: req.objective,
            workspace_version: req.workspace_version,
            data_class_ceiling: req.data_class_ceiling,
            allowed_paths: PathPolicy { rules: vec![] },
            excluded_paths: PathPolicy { rules: vec![] },
            token_budget: TokenBudget {
                max_tokens: 4000,
                safety_reserve: 500,
            },
            retrieval_profile: RetrievalProfile {
                version: "1.0".into(),
                max_candidates: 20,
            },
            prior_artifacts: vec![],
            trace_context: Default::default(),
        };
        let pack = self.compile(request).await.map_err(|e| e.to_string())?;
        Ok(tetonic_domain::CompiledContext {
            run_id: pack.run_id,
            task_id: pack.task_id,
            workspace_version: pack.workspace_version,
            data_class: pack.data_class,
            evidence: pack
                .evidence
                .into_iter()
                .map(|ev| tetonic_domain::CompiledEvidence {
                    evidence_id: ev.evidence_id.0,
                    repository_path: ev.repository_path,
                    text: ev.text,
                })
                .collect(),
        })
    }

    async fn expand(
        &self,
        request: &tetonic_domain::ContextExpansionRequest,
    ) -> Result<Vec<tetonic_domain::CompiledEvidence>, String> {
        let evidence = self.expand_pack(request).await?;
        Ok(evidence
            .into_iter()
            .map(|ev| tetonic_domain::CompiledEvidence {
                evidence_id: ev.evidence_id.0,
                repository_path: ev.repository_path,
                text: ev.text,
            })
            .collect())
    }
}

use chrono::Utc;

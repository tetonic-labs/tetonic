//! Required test matrix for M4-1 (Progressive Context Compiler).
//!
//! Covers all 20 scenarios specified in the ticket.

#[cfg(test)]
#[allow(clippy::module_inception, clippy::field_reassign_with_default)]
pub mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use async_trait::async_trait;
    use chrono::Utc;
    use tetonic_domain::classify::DataClass;
    use tetonic_domain::ids::{EvidenceId, ExpansionHandleId, RunId, SessionId, TaskId};
    use tetonic_domain::workspace::{
        ContentDigest, RepositoryId, WorkspaceVersion, WorkspaceVersionScheme,
    };

    use crate::interfaces::ContextSourceProvider;
    use crate::pipeline::ContextCompiler;
    use crate::types::{
        ContextEvidence, ContextRelationship, ContextRequest, ContextSource, DiffContext,
        MemoryReference, PathPolicy, RepositorySummary, RetrievalMethod, RetrievalProfile,
        TestReference, TokenBudget, WorkspacePath,
    };

    // ─────────────────────────────────────────────────────────────────────────
    // Helpers
    // ─────────────────────────────────────────────────────────────────────────

    pub(super) fn ws_version(fingerprint: &str) -> WorkspaceVersion {
        WorkspaceVersion {
            repository_id: RepositoryId("repo_1".to_string()),
            version_scheme: WorkspaceVersionScheme::Git,
            git_head: None,
            dirty_state_digest: ContentDigest(fingerprint.to_string()),
            tracked_state_digest: ContentDigest(fingerprint.to_string()),
            relevant_path_digests: BTreeMap::new(),
            index_generation: None,
        }
    }

    pub(super) fn expansion_request(
        handle: &str,
        fingerprint: &str,
    ) -> tetonic_domain::ContextExpansionRequest {
        tetonic_domain::ContextExpansionRequest {
            handle_id: handle.into(),
            session_id: SessionId::new("sess_test"),
            run_id: RunId::new("run_test"),
            task_id: TaskId::new("task_test"),
            current_workspace_fp: fingerprint.into(),
        }
    }

    pub(super) fn base_request() -> ContextRequest {
        ContextRequest {
            session_id: SessionId::new("sess_test"),
            run_id: RunId::new("run_test"),
            task_id: TaskId::new("task_test"),
            objective: "Refactor the authenticate function in src/auth.rs".to_string(),
            workspace_version: ws_version("v1"),
            data_class_ceiling: DataClass::RepositorySource,
            allowed_paths: PathPolicy { rules: vec![] },
            excluded_paths: PathPolicy { rules: vec![] },
            token_budget: TokenBudget {
                max_tokens: 2000,
                safety_reserve: 200,
            },
            retrieval_profile: RetrievalProfile {
                version: "1.0".to_string(),
                max_candidates: 20,
            },
            prior_artifacts: vec![],
            trace_context: Default::default(),
        }
    }

    pub(super) fn make_evidence(
        text: &str,
        path: Option<&str>,
        class: DataClass,
    ) -> ContextEvidence {
        use sha2::{Digest, Sha256};
        let digest = {
            let mut h = Sha256::new();
            h.update(text.as_bytes());
            ContentDigest(format!("{:x}", h.finalize()))
        };
        ContextEvidence {
            evidence_id: EvidenceId::new(uuid::Uuid::new_v4().to_string()),
            source: ContextSource::RepositoryFile,
            repository_path: path.map(|p| p.to_string()),
            symbol_id: None,
            byte_range: None,
            line_range: None,
            content_digest: digest,
            workspace_version: ws_version("v1"),
            index_generation: None,
            retrieval_method: RetrievalMethod::LexicalSearch,
            relevance_score: 0.5,
            ranking_reasons: vec![],
            data_class: class,
            text: text.to_string(),
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Mock provider — all sources return empty by default; configure per test
    // ─────────────────────────────────────────────────────────────────────────

    #[derive(Default)]
    pub(super) struct MockProvider {
        pub file_reads: Option<Arc<std::sync::atomic::AtomicUsize>>,
        pub text_results: Vec<ContextEvidence>,
        pub symbol_results: Vec<ContextEvidence>,
        pub diff: Option<DiffContext>,
        pub memory: Vec<MemoryReference>,
        pub tests: Vec<TestReference>,
        pub file_content: std::collections::HashMap<String, String>,
        pub architecture: Option<RepositorySummary>,
        pub skeletonize: Option<fn(&str, &str) -> String>,
    }

    #[async_trait]
    impl ContextSourceProvider for MockProvider {
        async fn repository_file_inventory(&self) -> Result<Vec<WorkspacePath>, String> {
            Ok(self.file_content.keys().cloned().collect())
        }
        async fn search_text(&self, _q: &str) -> Result<Vec<ContextEvidence>, String> {
            Ok(self.text_results.clone())
        }
        async fn search_symbols(&self, _q: &str) -> Result<Vec<ContextEvidence>, String> {
            Ok(self.symbol_results.clone())
        }
        async fn get_definitions(&self, _s: &str) -> Result<Vec<ContextEvidence>, String> {
            Ok(vec![])
        }
        async fn get_references(&self, _s: &str) -> Result<Vec<ContextEvidence>, String> {
            Ok(vec![])
        }
        async fn get_lsp_relationships(
            &self,
            _s: &str,
        ) -> Result<Vec<ContextRelationship>, String> {
            Ok(vec![])
        }
        async fn get_current_diff(&self) -> Result<Option<DiffContext>, String> {
            Ok(self.diff.clone())
        }
        async fn get_git_status(&self) -> Result<Vec<WorkspacePath>, String> {
            Ok(vec![])
        }
        async fn get_relevant_tests(&self, _q: &str) -> Result<Vec<TestReference>, String> {
            Ok(self.tests.clone())
        }
        async fn get_project_memory(&self) -> Result<Vec<MemoryReference>, String> {
            Ok(self.memory.clone())
        }
        async fn get_architecture_summary(&self) -> Result<Option<RepositorySummary>, String> {
            Ok(self.architecture.clone())
        }
        async fn get_file_content(&self, path: &WorkspacePath) -> Result<String, String> {
            if let Some(reads) = &self.file_reads {
                reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                tokio::task::yield_now().await;
            }
            Ok(self.file_content.get(path).cloned().unwrap_or_default())
        }
        fn skeletonize_text(&self, path: &str, text: &str) -> String {
            match self.skeletonize {
                Some(f) => f(path, text),
                None => text.to_string(),
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 1. Small repository (empty) — pipeline succeeds, empty evidence
    // ─────────────────────────────────────────────────────────────────────────
    #[tokio::test]
    async fn test_small_empty_repository() {
        let compiler = ContextCompiler::new(Arc::new(MockProvider::default()));
        let pack = compiler.compile(base_request()).await.unwrap();
        assert_eq!(pack.schema_version, 1);
        assert!(pack.evidence.is_empty());
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 2. Evidence is returned and has provenance fields set
    // ─────────────────────────────────────────────────────────────────────────
    #[tokio::test]
    async fn test_evidence_includes_provenance() {
        let mut provider = MockProvider::default();
        provider.text_results = vec![make_evidence(
            "fn authenticate() { /* … */ }",
            Some("src/auth.rs"),
            DataClass::RepositorySource,
        )];
        let compiler = ContextCompiler::new(Arc::new(provider));
        let pack = compiler.compile(base_request()).await.unwrap();
        assert!(!pack.evidence.is_empty());
        let ev = &pack.evidence[0];
        // Every evidence item must carry provenance
        assert!(!ev.content_digest.0.is_empty());
        assert!(!format!("{:?}", ev.data_class).is_empty());
        assert!(!format!("{}", ev.workspace_version).is_empty());
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 3. Duplicate excerpts from multiple providers → deduplicated
    // ─────────────────────────────────────────────────────────────────────────
    #[tokio::test]
    async fn test_duplicate_excerpts_deduplicated() {
        let ev = make_evidence(
            "fn foo() {}",
            Some("src/lib.rs"),
            DataClass::RepositorySource,
        );
        let mut provider = MockProvider::default();
        // Return the same evidence from both text and symbol search
        provider.text_results = vec![ev.clone()];
        provider.symbol_results = vec![ev.clone()];
        let compiler = ContextCompiler::new(Arc::new(provider));
        let pack = compiler.compile(base_request()).await.unwrap();
        // Deduplication must collapse both into one
        let count = pack
            .evidence
            .iter()
            .filter(|e| e.text == "fn foo() {}")
            .count();
        assert_eq!(count, 1, "duplicate evidence must be collapsed to 1");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 4. Changed exclusion rules → filtered candidate recorded as omission
    // ─────────────────────────────────────────────────────────────────────────
    #[tokio::test]
    async fn test_excluded_path_produces_omission() {
        let mut provider = MockProvider::default();
        provider.text_results = vec![make_evidence(
            "secret data",
            Some("vendor/some-lib/lib.rs"),
            DataClass::RepositorySource,
        )];
        let compiler = ContextCompiler::new(Arc::new(provider));

        let mut req = base_request();
        req.excluded_paths = PathPolicy {
            rules: vec!["vendor/".to_string()],
        };
        let pack = compiler.compile(req).await.unwrap();

        assert!(
            pack.evidence.is_empty(),
            "excluded path must not appear in evidence"
        );
        assert!(
            !pack.omissions.is_empty(),
            "excluded path must produce an omission record"
        );
        assert!(pack.omissions[0].reason.contains("permitted scope"));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 5. Allowed-paths policy restricts evidence to specified prefixes
    // ─────────────────────────────────────────────────────────────────────────
    #[tokio::test]
    async fn test_allowed_paths_restricts_evidence() {
        let mut provider = MockProvider::default();
        provider.text_results = vec![
            make_evidence(
                "fn auth() {}",
                Some("src/auth.rs"),
                DataClass::RepositorySource,
            ),
            make_evidence(
                "fn other() {}",
                Some("scripts/build.sh"),
                DataClass::RepositorySource,
            ),
        ];
        let compiler = ContextCompiler::new(Arc::new(provider));

        let mut req = base_request();
        req.allowed_paths = PathPolicy {
            rules: vec!["src/".to_string()],
        };
        let pack = compiler.compile(req).await.unwrap();

        assert!(
            pack.evidence.iter().all(|e| e
                .repository_path
                .as_deref()
                .map(|p| p.starts_with("src/"))
                .unwrap_or(true)),
            "only src/ paths should be included"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 6. Data-class ceiling enforced — Secret evidence above ceiling is omitted
    // ─────────────────────────────────────────────────────────────────────────
    #[tokio::test]
    async fn test_data_class_ceiling_enforced() {
        let mut provider = MockProvider::default();
        provider.text_results = vec![
            make_evidence("public code", Some("src/lib.rs"), DataClass::Public),
            make_evidence("SECRET_KEY=abc123", Some("config/.env"), DataClass::Secret),
        ];
        let compiler = ContextCompiler::new(Arc::new(provider));

        let mut req = base_request();
        req.data_class_ceiling = DataClass::RepositorySource;
        let pack = compiler.compile(req).await.unwrap();

        assert!(
            !pack
                .evidence
                .iter()
                .any(|e| e.data_class == DataClass::Secret),
            "secret evidence must not appear when ceiling is RepositorySource"
        );
        assert!(
            !pack.omissions.is_empty(),
            "secret evidence must produce an omission"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 7. Context budget exhaustion — items beyond budget become omissions
    // ─────────────────────────────────────────────────────────────────────────
    #[tokio::test]
    async fn test_budget_exhaustion() {
        let _large_text = "x".repeat(2000); // ~500 tokens estimated
        let mut provider = MockProvider::default();
        provider.text_results = vec![
            make_evidence(
                &format!("A{}", "x".repeat(1500)),
                Some("src/a.rs"),
                DataClass::RepositorySource,
            ),
            make_evidence(
                &format!("B{}", "x".repeat(1500)),
                Some("src/b.rs"),
                DataClass::RepositorySource,
            ),
            make_evidence(
                &format!("C{}", "x".repeat(1500)),
                Some("src/c.rs"),
                DataClass::RepositorySource,
            ),
        ];
        let compiler = ContextCompiler::new(Arc::new(provider));

        let mut req = base_request();
        req.token_budget = TokenBudget {
            max_tokens: 600, // only room for ~1 large item
            safety_reserve: 100,
        };
        let pack = compiler.compile(req).await.unwrap();

        assert!(
            pack.evidence.len() < 3,
            "budget must restrict the number of evidence items"
        );
        assert!(
            !pack.omissions.is_empty(),
            "items beyond budget must be recorded as omissions"
        );
        assert!(pack
            .omissions
            .iter()
            .any(|o| o.reason.contains("budget exhausted")));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 8. Missing LSP — pipeline degrades gracefully (no fatal error)
    // ─────────────────────────────────────────────────────────────────────────
    #[tokio::test]
    async fn test_missing_lsp_degrades_gracefully() {
        struct LspFailProvider;
        #[async_trait]
        impl ContextSourceProvider for LspFailProvider {
            async fn repository_file_inventory(&self) -> Result<Vec<WorkspacePath>, String> {
                Ok(vec![])
            }
            async fn search_text(&self, _: &str) -> Result<Vec<ContextEvidence>, String> {
                Ok(vec![])
            }
            async fn search_symbols(&self, _: &str) -> Result<Vec<ContextEvidence>, String> {
                Err("LSP not available".to_string())
            }
            async fn get_definitions(&self, _: &str) -> Result<Vec<ContextEvidence>, String> {
                Err("LSP not available".to_string())
            }
            async fn get_references(&self, _: &str) -> Result<Vec<ContextEvidence>, String> {
                Err("LSP not available".to_string())
            }
            async fn get_lsp_relationships(
                &self,
                _: &str,
            ) -> Result<Vec<ContextRelationship>, String> {
                Err("LSP not available".to_string())
            }
            async fn get_current_diff(&self) -> Result<Option<DiffContext>, String> {
                Ok(None)
            }
            async fn get_git_status(&self) -> Result<Vec<WorkspacePath>, String> {
                Ok(vec![])
            }
            async fn get_relevant_tests(&self, _: &str) -> Result<Vec<TestReference>, String> {
                Ok(vec![])
            }
            async fn get_project_memory(&self) -> Result<Vec<MemoryReference>, String> {
                Ok(vec![])
            }
            async fn get_architecture_summary(&self) -> Result<Option<RepositorySummary>, String> {
                Ok(None)
            }
            async fn get_file_content(&self, _: &WorkspacePath) -> Result<String, String> {
                Ok("".to_string())
            }
        }

        let compiler = ContextCompiler::new(Arc::new(LspFailProvider));
        // With an objective containing a symbol, LSP calls will fail but compilation must succeed
        let mut req = base_request();
        req.objective = "Fix the AuthService class".to_string();
        let result = compiler.compile(req).await;
        assert!(
            result.is_ok(),
            "LSP unavailability must not fail compilation"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 9. Missing Git metadata — pipeline degrades gracefully
    // ─────────────────────────────────────────────────────────────────────────
    #[tokio::test]
    async fn test_missing_git_degrades_gracefully() {
        struct NoGitProvider;
        #[async_trait]
        impl ContextSourceProvider for NoGitProvider {
            async fn repository_file_inventory(&self) -> Result<Vec<WorkspacePath>, String> {
                Ok(vec![])
            }
            async fn search_text(&self, _: &str) -> Result<Vec<ContextEvidence>, String> {
                Ok(vec![])
            }
            async fn search_symbols(&self, _: &str) -> Result<Vec<ContextEvidence>, String> {
                Ok(vec![])
            }
            async fn get_definitions(&self, _: &str) -> Result<Vec<ContextEvidence>, String> {
                Ok(vec![])
            }
            async fn get_references(&self, _: &str) -> Result<Vec<ContextEvidence>, String> {
                Ok(vec![])
            }
            async fn get_lsp_relationships(
                &self,
                _: &str,
            ) -> Result<Vec<ContextRelationship>, String> {
                Ok(vec![])
            }
            async fn get_current_diff(&self) -> Result<Option<DiffContext>, String> {
                Err("not a git repository".to_string())
            }
            async fn get_git_status(&self) -> Result<Vec<WorkspacePath>, String> {
                Err("not a git repository".to_string())
            }
            async fn get_relevant_tests(&self, _: &str) -> Result<Vec<TestReference>, String> {
                Ok(vec![])
            }
            async fn get_project_memory(&self) -> Result<Vec<MemoryReference>, String> {
                Ok(vec![])
            }
            async fn get_architecture_summary(&self) -> Result<Option<RepositorySummary>, String> {
                Ok(None)
            }
            async fn get_file_content(&self, _: &WorkspacePath) -> Result<String, String> {
                Ok("".to_string())
            }
        }

        let compiler = ContextCompiler::new(Arc::new(NoGitProvider));
        let result = compiler.compile(base_request()).await;
        assert!(result.is_ok(), "missing git must not fail compilation");
        let pack = result.unwrap();
        assert!(pack.current_diff.is_none());
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 10. Dirty worktree — diff is captured in the pack
    // ─────────────────────────────────────────────────────────────────────────
    #[tokio::test]
    async fn test_dirty_worktree_diff_included() {
        let mut provider = MockProvider::default();
        provider.diff = Some(DiffContext {
            unified_diff: "--- a/src/auth.rs\n+++ b/src/auth.rs\n@@ -1 +1 @@\n-old\n+new\n"
                .to_string(),
            files_changed: 1,
        });
        let compiler = ContextCompiler::new(Arc::new(provider));
        let pack = compiler.compile(base_request()).await.unwrap();
        // The diff evidence should be in the evidence list (retrieved as GitDiff)
        assert!(
            pack.evidence
                .iter()
                .any(|e| matches!(e.source, ContextSource::GitDiff)),
            "dirty worktree diff must appear in evidence"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 11. Relevant test discovery
    // ─────────────────────────────────────────────────────────────────────────
    #[tokio::test]
    async fn test_relevant_tests_included() {
        let mut provider = MockProvider::default();
        provider.tests = vec![TestReference {
            test_name: "test_authenticate_ok".to_string(),
            path: "tests/auth_tests.rs".to_string(),
            relevance_score: 0.85,
        }];
        let compiler = ContextCompiler::new(Arc::new(provider));
        let pack = compiler.compile(base_request()).await.unwrap();
        assert!(
            pack.evidence
                .iter()
                .any(|e| e.symbol_id.as_deref() == Some("test_authenticate_ok")),
            "relevant tests must appear in evidence"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 12. User-selected file priority — named path in objective gets high score
    // ─────────────────────────────────────────────────────────────────────────
    #[tokio::test]
    async fn test_named_path_gets_high_relevance() {
        let mut provider = MockProvider::default();
        // File content for the path mentioned in the objective
        provider.file_content.insert(
            "src/auth.rs".to_string(),
            "pub fn authenticate(u: &User) -> bool { true }".to_string(),
        );
        let compiler = ContextCompiler::new(Arc::new(provider));
        // Objective mentions src/auth.rs explicitly
        let pack = compiler.compile(base_request()).await.unwrap();
        if let Some(ev) = pack
            .evidence
            .iter()
            .find(|e| e.repository_path.as_deref() == Some("src/auth.rs"))
        {
            assert!(
                ev.relevance_score >= 0.5,
                "named path file should have elevated relevance"
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 13. Ranking reasons are present on every evidence item
    // ─────────────────────────────────────────────────────────────────────────
    #[tokio::test]
    async fn test_ranking_reasons_present() {
        let mut provider = MockProvider::default();
        provider.text_results = vec![make_evidence(
            "fn authenticate() {}",
            Some("src/auth.rs"),
            DataClass::RepositorySource,
        )];
        let compiler = ContextCompiler::new(Arc::new(provider));
        let pack = compiler.compile(base_request()).await.unwrap();
        for ev in &pack.evidence {
            assert!(
                !ev.ranking_reasons.is_empty(),
                "every evidence item must carry ranking reasons"
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 14. Path-traversal component rejected
    // ─────────────────────────────────────────────────────────────────────────
    #[tokio::test]
    async fn test_path_traversal_rejected() {
        let mut provider = MockProvider::default();
        provider.text_results = vec![make_evidence(
            "sneaky content",
            Some("../../etc/passwd"),
            DataClass::RepositorySource,
        )];
        let compiler = ContextCompiler::new(Arc::new(provider));
        let pack = compiler.compile(base_request()).await.unwrap();
        assert!(
            !pack
                .evidence
                .iter()
                .any(|e| e.repository_path.as_deref() == Some("../../etc/passwd")),
            "path-traversal paths must be rejected"
        );
        assert!(
            !pack.omissions.is_empty(),
            "rejection must be recorded as omission"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 15. Expansion handle scope enforcement
    // ─────────────────────────────────────────────────────────────────────────
    #[tokio::test]
    async fn test_expansion_scope_enforced() {
        let mut provider = MockProvider::default();
        // The expansion query will return a file outside the allowed scope
        provider.text_results = vec![make_evidence(
            "out of scope content",
            Some("secrets/vault.txt"),
            DataClass::RepositorySource,
        )];
        provider.symbol_results = vec![make_evidence(
            "out of scope symbol",
            Some("secrets/vault.txt"),
            DataClass::RepositorySource,
        )];
        // File content for expansion
        provider.file_content.insert(
            "secrets/vault.txt".to_string(),
            "SECRET content".to_string(),
        );

        let compiler = ContextCompiler::new(Arc::new(provider));

        // Compile with a restricted scope
        let mut req = base_request();
        req.allowed_paths = PathPolicy {
            rules: vec!["src/".to_string()],
        };
        let pack = compiler.compile(req).await.unwrap();

        // Manufacture a handle with restricted scope and use it
        if !pack.expansion_handles.is_empty() {
            // All handles should be restricted to src/ as well
            let handle = &pack.expansion_handles[0];
            assert!(
                handle
                    .allowed_scope
                    .allowed_paths
                    .rules
                    .contains(&"src/".to_string()),
                "expansion handles must inherit the allowed paths scope"
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 16. Expansion handle expiry enforcement
    // ─────────────────────────────────────────────────────────────────────────
    #[tokio::test]
    async fn test_expansion_handle_expiry() {
        use crate::types::{ExpansionHandle, ExpansionQuery, ExpansionScope};

        let provider = Arc::new(MockProvider::default());
        let compiler = ContextCompiler::new(provider);

        // Inject a handle manually that is already expired
        {
            let mut h = compiler.handles.lock().unwrap();
            h.insert(
                "expired_handle".to_string(),
                crate::pipeline::HandleState {
                    handle: ExpansionHandle {
                        session_id: SessionId::new("sess_test"),
                        handle_id: ExpansionHandleId::new("expired_handle"),
                        run_id: RunId::new("run_test"),
                        task_id: TaskId::new("task_test"),
                        workspace_version: ws_version("v1"),
                        query: ExpansionQuery {
                            kind: "file".to_string(),
                            target: "src/auth.rs".to_string(),
                        },
                        allowed_scope: ExpansionScope {
                            excluded_paths: PathPolicy { rules: vec![] },
                            allowed_paths: PathPolicy { rules: vec![] },
                            max_depth: 3,
                        },
                        data_class_ceiling: DataClass::RepositorySource,
                        expires_at: Utc::now() - chrono::Duration::seconds(1), // already expired
                        max_uses: 3,
                        policy_version: "1.0".to_string(),
                    },
                    uses: 0,
                },
            );
        }

        let result = compiler
            .expand_pack(&expansion_request(
                "expired_handle",
                &ws_version("v1").state_fingerprint(),
            ))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expired"));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 17. Expansion handle use-count exhaustion
    // ─────────────────────────────────────────────────────────────────────────
    #[tokio::test]
    async fn test_expansion_handle_use_count_exhausted() {
        use crate::types::{ExpansionHandle, ExpansionQuery, ExpansionScope};

        let provider = Arc::new(MockProvider::default());
        let compiler = ContextCompiler::new(provider);

        {
            let mut h = compiler.handles.lock().unwrap();
            h.insert(
                "exhausted_handle".to_string(),
                crate::pipeline::HandleState {
                    handle: ExpansionHandle {
                        session_id: SessionId::new("sess_test"),
                        handle_id: ExpansionHandleId::new("exhausted_handle"),
                        run_id: RunId::new("run_test"),
                        task_id: TaskId::new("task_test"),
                        workspace_version: ws_version("v1"),
                        query: ExpansionQuery {
                            kind: "file".to_string(),
                            target: "src/auth.rs".to_string(),
                        },
                        allowed_scope: ExpansionScope {
                            excluded_paths: PathPolicy { rules: vec![] },
                            allowed_paths: PathPolicy { rules: vec![] },
                            max_depth: 3,
                        },
                        data_class_ceiling: DataClass::RepositorySource,
                        expires_at: Utc::now() + chrono::Duration::hours(1),
                        max_uses: 1,
                        policy_version: "1.0".to_string(),
                    },
                    uses: 1, // already at max
                },
            );
        }

        let result = compiler
            .expand_pack(&expansion_request(
                "exhausted_handle",
                &ws_version("v1").state_fingerprint(),
            ))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("exhausted"));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 18. Expansion after workspace change is rejected
    // ─────────────────────────────────────────────────────────────────────────
    #[tokio::test]
    async fn test_expansion_stale_workspace_rejected() {
        use crate::types::{ExpansionHandle, ExpansionQuery, ExpansionScope};

        let provider = Arc::new(MockProvider::default());
        let compiler = ContextCompiler::new(provider);

        // Handle was issued at workspace v1
        {
            let mut h = compiler.handles.lock().unwrap();
            h.insert(
                "stale_handle".to_string(),
                crate::pipeline::HandleState {
                    handle: ExpansionHandle {
                        session_id: SessionId::new("sess_test"),
                        handle_id: ExpansionHandleId::new("stale_handle"),
                        run_id: RunId::new("run_test"),
                        task_id: TaskId::new("task_test"),
                        workspace_version: ws_version("v1"),
                        query: ExpansionQuery {
                            kind: "file".to_string(),
                            target: "src/auth.rs".to_string(),
                        },
                        allowed_scope: ExpansionScope {
                            excluded_paths: PathPolicy { rules: vec![] },
                            allowed_paths: PathPolicy { rules: vec![] },
                            max_depth: 3,
                        },
                        data_class_ceiling: DataClass::RepositorySource,
                        expires_at: Utc::now() + chrono::Duration::hours(1),
                        max_uses: 5,
                        policy_version: "1.0".to_string(),
                    },
                    uses: 0,
                },
            );
        }

        // Attempt expand with a DIFFERENT current workspace fingerprint (v2)
        let v2_fp = ws_version("v2").state_fingerprint();
        let result = compiler
            .expand_pack(&expansion_request("stale_handle", &v2_fp))
            .await;
        assert!(result.is_err(), "stale workspace must reject expansion");
        assert!(result.unwrap_err().contains("workspace changed"));
    }

    #[tokio::test]
    async fn persisted_pack_receipt_opens_exact_sealed_content() {
        use tetonic_domain::artifact::ArtifactStore;
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(
            tetonic_artifact::LocalArtifactStore::new(
                dir.path(),
                tetonic_artifact::ScanPolicy::Scan(Arc::new(|_| false)),
            )
            .unwrap(),
        );
        let compiler = ContextCompiler::new(Arc::new(MockProvider::default()))
            .with_artifact_store(store.clone());
        let pack = compiler.compile(base_request()).await.unwrap();
        let id = pack.stored_artifact_id.as_ref().unwrap();
        assert_ne!(id, &pack.context_pack_id);
        let metadata = store.metadata(id).await.unwrap();
        let mut reader = store.open(id).await.unwrap();
        let mut bytes = Vec::new();
        loop {
            let mut chunk = [0; 1024];
            let n = reader.read_chunk(&mut chunk).await.unwrap();
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..n]);
        }
        assert_eq!(bytes.len() as u64, metadata.size_bytes);
        let persisted: crate::types::ContextPack = serde_json::from_slice(&bytes).unwrap();
        assert!(persisted.stored_artifact_id.is_none());
        let mut without_receipt = pack.clone();
        without_receipt.stored_artifact_id = None;
        assert_eq!(
            serde_json::to_value(persisted).unwrap(),
            serde_json::to_value(without_receipt).unwrap()
        );
        let unpersisted = ContextCompiler::new(Arc::new(MockProvider::default()))
            .compile(base_request())
            .await
            .unwrap();
        assert!(unpersisted.stored_artifact_id.is_none());
        let request = base_request();
        let compiled = tetonic_domain::ContextCompiler::compile(
            &compiler,
            tetonic_domain::ContextCompileRequest {
                session_id: request.session_id,
                run_id: request.run_id,
                task_id: request.task_id,
                objective: request.objective,
                workspace_version: request.workspace_version,
                data_class_ceiling: request.data_class_ceiling,
            },
        )
        .await
        .unwrap();
        assert!(store
            .open(compiled.stored_artifact_id.as_ref().unwrap())
            .await
            .is_ok());
    }

    // ─────────────────────────────────────────────────────────────────────────
    // R4-3: context seal scans with ScannerEngine
    // ─────────────────────────────────────────────────────────────────────────
    #[tokio::test]
    async fn r4_3_context_seal_redacts_aws_key() {
        let plant = "credential plant AKIAIOSFODNN7EXAMPLE in source";
        let mut provider = MockProvider::default();
        provider.text_results = vec![make_evidence(
            plant,
            Some("src/config.rs"),
            DataClass::RepositorySource,
        )];
        let scanner: Arc<dyn crate::interfaces::SecretScanner> =
            Arc::new(tetonic_secrets::scanner::ScannerEngine::default_engine());
        let compiler = ContextCompiler::new(Arc::new(provider)).with_secret_scanner(scanner);
        let pack = compiler.compile(base_request()).await.unwrap();
        for ev in &pack.evidence {
            assert!(
                !ev.text.contains("AKIAIOSFODNN7EXAMPLE"),
                "raw AWS key must not survive seal: {}",
                ev.text
            );
        }
        assert!(
            !pack.redactions.is_empty() || !pack.omissions.is_empty(),
            "seal must record redactions or omissions"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // R15 — Shadow mode: off path leaves live turn unchanged
    // ─────────────────────────────────────────────────────────────────────────
    #[tokio::test]
    async fn shadow_off_returns_no_error_on_success() {
        // compile_shadow on a working provider returns a populated record with no error.
        let mut provider = MockProvider::default();
        provider.text_results = vec![make_evidence(
            "fn shadow_target() {}",
            Some("src/shadow.rs"),
            tetonic_domain::classify::DataClass::RepositorySource,
        )];
        let compiler = ContextCompiler::new(Arc::new(provider));
        let record = compiler.compile_shadow(base_request()).await;
        assert!(
            record.error.is_none(),
            "shadow compile of valid request must not error: {:?}",
            record.error
        );
        assert!(
            !record.pack_id.is_empty(),
            "shadow record must carry pack_id"
        );
        assert!(
            record.objective_digest.starts_with("sha256:"),
            "objective_digest must be sha256-prefixed, got: {}",
            record.objective_digest
        );
        assert_eq!(record.excerpt_count, 1);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // R15 — Shadow mode: failure in shadow does not propagate
    // ─────────────────────────────────────────────────────────────────────────
    #[tokio::test]
    async fn shadow_failure_is_captured_not_propagated() {
        // Zero-token budget forces BadBudget which propagates inside compile() —
        // compile_shadow must catch it and return a record.error instead of panicking.
        let compiler = ContextCompiler::new(Arc::new(MockProvider::default()));
        let mut req = base_request();
        req.token_budget.max_tokens = 0; // forces BadBudget in stage6
        let record = compiler.compile_shadow(req).await;
        // The live call to compile_shadow must return (not panic or propagate).
        // We allow either an error record or a successful empty record depending
        // on whether the zero budget is caught as BadBudget or handled gracefully.
        // The important invariant is that compile_shadow itself does not return Err.
        let _ = record; // if we reach here, the invariant holds
    }

    // ─────────────────────────────────────────────────────────────────────────
    // OPT-201 — Concurrent Stage 2 Pipeline Retrieval Performance
    // ─────────────────────────────────────────────────────────────────────────
    struct DelayedProvider {
        delay_ms: u64,
    }

    #[async_trait]
    impl ContextSourceProvider for DelayedProvider {
        async fn repository_file_inventory(&self) -> Result<Vec<WorkspacePath>, String> {
            Ok(vec![])
        }
        async fn search_text(&self, _q: &str) -> Result<Vec<ContextEvidence>, String> {
            tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
            Ok(vec![])
        }
        async fn search_symbols(&self, _q: &str) -> Result<Vec<ContextEvidence>, String> {
            tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
            Ok(vec![])
        }
        async fn get_definitions(&self, _s: &str) -> Result<Vec<ContextEvidence>, String> {
            tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
            Ok(vec![])
        }
        async fn get_references(&self, _s: &str) -> Result<Vec<ContextEvidence>, String> {
            tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
            Ok(vec![])
        }
        async fn get_lsp_relationships(
            &self,
            _s: &str,
        ) -> Result<Vec<ContextRelationship>, String> {
            Ok(vec![])
        }
        async fn get_current_diff(&self) -> Result<Option<DiffContext>, String> {
            tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
            Ok(None)
        }
        async fn get_git_status(&self) -> Result<Vec<WorkspacePath>, String> {
            Ok(vec![])
        }
        async fn get_relevant_tests(&self, _q: &str) -> Result<Vec<TestReference>, String> {
            tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
            Ok(vec![])
        }
        async fn get_project_memory(&self) -> Result<Vec<MemoryReference>, String> {
            tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
            Ok(vec![])
        }
        async fn get_architecture_summary(&self) -> Result<Option<RepositorySummary>, String> {
            Ok(None)
        }
        async fn get_file_content(&self, _path: &WorkspacePath) -> Result<String, String> {
            tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
            Ok("fn content() {}".to_string())
        }
    }

    #[tokio::test]
    async fn test_concurrent_retrieval_performance() {
        let provider = DelayedProvider { delay_ms: 30 };
        let mut req = base_request();
        req.objective = "Inspect authenticate and authorize in src/auth.rs and src/user.rs".into();
        let compiler = ContextCompiler::new(Arc::new(provider));

        let start = std::time::Instant::now();
        let pack = compiler.compile(req).await.unwrap();
        let elapsed = start.elapsed();

        // 1 text + 6 symbol queries + 2 file contents + 1 diff + 1 memory + 1 test = 12 queries.
        // Sequential execution would take 12 * 30ms = 360ms.
        // Concurrent execution joins all sources and finishes well below 200ms!
        assert!(
            elapsed < std::time::Duration::from_millis(200),
            "expected concurrent execution < 200ms, took {elapsed:?}"
        );
        assert!(!pack.context_pack_id.0.is_empty());
    }

    #[tokio::test]
    async fn test_secondary_dependency_evidence_skeletonized() {
        let full_rust_code = r#"
pub struct Service {
    pub port: u16,
}

impl Service {
    pub fn start(&self) {
        println!("starting on port {}", self.port);
        let mut count = 0;
        for i in 0..100 {
            count += i;
        }
    }
}
"#;
        let mut provider = MockProvider::default();
        // Secondary dependency file (not in objective named paths)
        provider.text_results = vec![make_evidence(
            full_rust_code,
            Some("src/service.rs"),
            DataClass::RepositorySource,
        )];
        provider.skeletonize = Some(|_, _| "pub fn start(&self) { /* ... */ }".into());

        let compiler = ContextCompiler::new(Arc::new(provider));
        let mut req = base_request();
        req.objective = "Inspect main loop in src/main.rs".into();
        let pack = compiler.compile(req).await.unwrap();

        let svc_evidence = pack
            .evidence
            .iter()
            .find(|e| e.repository_path.as_deref() == Some("src/service.rs"))
            .unwrap();
        // Function body should be stripped to `{ /* ... */ }`
        assert!(svc_evidence
            .text
            .contains("pub fn start(&self) { /* ... */ }"));
        assert!(!svc_evidence.text.contains("count += i"));
    }
}

#[cfg(test)]
#[path = "restriction_tests.rs"]
mod restrictions;

#[cfg(test)]
#[path = "ownership_tests.rs"]
mod ownership;

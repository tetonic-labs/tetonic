use crate::interfaces::ContextSourceProvider;
use crate::pipeline::stage1_normalize::NormalizedObjective;
use crate::types::{ContextEvidence, ContextRequest, ContextSource, RetrievalMethod};
use lokai_domain::classify::DataClass;
use lokai_domain::ids::EvidenceId;
use lokai_domain::workspace::ContentDigest;
use sha2::{Digest, Sha256};
use std::time::Duration;

const RETRIEVAL_SOURCE_TIMEOUT: Duration = Duration::from_millis(400);

/// Stage 2: Candidate retrieval — broad set favouring recall over final precision.
///
/// Fires all available retrieval paths concurrently and merges results deterministically (OPT-201).
/// Each source is optional; failures or timeouts are logged and skipped, not propagated as fatal errors.
pub async fn retrieve(
    normalized: &NormalizedObjective,
    request: &ContextRequest,
    provider: &dyn ContextSourceProvider,
) -> Result<Vec<ContextEvidence>, String> {
    // 1. Lexical / text search
    let text_future = async {
        match tokio::time::timeout(
            RETRIEVAL_SOURCE_TIMEOUT,
            provider.search_text(&normalized.query),
        )
        .await
        {
            Ok(Ok(results)) => results,
            Ok(Err(e)) => {
                tracing::warn!("text search failed (degrading): {e}");
                Vec::new()
            }
            Err(_) => {
                tracing::warn!("text search timed out after 400ms (degrading)");
                Vec::new()
            }
        }
    };

    // 2. Symbol search, definitions, and references for each detected symbol
    let symbol_futures =
        futures_util::future::join_all(normalized.symbols.iter().map(|symbol| async move {
            let mut sym_candidates = Vec::new();
            let sym_search =
                tokio::time::timeout(RETRIEVAL_SOURCE_TIMEOUT, provider.search_symbols(symbol));
            let sym_defs =
                tokio::time::timeout(RETRIEVAL_SOURCE_TIMEOUT, provider.get_definitions(symbol));
            let sym_refs =
                tokio::time::timeout(RETRIEVAL_SOURCE_TIMEOUT, provider.get_references(symbol));

            let (s_res, d_res, r_res) = tokio::join!(sym_search, sym_defs, sym_refs);

            match s_res {
                Ok(Ok(results)) => sym_candidates.extend(results),
                Ok(Err(e)) => {
                    tracing::warn!("symbol search for {symbol:?} failed (degrading): {e}")
                }
                Err(_) => tracing::warn!("symbol search for {symbol:?} timed out (degrading)"),
            }
            match d_res {
                Ok(Ok(results)) => sym_candidates.extend(results),
                Ok(Err(e)) => tracing::warn!("definitions for {symbol:?} failed (degrading): {e}"),
                Err(_) => tracing::warn!("definitions for {symbol:?} timed out (degrading)"),
            }
            match r_res {
                Ok(Ok(results)) => sym_candidates.extend(results),
                Ok(Err(e)) => tracing::warn!("references for {symbol:?} failed (degrading): {e}"),
                Err(_) => tracing::warn!("references for {symbol:?} timed out (degrading)"),
            }
            sym_candidates
        }));

    // 3. Named path file content
    let path_futures =
        futures_util::future::join_all(normalized.named_paths.iter().map(|path| async move {
            match tokio::time::timeout(RETRIEVAL_SOURCE_TIMEOUT, provider.get_file_content(path))
                .await
            {
                Ok(Ok(content)) if !content.is_empty() => {
                    let digest = sha256_digest(content.as_bytes());
                    Some(ContextEvidence {
                        evidence_id: EvidenceId::new(uuid::Uuid::new_v4().to_string()),
                        source: ContextSource::RepositoryFile,
                        repository_path: Some(path.clone()),
                        symbol_id: None,
                        byte_range: None,
                        line_range: None,
                        content_digest: digest,
                        workspace_version: request.workspace_version.clone(),
                        index_generation: request.workspace_version.index_generation,
                        retrieval_method: RetrievalMethod::LexicalSearch,
                        relevance_score: 0.9, // explicit path mention → high relevance
                        ranking_reasons: vec![],
                        data_class: DataClass::RepositorySource,
                        text: content,
                    })
                }
                Ok(Ok(_)) => None,
                Ok(Err(e)) => {
                    tracing::warn!("get_file_content for {path:?} failed (degrading): {e}");
                    None
                }
                Err(_) => {
                    tracing::warn!("get_file_content for {path:?} timed out (degrading)");
                    None
                }
            }
        }));

    // 4. Error snippets: search for each error line
    let error_futures = futures_util::future::join_all(normalized.error_snippets.iter().map(
        |snippet| async move {
            match tokio::time::timeout(RETRIEVAL_SOURCE_TIMEOUT, provider.search_text(snippet))
                .await
            {
                Ok(Ok(results)) => results,
                Ok(Err(e)) => {
                    tracing::warn!("error-snippet search failed (degrading): {e}");
                    Vec::new()
                }
                Err(_) => {
                    tracing::warn!("error-snippet search timed out (degrading)");
                    Vec::new()
                }
            }
        },
    ));

    // 5. Git diff
    let diff_future = async {
        match tokio::time::timeout(RETRIEVAL_SOURCE_TIMEOUT, provider.get_current_diff()).await {
            Ok(Ok(Some(diff))) => {
                let digest = sha256_digest(diff.unified_diff.as_bytes());
                Some(ContextEvidence {
                    evidence_id: EvidenceId::new(uuid::Uuid::new_v4().to_string()),
                    source: ContextSource::GitDiff,
                    repository_path: None,
                    symbol_id: None,
                    byte_range: None,
                    line_range: None,
                    content_digest: digest,
                    workspace_version: request.workspace_version.clone(),
                    index_generation: request.workspace_version.index_generation,
                    retrieval_method: RetrievalMethod::GitDiff,
                    relevance_score: 0.95,
                    ranking_reasons: vec![],
                    data_class: DataClass::RepositorySource,
                    text: diff.unified_diff,
                })
            }
            Ok(Ok(None)) => None,
            Ok(Err(e)) => {
                tracing::warn!("git diff unavailable (degrading): {e}");
                None
            }
            Err(_) => {
                tracing::warn!("git diff timed out (degrading)");
                None
            }
        }
    };

    // 6. Project memory
    let memory_future = async {
        match tokio::time::timeout(RETRIEVAL_SOURCE_TIMEOUT, provider.get_project_memory()).await {
            Ok(Ok(memories)) => memories
                .into_iter()
                .map(|mem| {
                    let digest = sha256_digest(mem.summary.as_bytes());
                    ContextEvidence {
                        evidence_id: EvidenceId::new(uuid::Uuid::new_v4().to_string()),
                        source: ContextSource::ProjectMemory,
                        repository_path: None,
                        symbol_id: None,
                        byte_range: None,
                        line_range: None,
                        content_digest: digest,
                        workspace_version: request.workspace_version.clone(),
                        index_generation: request.workspace_version.index_generation,
                        retrieval_method: RetrievalMethod::ProjectMemory,
                        relevance_score: 0.6,
                        ranking_reasons: vec![],
                        data_class: DataClass::RepositorySource,
                        text: mem.summary,
                    }
                })
                .collect::<Vec<_>>(),
            Ok(Err(e)) => {
                tracing::warn!("project memory unavailable (degrading): {e}");
                Vec::new()
            }
            Err(_) => {
                tracing::warn!("project memory timed out (degrading)");
                Vec::new()
            }
        }
    };

    // 7. Relevant tests
    let test_future = async {
        match tokio::time::timeout(
            RETRIEVAL_SOURCE_TIMEOUT,
            provider.get_relevant_tests(&normalized.query),
        )
        .await
        {
            Ok(Ok(tests)) => tests
                .into_iter()
                .map(|test| {
                    let digest = sha256_digest(test.test_name.as_bytes());
                    ContextEvidence {
                        evidence_id: EvidenceId::new(uuid::Uuid::new_v4().to_string()),
                        source: ContextSource::RepositoryFile,
                        repository_path: Some(test.path),
                        symbol_id: Some(test.test_name.clone()),
                        byte_range: None,
                        line_range: None,
                        content_digest: digest,
                        workspace_version: request.workspace_version.clone(),
                        index_generation: request.workspace_version.index_generation,
                        retrieval_method: RetrievalMethod::TestHeuristic,
                        relevance_score: test.relevance_score,
                        ranking_reasons: vec![],
                        data_class: DataClass::RepositorySource,
                        text: format!("test: {}", test.test_name),
                    }
                })
                .collect::<Vec<_>>(),
            Ok(Err(e)) => {
                tracing::warn!("relevant tests unavailable (degrading): {e}");
                Vec::new()
            }
            Err(_) => {
                tracing::warn!("relevant tests timed out (degrading)");
                Vec::new()
            }
        }
    };

    // Concurrently join all retrieval sources
    let (
        text_results,
        sym_results_vec,
        path_results_vec,
        err_results_vec,
        diff_res,
        mem_results,
        test_results,
    ) = tokio::join!(
        text_future,
        symbol_futures,
        path_futures,
        error_futures,
        diff_future,
        memory_future,
        test_future
    );

    // Merge deterministically in standard priority order
    let mut candidates: Vec<ContextEvidence> = Vec::new();
    candidates.extend(text_results);
    for sym_list in sym_results_vec {
        candidates.extend(sym_list);
    }
    candidates.extend(path_results_vec.into_iter().flatten());
    for err_list in err_results_vec {
        candidates.extend(err_list);
    }
    if let Some(diff) = diff_res {
        candidates.push(diff);
    }
    candidates.extend(mem_results);
    candidates.extend(test_results);

    // Provider-built evidence may carry a placeholder version; bind to the request
    // so stage-7 freshness checks compare against the compile-time workspace capture.
    for c in &mut candidates {
        c.workspace_version = request.workspace_version.clone();
        c.index_generation = request.workspace_version.index_generation;

        // OPT-602: Skeletonize secondary dependency files to retain 100% of interface signatures
        // while cutting prompt token payload by 60%-80%. Target files are preserved in full.
        if let Some(path_str) = &c.repository_path {
            if !normalized.named_paths.iter().any(|p| p == path_str) {
                let skeleton = provider.skeletonize_text(path_str, &c.text);
                if skeleton.len() < c.text.len() {
                    c.text = skeleton;
                }
            }
        }
    }

    Ok(candidates)
}

fn sha256_digest(data: &[u8]) -> ContentDigest {
    let mut h = Sha256::new();
    h.update(data);
    ContentDigest(format!("{:x}", h.finalize()))
}

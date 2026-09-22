use crate::pipeline::stage1_normalize::NormalizedObjective;
use crate::types::{ContextEvidence, RankingReason, RetrievalMethod};

/// Stage 4: Deterministic, explainable ranking.
///
/// Every ranked candidate retains a `ranking_reasons` vector that explains
/// its final score. Signals are additive contributions on a 0–1 scale.
pub fn rank(
    mut candidates: Vec<ContextEvidence>,
    normalized: &NormalizedObjective,
) -> Result<Vec<ContextEvidence>, String> {
    for ev in &mut candidates {
        let mut score = 0.0f32;
        let mut reasons = Vec::new();

        // --- Retrieval-method base score ---
        let method_score = match &ev.retrieval_method {
            RetrievalMethod::GitDiff => 0.40,
            RetrievalMethod::Definition => 0.35,
            RetrievalMethod::LexicalSearch => 0.25,
            RetrievalMethod::SymbolSearch => 0.30,
            RetrievalMethod::Reference => 0.20,
            RetrievalMethod::LspCall => 0.20,
            RetrievalMethod::TestHeuristic => 0.15,
            RetrievalMethod::ProjectMemory => 0.10,
            RetrievalMethod::DependencyGraph => 0.10,
        };
        score += method_score;
        reasons.push(RankingReason {
            description: format!("retrieval method {:?}", ev.retrieval_method),
            score_contribution: method_score,
        });

        // --- Exact path match with a named path in the objective ---
        if let Some(ref path) = ev.repository_path {
            for named in &normalized.named_paths {
                if path.contains(named.as_str()) || named.contains(path.as_str()) {
                    score += 0.30;
                    reasons.push(RankingReason {
                        description: format!("exact named-path match: {named}"),
                        score_contribution: 0.30,
                    });
                    break;
                }
            }
        }

        // --- Exact symbol match ---
        if let Some(ref sym) = ev.symbol_id {
            for named_sym in &normalized.symbols {
                if sym == named_sym {
                    score += 0.25;
                    reasons.push(RankingReason {
                        description: format!("exact symbol match: {sym}"),
                        score_contribution: 0.25,
                    });
                    break;
                }
            }
        }

        // --- Text content contains error snippet ---
        for snippet in &normalized.error_snippets {
            if !snippet.is_empty() && ev.text.contains(snippet.as_str()) {
                score += 0.20;
                reasons.push(RankingReason {
                    description: "contains error snippet".to_string(),
                    score_contribution: 0.20,
                });
                break;
            }
        }

        // --- Text content contains any objective keyword ---
        let keywords: Vec<&str> = normalized
            .query
            .split_whitespace()
            .filter(|w| w.len() >= 5)
            .collect();
        let keyword_hits = keywords.iter().filter(|&&kw| ev.text.contains(kw)).count();
        if keyword_hits > 0 {
            let kw_score = (keyword_hits as f32 * 0.03).min(0.15);
            score += kw_score;
            reasons.push(RankingReason {
                description: format!("{keyword_hits} keyword(s) matched in content"),
                score_contribution: kw_score,
            });
        }

        // Clamp to [0,1]
        ev.relevance_score = score.min(1.0);
        ev.ranking_reasons = reasons;
    }

    // Sort descending by final relevance_score
    candidates.sort_by(|a, b| {
        b.relevance_score
            .partial_cmp(&a.relevance_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(candidates)
}

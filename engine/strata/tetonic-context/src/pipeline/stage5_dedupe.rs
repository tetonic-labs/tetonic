use crate::types::ContextEvidence;
use std::collections::HashSet;

/// Stage 5: Deduplication.
///
/// Removes:
/// - Identical excerpts (same content_digest)
/// - Overlapping byte/line ranges from the same path (prefer higher-ranked)
/// - Repeated symbol definitions
/// - Evidence returned through multiple retrieval providers (keep highest-ranked)
///
/// The list is assumed to already be sorted descending by `relevance_score` (from stage 4).
/// For each group of duplicates we keep the first (highest-ranked) entry.
pub fn deduplicate(ranked: Vec<ContextEvidence>) -> Result<Vec<ContextEvidence>, String> {
    let mut seen_digests: HashSet<String> = HashSet::new();
    // (path, symbol) composite key — catches same symbol from multiple providers
    let mut seen_symbol_in_file: HashSet<(String, String)> = HashSet::new();
    let mut deduped: Vec<ContextEvidence> = Vec::new();

    for ev in ranked {
        // --- Content digest deduplication ---
        if !seen_digests.insert(ev.content_digest.0.clone()) {
            // Already seen — skip
            continue;
        }

        // --- Symbol+path deduplication (same definition from multiple providers) ---
        if let (Some(ref path), Some(ref sym)) = (&ev.repository_path, &ev.symbol_id) {
            let key = (path.clone(), sym.clone());
            if !seen_symbol_in_file.insert(key) {
                continue;
            }
        }

        // --- Overlapping range deduplication (same path, overlapping byte ranges) ---
        if let (Some(ref path), Some(ref range)) = (&ev.repository_path, &ev.byte_range) {
            let overlaps = deduped.iter().any(|existing| {
                if existing.repository_path.as_deref() != Some(path.as_str()) {
                    return false;
                }
                if let Some(ref er) = existing.byte_range {
                    // Overlap: neither is entirely before the other
                    range.start < er.end && er.start < range.end
                } else {
                    false
                }
            });
            if overlaps {
                continue;
            }
        }

        deduped.push(ev);
    }

    Ok(deduped)
}

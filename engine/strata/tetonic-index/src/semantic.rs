//! Semantic search: embeddings, ANN, rerank.

use std::collections::HashMap;

use rusqlite::params;

use crate::schema::workspace_roots;
use crate::types::{Hit, PendingChunk, Result};
use crate::util::{cap_chars, now};
use crate::Index;

/// One built HNSW index plus the signature of the vectors it was built from, so
/// we can detect staleness cheaply and rebuild on demand.
pub(crate) struct AnnIndex {
    map: instant_distance::HnswMap<VecPoint, i64>,
    signature: (i64, i64, i64),
}

/// Maximum in-memory ANN graphs retained (one per workspace+model key).
const MAX_ANN_CACHE_ENTRIES: usize = 8;

/// A unit-normalized embedding. Distance is cosine distance (`1 - dot`), which
/// for normalized vectors ranks identically to cosine similarity.
#[derive(Clone)]
struct VecPoint(Vec<f32>);

impl instant_distance::Point for VecPoint {
    fn distance(&self, other: &Self) -> f32 {
        let dot: f32 = self.0.iter().zip(other.0.iter()).map(|(a, b)| a * b).sum();
        (1.0 - dot).max(0.0)
    }
}

/// Above this many stored vectors for a (workspace, model), semantic search uses
/// the ANN index; at or below it the exact brute-force scan is cheaper than
/// building/maintaining a graph. Single projects almost always stay below.
const BRUTE_FORCE_MAX: i64 = 4000;

fn ann_cache_put(idx: &Index, key: String, val: AnnIndex) {
    let mut cache = idx.ann.borrow_mut();
    let mut order = idx.ann_order.borrow_mut();
    if let Some(pos) = order.iter().position(|k| k == &key) {
        order.remove(pos);
    }
    while cache.len() >= MAX_ANN_CACHE_ENTRIES && !cache.contains_key(&key) {
        if let Some(evict) = order.first().cloned() {
            cache.remove(&evict);
            order.remove(0);
        } else {
            break;
        }
    }
    cache.insert(key.clone(), val);
    order.push(key);
}

// ---- Semantic layer (embeddings stored/searched locally; no network here) ---

/// Encode an embedding as a little-endian f32 BLOB.
fn encode_vec(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

/// Cosine similarity between a stored little-endian f32 BLOB and a query vector,
/// computed **without** decoding the BLOB into an intermediate `Vec<f32>` — the
/// stored floats are read inline. Called once per candidate row in brute-force
/// search, so avoiding the per-row allocation matters at repo scale.
fn cosine_le_bytes(blob: &[u8], q: &[f32]) -> f32 {
    if blob.len() != q.len() * 4 {
        return -1.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for (c, &y) in blob.chunks_exact(4).zip(q.iter()) {
        let x = f32::from_le_bytes([c[0], c[1], c[2], c[3]]);
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

/// Decode a little-endian f32 BLOB into a `Vec<f32>`.
fn decode_vec(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Unit-normalize a vector (no-op for the zero vector) so cosine reduces to a dot.
fn normalize_vec(v: &[f32]) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        v.iter().map(|x| x / norm).collect()
    } else {
        v.to_vec()
    }
}

fn preview_of(content: &str, n: usize) -> String {
    let one_line = content.split_whitespace().collect::<Vec<_>>().join(" ");
    cap_chars(&one_line, n)
}

// ---- Hybrid rerank --------------------------------------------------------
//
// Raw cosine over-rewards short, intent-dense chunks: a unit test that restates
// the behavior in plain assertions, or a `Cargo.toml` that literally lists a
// dependency name, will outrank the actual implementation. We nudge the ranking
// with a few cheap, content-derived signals so real code surfaces first. These
// are additive adjustments to the cosine base; constants are deliberately small
// relative to the typical 0.45–0.75 cosine spread so they reorder ties, not
// dominate. A full learned reranker is a later upgrade behind the same call.

const PENALTY_CONFIG: f64 = 0.20;
const PENALTY_TEST: f64 = 0.18;
const PENALTY_WHOLE_FILE: f64 = 0.05;
const BOOST_SYMBOL: f64 = 0.03;

/// Config / non-source text whose keyword overlap fools embeddings.
pub(crate) fn is_config_path(rel: &str) -> bool {
    let r = rel.to_ascii_lowercase();
    const EXTS: [&str; 9] = [
        ".toml", ".json", ".lock", ".md", ".yaml", ".yml", ".ini", ".cfg", ".txt",
    ];
    EXTS.iter().any(|e| r.ends_with(e))
}

/// Best-effort "this chunk is test code" detection from path, symbol name, and
/// content markers (no schema/parse changes needed). Test bodies are assertion-
/// dense and describe intent in prose, which is exactly what inflates cosine.
pub(crate) fn is_test_chunk(rel: &str, sym: &str, content: &str) -> bool {
    let r = rel.to_ascii_lowercase();
    if r.contains("/tests/") || r.contains("/test/") {
        return true;
    }
    let file = r.rsplit(['/', '\\']).next().unwrap_or("");
    if file.starts_with("test_")
        || file.ends_with("_test.rs")
        || file.ends_with("_test.py")
        || file.ends_with("_test.ts")
        || file.ends_with("_test.js")
        || file.contains(".test.")
        || file.contains(".spec.")
    {
        return true;
    }
    if sym.starts_with("test_") {
        return true;
    }
    content.contains("#[test]")
        || content.contains("#[cfg(test)]")
        || content.contains("assert!(")
        || content.contains("assert_eq!(")
        || content.contains("assert_ne!(")
        || content.contains("def test_")
}

/// Adjust a cosine score with the structural/test/config signals above.
pub(crate) fn rerank_score(base: f32, rel: &str, sym: &str, kind: &str, content: &str) -> f64 {
    let mut s = base as f64;
    let test = is_test_chunk(rel, sym, content);
    if is_config_path(rel) {
        s -= PENALTY_CONFIG;
    }
    if test {
        s -= PENALTY_TEST;
    }
    match kind {
        "file" => s -= PENALTY_WHOLE_FILE,
        // Real definitions get a small boost — but not test functions, whose
        // penalty must not be cancelled by the kind boost.
        "function" | "method" | "struct" | "class" | "trait" | "enum" | "impl" | "const"
        | "module"
            if !test =>
        {
            s += BOOST_SYMBOL
        }
        _ => {}
    }
    s
}

impl Index {
    /// Purge in-memory ANN and query caches (OPT-702 memory hygiene).
    pub fn purge_memory_caches(&self) {
        self.ann.borrow_mut().clear();
        self.ann_order.borrow_mut().clear();
    }

    /// Chunks in a workspace that have no embedding yet for `model_id` (so a
    /// caller can embed them with the egress-guarded provider and store results).
    pub fn pending_embeddings(
        &self,
        ws: &str,
        model_id: &str,
        limit: u32,
    ) -> Result<Vec<PendingChunk>> {
        let [a, b] = workspace_roots(ws);
        let mut stmt = self.conn.prepare(
            "SELECT c.id, fts.content\n\
             FROM chunks c\n\
             JOIN files f ON c.file_path = f.path\n\
             JOIN fts_chunks fts ON fts.chunk_id = c.id\n\
             LEFT JOIN vec_chunks v ON v.chunk_id = c.id AND v.model_id = ?3\n\
             WHERE (f.workspace_root = ?1 OR f.workspace_root = ?2) AND v.chunk_id IS NULL\n\
             LIMIT ?4",
        )?;
        let rows = stmt
            .query_map(params![a, b, model_id, limit], |r| {
                Ok(PendingChunk {
                    chunk_id: r.get(0)?,
                    content: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Store (or replace) the embedding for a chunk under `model_id`.
    pub fn store_embedding(&self, chunk_id: i64, model_id: &str, vec: &[f32]) -> Result<()> {
        self.conn.execute(
            "INSERT INTO vec_chunks(chunk_id, model_id, dim, vec, created_at)\n\
             VALUES (?1, ?2, ?3, ?4, ?5)\n\
             ON CONFLICT(chunk_id, model_id) DO UPDATE SET\n\
                 dim = excluded.dim, vec = excluded.vec, created_at = excluded.created_at",
            params![chunk_id, model_id, vec.len() as i64, encode_vec(vec), now()],
        )?;
        Ok(())
    }

    /// `(embedded, total)` chunk counts for a workspace under `model_id`.
    pub fn embedding_status(&self, ws: &str, model_id: &str) -> Result<(i64, i64)> {
        let [a, b] = workspace_roots(ws);
        let total: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM chunks c JOIN files f ON c.file_path = f.path \
             WHERE f.workspace_root = ?1 OR f.workspace_root = ?2",
            params![a, b],
            |r| r.get(0),
        )?;
        let embedded: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM vec_chunks v\n\
             JOIN chunks c ON c.id = v.chunk_id JOIN files f ON c.file_path = f.path\n\
             WHERE (f.workspace_root = ?1 OR f.workspace_root = ?2) AND v.model_id = ?3",
            params![a, b, model_id],
            |r| r.get(0),
        )?;
        Ok((embedded, total))
    }

    /// Semantic search: brute-force cosine of `query_vec` against this workspace's
    /// stored vectors for `model_id`. Returns ranked, line-ranged hits (score is
    /// cosine similarity in `[-1, 1]`). Brute force is fine at local-repo scale;
    /// `sqlite-vec`/HNSW can replace this behind the same signature.
    ///
    /// Two-phase to keep per-query cost down: phase 1 scores **every** vector but
    /// reads only the vector + light metadata (no chunk text), so we don't
    /// materialize/preview thousands of content strings we'll immediately discard.
    /// Phase 2 fetches content **only** for a small top-K shortlist, applies the
    /// content-aware rerank, and builds previews for just the returned hits. The
    /// shortlist (≈ 5×limit, ≥ 50) is far wider than `limit`, so the content-aware
    /// reorder cannot drop a real winner — ranking quality is unchanged.
    pub fn semantic_search(
        &self,
        ws: &str,
        model_id: &str,
        query_vec: &[f32],
        limit: u32,
    ) -> Result<Vec<Hit>> {
        // Route by corpus size: brute force is exact and cheapest for small
        // corpora (the common single-project case); the ANN index keeps query
        // cost flat as the corpus grows. Both end with the same phase-2 rerank,
        // so result quality is consistent.
        let sig = self.ann_signature(ws, model_id)?;
        if sig.0 > BRUTE_FORCE_MAX {
            return self.semantic_search_ann(ws, model_id, query_vec, limit, sig);
        }
        self.semantic_search_brute(ws, model_id, query_vec, limit)
    }

    /// Exact brute-force cosine over every stored vector. Two-phase: phase 1
    /// scores all vectors (no text read), phase 2 fetches content + reranks a
    /// small shortlist. Used directly for small corpora and as the ANN fallback.
    pub(crate) fn semantic_search_brute(
        &self,
        ws: &str,
        model_id: &str,
        query_vec: &[f32],
        limit: u32,
    ) -> Result<Vec<Hit>> {
        let [a, b] = workspace_roots(ws);
        struct Cand {
            chunk_id: i64,
            rel: String,
            start_line: i64,
            end_line: i64,
            symbol_name: String,
            kind: String,
            base: f32,
            score: f64,
        }

        // Phase 1: cosine + content-free rerank over all candidates (no text read).
        let mut stmt = self.conn.prepare(
            "SELECT v.vec, c.id, f.rel, c.start_line, c.end_line, c.symbol_name, c.kind\n\
             FROM vec_chunks v\n\
             JOIN chunks c ON c.id = v.chunk_id\n\
             JOIN files  f ON c.file_path = f.path\n\
             WHERE (f.workspace_root = ?1 OR f.workspace_root = ?2) AND v.model_id = ?3",
        )?;
        let mut cands: Vec<Cand> = stmt
            .query_map(params![a, b, model_id], |r| {
                let blob: Vec<u8> = r.get(0)?;
                let rel: String = r.get(2)?;
                let symbol_name: String = r.get::<_, Option<String>>(5)?.unwrap_or_default();
                let kind: String = r.get(6)?;
                let base = cosine_le_bytes(&blob, query_vec);
                Ok(Cand {
                    chunk_id: r.get(1)?,
                    start_line: r.get(3)?,
                    end_line: r.get(4)?,
                    // Provisional score for the shortlist cut (content markers folded
                    // in during phase 2). Path/symbol/kind signals already apply.
                    score: rerank_score(base, &rel, &symbol_name, &kind, ""),
                    rel,
                    symbol_name,
                    kind,
                    base,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let k = (limit as usize).saturating_mul(5).max(50);
        cands.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        cands.truncate(k);
        if cands.is_empty() {
            return Ok(Vec::new());
        }

        // Phase 2: fetch content for the shortlist in one pass, then full rerank.
        let placeholders = cands.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql =
            format!("SELECT chunk_id, content FROM fts_chunks WHERE chunk_id IN ({placeholders})");
        let mut content_of: HashMap<i64, String> = HashMap::with_capacity(cands.len());
        {
            let ids: Vec<i64> = cands.iter().map(|c| c.chunk_id).collect();
            let mut cstmt = self.conn.prepare(&sql)?;
            let rows = cstmt.query_map(rusqlite::params_from_iter(ids.iter()), |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                ))
            })?;
            for row in rows {
                let (id, content) = row?;
                content_of.insert(id, content);
            }
        }

        let mut hits: Vec<Hit> = cands
            .into_iter()
            .map(|c| {
                let content = content_of
                    .get(&c.chunk_id)
                    .map(String::as_str)
                    .unwrap_or("");
                Hit {
                    start_line: c.start_line,
                    end_line: c.end_line,
                    score: rerank_score(c.base, &c.rel, &c.symbol_name, &c.kind, content),
                    rel: c.rel,
                    symbol_name: c.symbol_name,
                    preview: preview_of(content, 160),
                }
            })
            .collect();
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits.truncate(limit as usize);
        Ok(hits)
    }

    /// Cheap staleness signature for a (workspace, model)'s stored vectors:
    /// `(count, max(chunk_id), sum(chunk_id))`. Re-embedding deletes and
    /// re-inserts chunk rows (new ids), so any of these shifts when vectors
    /// change — enough to detect that a cached ANN index is stale.
    pub(crate) fn ann_signature(&self, ws: &str, model_id: &str) -> Result<(i64, i64, i64)> {
        let [a, b] = workspace_roots(ws);
        Ok(self.conn.query_row(
            "SELECT COUNT(*), COALESCE(MAX(v.chunk_id), 0), COALESCE(SUM(v.chunk_id), 0)\n\
             FROM vec_chunks v\n\
             JOIN chunks c ON c.id = v.chunk_id\n\
             JOIN files  f ON c.file_path = f.path\n\
             WHERE (f.workspace_root = ?1 OR f.workspace_root = ?2) AND v.model_id = ?3",
            params![a, b, model_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?)
    }

    /// ANN-backed semantic search for large corpora. Builds (and caches) an HNSW
    /// over the normalized stored vectors, queries a wide shortlist of nearest
    /// neighbours, then runs the same content-aware rerank as the brute path.
    pub(crate) fn semantic_search_ann(
        &self,
        ws: &str,
        model_id: &str,
        query_vec: &[f32],
        limit: u32,
        sig: (i64, i64, i64),
    ) -> Result<Vec<Hit>> {
        let key = format!("{ws}\u{0}{model_id}");

        // (Re)build the HNSW if missing or stale. Building reads only id+vector.
        let needs_build = match self.ann.borrow().get(&key) {
            Some(a) => a.signature != sig,
            None => true,
        };
        if needs_build {
            let [a, b] = workspace_roots(ws);
            let mut points: Vec<VecPoint> = Vec::new();
            let mut ids: Vec<i64> = Vec::new();
            {
                let mut stmt = self.conn.prepare(
                    "SELECT v.chunk_id, v.vec\n\
                     FROM vec_chunks v\n\
                     JOIN chunks c ON c.id = v.chunk_id\n\
                     JOIN files  f ON c.file_path = f.path\n\
                     WHERE (f.workspace_root = ?1 OR f.workspace_root = ?2) AND v.model_id = ?3",
                )?;
                let rows = stmt.query_map(params![a, b, model_id], |r| {
                    let id: i64 = r.get(0)?;
                    let blob: Vec<u8> = r.get(1)?;
                    Ok((id, blob))
                })?;
                for row in rows {
                    let (id, blob) = row?;
                    points.push(VecPoint(normalize_vec(&decode_vec(&blob))));
                    ids.push(id);
                }
            }
            // Bounded ef keeps both build and query cost predictable regardless
            // of input distribution (real embeddings cluster; degenerate inputs
            // like near-orthogonal random vectors would otherwise blow up
            // exploration). These are the standard HNSW knobs from the paper.
            let map = instant_distance::Builder::default()
                .ef_construction(64)
                .ef_search(64)
                .build(points, ids);
            ann_cache_put(
                self,
                key.clone(),
                AnnIndex {
                    map,
                    signature: sig,
                },
            );
        }

        // Query a shortlist of nearest neighbours, far wider than `limit` so the
        // content-aware rerank can reorder freely without dropping a real winner.
        let k = (limit as usize).saturating_mul(5).max(50);
        let qn = VecPoint(normalize_vec(query_vec));
        let shortlist: Vec<i64> = {
            let cache = self.ann.borrow();
            let ann = cache
                .get(&key)
                .expect("ANN index was just built/validated above");
            let mut search = instant_distance::Search::default();
            ann.map
                .search(&qn, &mut search)
                .take(k)
                .map(|item| *item.value)
                .collect()
        };
        if shortlist.is_empty() {
            return Ok(Vec::new());
        }

        self.rerank_shortlist(query_vec, &shortlist, limit)
    }

    /// Phase 2 shared by the ANN path: fetch vector + metadata + content for a
    /// shortlist of chunk ids, apply the content-aware rerank, return top hits.
    fn rerank_shortlist(&self, query_vec: &[f32], ids: &[i64], limit: u32) -> Result<Vec<Hit>> {
        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT v.vec, f.rel, c.start_line, c.end_line, c.symbol_name, c.kind, fc.content\n\
             FROM chunks c\n\
             JOIN files f ON c.file_path = f.path\n\
             JOIN vec_chunks v ON v.chunk_id = c.id\n\
             LEFT JOIN fts_chunks fc ON fc.chunk_id = c.id\n\
             WHERE c.id IN ({placeholders})"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut hits: Vec<Hit> = stmt
            .query_map(rusqlite::params_from_iter(ids.iter()), |r| {
                let blob: Vec<u8> = r.get(0)?;
                let rel: String = r.get(1)?;
                let start_line: i64 = r.get(2)?;
                let end_line: i64 = r.get(3)?;
                let symbol_name: String = r.get::<_, Option<String>>(4)?.unwrap_or_default();
                let kind: String = r.get(5)?;
                let content: String = r.get::<_, Option<String>>(6)?.unwrap_or_default();
                let base = cosine_le_bytes(&blob, query_vec);
                let score = rerank_score(base, &rel, &symbol_name, &kind, &content);
                Ok(Hit {
                    start_line,
                    end_line,
                    score,
                    rel,
                    symbol_name,
                    preview: preview_of(&content, 160),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits.truncate(limit as usize);
        Ok(hits)
    }
}

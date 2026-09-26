//! Keyword and structural queries (FTS, outline, mentions).

use std::collections::HashMap;

use rusqlite::params;

use crate::schema::workspace_roots;
use crate::semantic::{is_config_path, is_test_chunk};
use crate::types::{Hit, IndexStatus, OutlineRow, Result, SymbolRow};
use crate::{Index, INDEX_HEALTHY_MIN_FILES};

fn map_symbol_row(r: &rusqlite::Row) -> rusqlite::Result<SymbolRow> {
    Ok(SymbolRow {
        kind: r.get(0)?,
        name: r.get(1)?,
        signature: r.get(2)?,
        start_line: r.get(3)?,
        end_line: r.get(4)?,
        rel: r.get(5)?,
    })
}

pub(crate) fn without_live_sqlite_hits(workspace: &str, hits: Vec<Hit>) -> Vec<Hit> {
    hits.into_iter()
        .filter(|hit| {
            let rel = hit.rel.trim_start_matches(['/', '\\']);
            !crate::schema::is_sqlite_database(&std::path::Path::new(workspace).join(rel))
        })
        .collect()
}

fn map_hit(r: &rusqlite::Row) -> rusqlite::Result<Hit> {
    Ok(Hit {
        rel: r.get(0)?,
        start_line: r.get(1)?,
        end_line: r.get(2)?,
        score: r.get(3)?,
        symbol_name: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
        preview: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
    })
}

/// Rank definition hits so production symbols beat tests/config and shorter paths
/// win ties (common names like `new`/`run` appear in many files).
fn definition_rank(row: &SymbolRow) -> i64 {
    let mut score = 0i64;
    if is_test_chunk(&row.rel, &row.name, "") {
        score -= 200;
    }
    if is_config_path(&row.rel) {
        score -= 100;
    }
    score -= row.rel.len() as i64;
    match row.kind.as_str() {
        "struct" | "class" | "trait" | "enum" | "interface" => score += 30,
        "function" | "method" | "const" | "type" => score += 20,
        "module" => score += 10,
        _ => {}
    }
    score
}

/// Sanitize a user query into a safe FTS5 match string: keep word characters,
/// turn everything else into spaces (tokens are implicitly AND-ed).
/// Turn arbitrary user/agent input into a safe FTS5 MATCH expression. We strip
/// everything but alphanumerics/underscore to spaces, then wrap each token in
/// double quotes so it is treated as a **literal term** — never an FTS5 operator
/// (`OR`/`AND`/`NOT`/`NEAR`) or syntax (`"`, `*`, `:`, `(`). Without the quoting,
/// a query like `foo OR` is a dangling operator and SQLite returns a hard
/// `fts5: syntax error`. Implicit AND between the quoted phrases is the keyword
/// semantics we want. Empty input matches nothing rather than erroring.
fn fts_term(q: &str) -> String {
    let cleaned: String = q
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                ' '
            }
        })
        .collect();
    let tokens: Vec<&str> = cleaned.split_whitespace().collect();
    if tokens.is_empty() {
        // Match nothing rather than erroring on an empty query.
        "\"\"".to_string()
    } else {
        tokens
            .iter()
            .map(|t| format!("\"{t}\""))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl Index {
    /// Find symbol definitions by exact name. Results are ranked: production
    /// code before tests/config, shorter paths before longer ones.
    pub fn find_definition(&self, ws: &str, name: &str) -> Result<Vec<SymbolRow>> {
        self.find_definition_in(ws, name, None)
    }

    /// Like [`Self::find_definition`], optionally restricted to a workspace-relative path.
    pub fn find_definition_in(
        &self,
        ws: &str,
        name: &str,
        rel: Option<&str>,
    ) -> Result<Vec<SymbolRow>> {
        let [a, b] = workspace_roots(ws);
        // `impl` blocks carry the type's name but are not its definition — they
        // belong in `outline`, not here.
        let mut rows = if let Some(r) = rel {
            let mut stmt = self.conn.prepare(
                "SELECT s.kind, s.name, s.signature, s.start_line, s.end_line, f.rel\n\
                 FROM symbols s JOIN files f ON s.file_path = f.path\n\
                 WHERE (f.workspace_root = ?1 OR f.workspace_root = ?2) AND s.name = ?3\n\
                   AND s.kind <> 'impl' AND f.rel = ?4",
            )?;
            let fetched = stmt
                .query_map(params![a, b, name, r], map_symbol_row)?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            fetched
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT s.kind, s.name, s.signature, s.start_line, s.end_line, f.rel\n\
                 FROM symbols s JOIN files f ON s.file_path = f.path\n\
                 WHERE (f.workspace_root = ?1 OR f.workspace_root = ?2) AND s.name = ?3 AND s.kind <> 'impl'",
            )?;
            let fetched = stmt
                .query_map(params![a, b, name], map_symbol_row)?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            fetched
        };
        rows.retain(|row| {
            let rel = row.rel.trim_start_matches(['/', '\\']);
            !crate::schema::is_sqlite_database(&std::path::Path::new(ws).join(rel))
        });
        rows.sort_by(|x, y| {
            definition_rank(y)
                .cmp(&definition_rank(x))
                .then_with(|| x.rel.cmp(&y.rel))
                .then_with(|| x.start_line.cmp(&y.start_line))
        });
        Ok(rows)
    }

    /// Textual mentions of `name` in indexed chunks (FTS keyword hits), excluding
    /// the symbol's own definition chunk. **Not** precise call-graph references —
    /// use LSP `find_references` when available.
    pub fn find_mentions(&self, ws: &str, name: &str, limit: u32) -> Result<Vec<Hit>> {
        let [a, b] = workspace_roots(ws);
        let mut stmt = self.conn.prepare(
            "SELECT f.rel, c.start_line, c.end_line, bm25(fts_chunks),\n\
                    fts_chunks.symbol_name,\n\
                    snippet(fts_chunks, 0, '[', ']', '?', 8)\n\
             FROM fts_chunks\n\
             JOIN chunks c ON c.id = fts_chunks.chunk_id\n\
             JOIN files  f ON f.path = fts_chunks.path\n\
             WHERE fts_chunks MATCH ?1 AND (fts_chunks.workspace_root = ?2 OR fts_chunks.workspace_root = ?3)\n\
               AND (fts_chunks.symbol_name IS NULL OR fts_chunks.symbol_name <> ?4)\n\
             ORDER BY bm25(fts_chunks) LIMIT ?5",
        )?;
        let q = fts_term(name);
        let rows = stmt
            .query_map(params![q, a, b, name, limit], map_hit)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(without_live_sqlite_hits(ws, rows))
    }

    /// Deprecated alias for [`Self::find_mentions`].
    #[deprecated(note = "use find_mentions — this is keyword search, not call-graph references")]
    pub fn find_references(&self, ws: &str, name: &str, limit: u32) -> Result<Vec<Hit>> {
        self.find_mentions(ws, name, limit)
    }

    /// Symbol outline of a single file (by workspace-relative path).
    pub fn outline(&self, ws: &str, rel: &str) -> Result<Vec<OutlineRow>> {
        let rel_clean = rel.trim_start_matches(['/', '\\']);
        if crate::schema::is_sqlite_database(&std::path::Path::new(ws).join(rel_clean)) {
            return Ok(Vec::new());
        }
        let [a, b] = workspace_roots(ws);
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.parent_id, s.kind, s.name, s.signature, s.start_line\n\
             FROM symbols s JOIN files f ON s.file_path = f.path\n\
             WHERE (f.workspace_root = ?1 OR f.workspace_root = ?2) AND f.rel = ?3\n\
             ORDER BY s.start_line",
        )?;
        let rows = stmt
            .query_map(params![a, b, rel], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let parent_of: HashMap<i64, Option<i64>> = rows.iter().map(|r| (r.0, r.1)).collect();
        let depth_of = |mut id: i64| -> usize {
            let mut d = 0;
            while let Some(Some(p)) = parent_of.get(&id) {
                d += 1;
                id = *p;
                if d > 32 {
                    break;
                }
            }
            d
        };
        Ok(rows
            .iter()
            .map(|(id, _p, kind, name, sig, line)| OutlineRow {
                depth: depth_of(*id),
                kind: kind.clone(),
                name: name.clone(),
                signature: sig.clone(),
                start_line: *line,
            })
            .collect())
    }

    /// Keyword search (FTS5 BM25) over the workspace, returning ranked, line-
    /// ranged hits with a keyword-centric preview.
    pub fn search(&self, ws: &str, query: &str, limit: u32) -> Result<Vec<Hit>> {
        let [a, b] = workspace_roots(ws);
        let mut stmt = self.conn.prepare(
            "SELECT f.rel, c.start_line, c.end_line, bm25(fts_chunks),\n\
                    fts_chunks.symbol_name,\n\
                    snippet(fts_chunks, 0, '[', ']', '?', 10)\n\
             FROM fts_chunks\n\
             JOIN chunks c ON c.id = fts_chunks.chunk_id\n\
             JOIN files  f ON f.path = fts_chunks.path\n\
             WHERE fts_chunks MATCH ?1 AND (fts_chunks.workspace_root = ?2 OR fts_chunks.workspace_root = ?3)\n\
             ORDER BY bm25(fts_chunks) LIMIT ?4",
        )?;
        let q = fts_term(query);
        let rows = stmt
            .query_map(params![q, a, b, limit], map_hit)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(without_live_sqlite_hits(ws, rows))
    }

    /// Coverage for a workspace (accepts any path spelling; matches aliased roots).
    pub fn status(&self, ws: &str) -> Result<IndexStatus> {
        let [a, b] = workspace_roots(ws);
        let files: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM files WHERE workspace_root = ?1 OR workspace_root = ?2",
            params![a, b],
            |r| r.get(0),
        )?;
        let symbols: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM symbols s JOIN files f ON s.file_path = f.path \
             WHERE f.workspace_root = ?1 OR f.workspace_root = ?2",
            params![a, b],
            |r| r.get(0),
        )?;
        let chunks: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM chunks c JOIN files f ON c.file_path = f.path \
             WHERE f.workspace_root = ?1 OR f.workspace_root = ?2",
            params![a, b],
            |r| r.get(0),
        )?;
        let last_indexed: Option<String> = self.conn.query_row(
            "SELECT MAX(indexed_at) FROM files WHERE workspace_root = ?1 OR workspace_root = ?2",
            params![a, b],
            |r| r.get(0),
        )?;
        let mut stmt = self.conn.prepare(
            "SELECT lang, COUNT(*) FROM files WHERE workspace_root = ?1 OR workspace_root = ?2 \
             GROUP BY lang ORDER BY COUNT(*) DESC",
        )?;
        let by_lang = stmt
            .query_map(params![a, b], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(IndexStatus {
            files,
            symbols,
            chunks,
            by_lang,
            last_indexed,
        })
    }

    /// True when the workspace has enough indexed files for reliable code retrieval.
    pub fn is_healthy(&self, ws: &str) -> Result<bool> {
        Ok(self.status(ws)?.files >= INDEX_HEALTHY_MIN_FILES)
    }
}

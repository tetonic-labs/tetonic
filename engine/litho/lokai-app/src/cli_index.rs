//! Code Index methods on Application for CLI index, search, and embed operations.

use std::path::{Path, PathBuf};

use crate::errors::AppError;
use crate::Application;

#[derive(Debug, Clone)]
pub struct IndexStatsInfo {
    pub indexed: usize,
    pub unchanged: usize,
    pub skipped: usize,
    pub deleted: usize,
    pub symbols: usize,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone)]
pub struct IndexStatusInfo {
    pub files: i64,
    pub symbols: i64,
    pub chunks: i64,
    pub last_indexed: Option<String>,
    pub embedded_chunks: i64,
    pub total_chunks: i64,
    pub by_lang: Vec<(String, i64)>,
}

#[derive(Debug, Clone)]
pub struct DefinitionRowInfo {
    pub kind: String,
    pub rel: String,
    pub start_line: i64,
    pub signature: String,
}

#[derive(Debug, Clone)]
pub struct MentionHitInfo {
    pub rel: String,
    pub start_line: i64,
    pub preview: String,
}

#[derive(Debug, Clone)]
pub struct OutlineRowInfo {
    pub kind: String,
    pub name: String,
    pub start_line: i64,
    pub depth: usize,
}

#[derive(Debug, Clone)]
pub struct SearchHitInfo {
    pub rel: String,
    pub start_line: i64,
    pub end_line: i64,
    pub symbol_name: String,
    pub preview: String,
    pub score: Option<f64>,
}

pub fn default_index_db_path() -> Result<PathBuf, AppError> {
    let dirs = directories::ProjectDirs::from("", "", "lokai").ok_or_else(|| {
        AppError::InvalidRequest("could not resolve data dir for index.db".into())
    })?;
    Ok(dirs.data_dir().join("index.db"))
}

impl Application {
    pub fn index_db_path(&self) -> Result<PathBuf, AppError> {
        if let Some(p) = &self.turn.index_db {
            return Ok(p.clone());
        }
        default_index_db_path()
    }

    pub fn index_workspace(&self, ws_root: &str) -> Result<IndexStatsInfo, AppError> {
        let idx_path = self.index_db_path()?;
        let index = lokai_index::Index::open(&idx_path)
            .map_err(|e| AppError::InvalidRequest(format!("open index.db: {e}")))?;
        let stats = index
            .index_workspace(Path::new(ws_root))
            .map_err(|e| AppError::InvalidRequest(format!("indexing workspace: {e}")))?;
        Ok(IndexStatsInfo {
            indexed: stats.indexed,
            unchanged: stats.unchanged,
            skipped: stats.skipped,
            deleted: stats.deleted,
            symbols: stats.symbols,
            elapsed_ms: stats.elapsed_ms,
        })
    }

    pub fn index_status(&self, ws_root: &str, model: &str) -> Result<IndexStatusInfo, AppError> {
        let idx_path = self.index_db_path()?;
        let index = lokai_index::Index::open(&idx_path)
            .map_err(|e| AppError::InvalidRequest(format!("open index.db: {e}")))?;
        let s = index
            .status(ws_root)
            .map_err(|e| AppError::InvalidRequest(format!("index status: {e}")))?;
        let (embedded_chunks, total_chunks) =
            index.embedding_status(ws_root, model).unwrap_or((0, 0));
        Ok(IndexStatusInfo {
            files: s.files,
            symbols: s.symbols,
            chunks: s.chunks,
            last_indexed: s.last_indexed,
            embedded_chunks,
            total_chunks,
            by_lang: s.by_lang,
        })
    }

    pub fn find_definition(
        &self,
        ws_root: &str,
        name: &str,
    ) -> Result<Vec<DefinitionRowInfo>, AppError> {
        let idx_path = self.index_db_path()?;
        let index = lokai_index::Index::open(&idx_path)
            .map_err(|e| AppError::InvalidRequest(format!("open index.db: {e}")))?;
        let rows = index
            .find_definition(ws_root, name)
            .map_err(|e| AppError::InvalidRequest(e.to_string()))?;
        Ok(rows
            .into_iter()
            .map(|r| DefinitionRowInfo {
                kind: r.kind,
                rel: r.rel,
                start_line: r.start_line,
                signature: r.signature,
            })
            .collect())
    }

    pub fn find_mentions(
        &self,
        ws_root: &str,
        name: &str,
        limit: usize,
    ) -> Result<Vec<MentionHitInfo>, AppError> {
        let idx_path = self.index_db_path()?;
        let index = lokai_index::Index::open(&idx_path)
            .map_err(|e| AppError::InvalidRequest(format!("open index.db: {e}")))?;
        let hits = index
            .find_mentions(ws_root, name, limit as u32)
            .map_err(|e| AppError::InvalidRequest(e.to_string()))?;
        Ok(hits
            .into_iter()
            .map(|h| MentionHitInfo {
                rel: h.rel,
                start_line: h.start_line,
                preview: h.preview,
            })
            .collect())
    }

    pub fn index_outline(
        &self,
        ws_root: &str,
        path: &str,
    ) -> Result<Vec<OutlineRowInfo>, AppError> {
        let idx_path = self.index_db_path()?;
        let index = lokai_index::Index::open(&idx_path)
            .map_err(|e| AppError::InvalidRequest(format!("open index.db: {e}")))?;
        let rows = index
            .outline(ws_root, &path.replace('\\', "/"))
            .map_err(|e| AppError::InvalidRequest(e.to_string()))?;
        Ok(rows
            .into_iter()
            .map(|r| OutlineRowInfo {
                kind: r.kind,
                name: r.name,
                start_line: r.start_line,
                depth: r.depth,
            })
            .collect())
    }

    pub fn prune_index_workspaces(&self) -> Result<usize, AppError> {
        let idx_path = self.index_db_path()?;
        let index = lokai_index::Index::open(&idx_path)
            .map_err(|e| AppError::InvalidRequest(format!("open index.db: {e}")))?;
        index
            .prune_missing_workspaces()
            .map_err(|e| AppError::InvalidRequest(e.to_string()))
    }

    pub fn index_search(
        &self,
        ws_root: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchHitInfo>, AppError> {
        let idx_path = self.index_db_path()?;
        let index = lokai_index::Index::open(&idx_path)
            .map_err(|e| AppError::InvalidRequest(format!("open index.db: {e}")))?;
        let hits = index
            .search(ws_root, query, limit as u32)
            .map_err(|e| AppError::InvalidRequest(e.to_string()))?;
        Ok(hits
            .into_iter()
            .map(|h| SearchHitInfo {
                rel: h.rel,
                start_line: h.start_line,
                end_line: h.end_line,
                symbol_name: h.symbol_name,
                preview: h.preview,
                score: None,
            })
            .collect())
    }

    pub async fn index_semantic_search(
        &self,
        ws_root: &str,
        model: &str,
        query: &str,
        limit: usize,
    ) -> Result<(Vec<SearchHitInfo>, usize), AppError> {
        let idx_path = self.index_db_path()?;
        let index = lokai_index::Index::open(&idx_path)
            .map_err(|e| AppError::InvalidRequest(format!("open index.db: {e}")))?;
        let guard = self.turn.guard();
        let base = self.turn.ollama_base();
        let provider = lokai_inference::OllamaProvider::new(&base, guard.clone());
        let qv = provider
            .embed(model, std::slice::from_ref(&query.to_string()))
            .await
            .map_err(|e| AppError::InvalidRequest(format!("embedding query: {e}")))?
            .into_iter()
            .next()
            .unwrap_or_default();
        let hits = index
            .semantic_search(ws_root, model, &qv, limit as u32)
            .map_err(|e| AppError::InvalidRequest(e.to_string()))?;
        let net_requests = guard.activity_log().len();
        Ok((
            hits.into_iter()
                .map(|h| SearchHitInfo {
                    rel: h.rel,
                    start_line: h.start_line,
                    end_line: h.end_line,
                    symbol_name: h.symbol_name,
                    preview: h.preview,
                    score: Some(h.score),
                })
                .collect(),
            net_requests,
        ))
    }

    pub async fn embed_workspace<F>(
        &self,
        ws_root: &str,
        model: &str,
        progress: F,
    ) -> Result<(usize, i64, i64, usize), AppError>
    where
        F: Fn(usize, usize),
    {
        let idx_path = self.index_db_path()?;
        let index = lokai_index::Index::open(&idx_path)
            .map_err(|e| AppError::InvalidRequest(format!("open index.db: {e}")))?;
        let guard = self.turn.guard();
        let base = self.turn.ollama_base();
        let provider = lokai_inference::OllamaProvider::new(&base, guard.clone());

        let pending = index
            .pending_embeddings(ws_root, model, 1_000_000)
            .map_err(|e| AppError::InvalidRequest(e.to_string()))?;
        if pending.is_empty() {
            let (emb, total) = index
                .embedding_status(ws_root, model)
                .map_err(|e| AppError::InvalidRequest(e.to_string()))?;
            return Ok((0, emb, total, guard.activity_log().len()));
        }

        let total = pending.len();
        let mut embedded = 0usize;
        for batch in pending.chunks(128) {
            let texts: Vec<String> = batch.iter().map(|p| p.content.clone()).collect();
            let vecs = provider
                .embed(model, &texts)
                .await
                .map_err(|e| AppError::InvalidRequest(format!("embedding batch: {e}")))?;
            if vecs.len() != batch.len() {
                return Err(AppError::InvalidRequest(format!(
                    "embedder returned {} vector(s) for {} input(s) — model '{model}' may not be an embedding model",
                    vecs.len(),
                    batch.len()
                )));
            }
            for (p, v) in batch.iter().zip(vecs) {
                index
                    .store_embedding(p.chunk_id, model, &v)
                    .map_err(|e| AppError::PersistenceFailed(e.to_string()))?;
                embedded += 1;
            }
            progress(embedded, total);
        }

        let (emb, tot) = index
            .embedding_status(ws_root, model)
            .map_err(|e| AppError::InvalidRequest(e.to_string()))?;
        Ok((embedded, emb, tot, guard.activity_log().len()))
    }

    pub fn watch_index_blocking(&self, ws_root: &str) -> Result<(), AppError> {
        let idx_path = self.index_db_path()?;
        lokai_index::watch_index_blocking(&idx_path, Path::new(ws_root))
            .map_err(|e| AppError::InvalidRequest(format!("watch index: {e}")))
    }

    pub fn resolve_latest_workspace(&self) -> Result<Option<String>, AppError> {
        let store = match self.store() {
            Some(s) => s,
            None => return Ok(None),
        };
        store
            .read_sync(|db| {
                let rows = db
                    .list_recent_sessions(1)
                    .map_err(|e| AppError::PersistenceFailed(e.to_string()))?;
                Ok(rows.into_iter().next().map(|s| s.workspace_root))
            })
            .map_err(|e| AppError::PersistenceFailed(e.to_string()))?
    }
}

//! Schema migration and file indexing.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use crate::parse::extract;
use crate::types::{IndexStats, Lang, Result, MAX_CHUNK_CHARS, MAX_FILE_BYTES};
use crate::util::{cap_chars, content_hash, now};
use crate::Index;

#[cfg(test)]
#[path = "update_perf_tests.rs"]
mod update_perf_tests;

type FileIncrementality = HashMap<String, (String, Option<i64>, i64)>;

pub(crate) fn storage_path_key(path: &str) -> String {
    if cfg!(windows) {
        path.strip_prefix(r"\\?\").unwrap_or(path).to_string()
    } else {
        path.to_string()
    }
}

/// Both spellings of a path that can appear on Windows after canonicalize drift.
pub(crate) fn path_lookup_keys(path: &str) -> [String; 2] {
    let norm = storage_path_key(path);
    if cfg!(windows) {
        [norm.clone(), format!(r"\\?\{}", norm)]
    } else {
        [norm.clone(), norm]
    }
}

pub(crate) fn workspace_roots(ws: &str) -> [String; 2] {
    path_lookup_keys(ws)
}

/// Per-file metadata for incrementality (scoped to one workspace root).
fn load_known_for_workspace(conn: &Connection, ws: &str) -> Result<FileIncrementality> {
    let [a, b] = workspace_roots(ws);
    let mut known: FileIncrementality = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT path, content_hash, mtime, size FROM files WHERE workspace_root = ?1 OR workspace_root = ?2",
    )?;
    let rows = stmt.query_map(params![a, b], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<i64>>(2)?,
            r.get::<_, i64>(3)?,
        ))
    })?;
    for row in rows {
        let (p, h, m, sz) = row?;
        known.insert(storage_path_key(&p), (h, m, sz));
    }
    Ok(known)
}

enum FileIndexOutcome {
    Unchanged,
    Indexed { symbols: usize },
    Skipped,
}

/// Index one on-disk file (or skip if unchanged). Caller updates `stats`.
fn index_file_at_path(
    tx: &Connection,
    ws: &str,
    walk_root: &Path,
    abs: &Path,
    known: &FileIncrementality,
    self_db: &str,
) -> Result<FileIndexOutcome> {
    if abs.to_string_lossy().starts_with(self_db) || is_control_store_file(abs) {
        // An earlier pass may already have stored this file. Skipping the read
        // must also drop those rows, or search can still return them.
        purge_file_at_path(tx, &storage_path_key(&abs.to_string_lossy()))?;
        return Ok(FileIndexOutcome::Skipped);
    }
    let meta = abs.metadata()?;
    if !meta.is_file() {
        return Ok(FileIndexOutcome::Skipped);
    }
    if meta.len() > MAX_FILE_BYTES {
        return Ok(FileIndexOutcome::Skipped);
    }
    let abs_str = storage_path_key(&abs.to_string_lossy());
    // Any SQLite file, whatever its name, can be the control database. Do not
    // keep a previous text index of it, and do not read the rest of the file.
    if is_sqlite_database(abs) {
        purge_file_at_path(tx, &abs_str)?;
        return Ok(FileIndexOutcome::Skipped);
    }
    let size = meta.len() as i64;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);

    if let Some((_, kmtime, ksize)) = known.get(&abs_str) {
        if *ksize == size && kmtime.is_some() && *kmtime == mtime {
            return Ok(FileIndexOutcome::Unchanged);
        }
    }

    let raw = std::fs::read(abs)?;
    if raw.contains(&0) {
        purge_file_at_path(tx, &abs_str)?;
        return Ok(FileIndexOutcome::Skipped);
    }
    let hash = content_hash(&raw);
    if let Some((khash, _, _)) = known.get(&abs_str) {
        if khash == &hash {
            tx.execute(
                "UPDATE files SET mtime = ?1 WHERE path = ?2",
                params![mtime, abs_str],
            )?;
            return Ok(FileIndexOutcome::Unchanged);
        }
    }
    let source = String::from_utf8_lossy(&raw).into_owned();
    let lang = Lang::from_path(abs);
    let rel = rel_path(walk_root, abs);
    let existed = known.contains_key(&abs_str) || file_path_exists(tx, &abs_str).unwrap_or(false);
    let n = index_one_file(
        tx, ws, &abs_str, &rel, lang, &source, &hash, mtime, size, existed,
    )?;
    Ok(FileIndexOutcome::Indexed { symbols: n })
}

pub(crate) fn is_sqlite_database(path: &Path) -> bool {
    if sqlite_header_prefix(path) {
        return true;
    }
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(stem) = name
        .strip_suffix("-wal")
        .or_else(|| name.strip_suffix("-shm"))
        .or_else(|| name.strip_suffix("-journal"))
    else {
        return false;
    };
    !stem.is_empty() && sqlite_header_prefix(&path.with_file_name(stem))
}

fn sqlite_header_prefix(path: &Path) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut magic = [0u8; 16];
    let Ok(n) = std::io::Read::read(&mut file, &mut magic) else {
        return false;
    };
    let magic = &magic[..n];
    if magic.len() >= 15 && magic.starts_with(b"SQLite format 3") {
        return true;
    }
    magic.len() >= 4
        && matches!(
            [magic[0], magic[1], magic[2], magic[3]],
            [0x37, 0x7f, 0x06, 0x82]
                | [0x37, 0x7f, 0x06, 0x83]
                | [0x82, 0x06, 0x7f, 0x37]
                | [0x83, 0x06, 0x7f, 0x37]
        )
}

fn is_control_store_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    matches!(
        name.to_ascii_lowercase().as_str(),
        "lokai.db" | "lokai.db-wal" | "lokai.db-shm"
    )
}

fn file_path_exists(conn: &Connection, path: &str) -> Result<bool> {
    let [a, b] = path_lookup_keys(path);
    let found = conn
        .query_row(
            "SELECT 1 FROM files WHERE path = ?1 OR path = ?2 LIMIT 1",
            params![a, b],
            |_| Ok(()),
        )
        .optional()?;
    Ok(found.is_some())
}

fn purge_file_at_path(tx: &Connection, path: &str) -> Result<()> {
    let [a, b] = path_lookup_keys(path);
    tx.prepare_cached("DELETE FROM fts_chunks WHERE rowid IN (SELECT row_id FROM fts_file_rows WHERE file_path = ?1 OR file_path = ?2)")?
        .execute(params![a, b])?;
    tx.prepare_cached("DELETE FROM files WHERE path = ?1 OR path = ?2")?
        .execute(params![a, b])?;
    Ok(())
}

fn rel_path(root: &Path, abs: &Path) -> String {
    let rel = abs.strip_prefix(root).unwrap_or(abs);
    rel.to_string_lossy().replace('\\', "/")
}

/// Index a single file within a transaction; returns the symbol count.
#[allow(clippy::too_many_arguments)]
fn index_one_file(
    tx: &Connection,
    ws: &str,
    abs: &str,
    rel: &str,
    lang: Lang,
    source: &str,
    hash: &str,
    mtime: Option<i64>,
    size: i64,
    existed: bool,
) -> Result<usize> {
    // Replace any prior rows for this path. All the inserts below run once per
    // symbol/chunk, so they go through `prepare_cached`: the compiled statement is
    // reused across every symbol and every file in this indexing pass (same
    // connection), instead of being re-prepared on each call.
    //
    // The deletes are skipped for brand-new paths: `path` is UNINDEXED in the
    // FTS5 table, so `DELETE ... WHERE path = ?` scans the whole FTS index, which
    // turns a cold (all-new) pass quadratic. Files cascade-delete via the FK on
    // `files`, so deleting the `files` row clears symbols/chunks; the FTS rows
    // have no FK, hence the explicit FTS delete on replace.
    if existed {
        purge_file_at_path(tx, abs)?;
    }
    tx.prepare_cached(
        "INSERT INTO files(path, workspace_root, rel, content_hash, mtime, lang, size, indexed_at, embed_state)\n\
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'none')\n\
         ON CONFLICT(path) DO UPDATE SET\n\
           workspace_root = excluded.workspace_root,\n\
           rel = excluded.rel,\n\
           content_hash = excluded.content_hash,\n\
           mtime = excluded.mtime,\n\
           lang = excluded.lang,\n\
           size = excluded.size,\n\
           indexed_at = excluded.indexed_at,\n\
           embed_state = 'none'",
    )?
    .execute(params![abs, ws, rel, hash, mtime, lang.as_str(), size, now()])?;

    let (symbols, imports) = extract(lang, source);

    let mut db_ids: Vec<i64> = Vec::with_capacity(symbols.len());
    for s in &symbols {
        let parent_db = s.parent_local.map(|i| db_ids[i]);
        tx.prepare_cached(
            "INSERT INTO symbols(file_path, kind, name, signature, start_line, end_line, parent_id)\n\
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?
        .execute(params![abs, s.kind, s.name, s.signature, s.start_line, s.end_line, parent_db])?;
        db_ids.push(tx.last_insert_rowid());

        let content = cap_chars(&source[s.start_byte..s.end_byte], MAX_CHUNK_CHARS);
        let chunk_id = insert_chunk(tx, abs, s.start_line, s.end_line, &s.kind, Some(&s.name))?;
        insert_fts(tx, &content, Some(&s.name), abs, rel, ws, chunk_id)?;
    }

    for imp in &imports {
        tx.prepare_cached("INSERT INTO imports(file_path, target, line) VALUES (?1, ?2, ?3)")?
            .execute(params![abs, imp.target, imp.line])?;
    }

    // No symbols (plain text, or a parse we don't structure): index the whole
    // file as one chunk so keyword search still covers it.
    if symbols.is_empty() {
        let line_count = source.lines().count().max(1) as i64;
        let content = cap_chars(source, MAX_CHUNK_CHARS * 4);
        let chunk_id = insert_chunk(tx, abs, 1, line_count, "file", None)?;
        insert_fts(tx, &content, None, abs, rel, ws, chunk_id)?;
    }

    Ok(symbols.len())
}

fn insert_chunk(
    tx: &Connection,
    abs: &str,
    start_line: i64,
    end_line: i64,
    kind: &str,
    symbol_name: Option<&str>,
) -> Result<i64> {
    tx.prepare_cached(
        "INSERT INTO chunks(file_path, start_line, end_line, kind, symbol_name) VALUES (?1, ?2, ?3, ?4, ?5)",
    )?
    .execute(params![abs, start_line, end_line, kind, symbol_name])?;
    Ok(tx.last_insert_rowid())
}

fn insert_fts(
    tx: &Connection,
    content: &str,
    symbol_name: Option<&str>,
    abs: &str,
    rel: &str,
    ws: &str,
    chunk_id: i64,
) -> Result<()> {
    tx.prepare_cached(
        "INSERT INTO fts_chunks(content, symbol_name, path, rel, workspace_root, chunk_id)\n\
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?
    .execute(params![
        content,
        symbol_name.unwrap_or(""),
        abs,
        rel,
        ws,
        chunk_id
    ])?;
    tx.prepare_cached("INSERT INTO fts_file_rows(row_id, file_path) VALUES (?1, ?2)")?
        .execute(params![tx.last_insert_rowid(), abs])?;
    Ok(())
}

impl Index {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)?;
        conn.execute_batch(
            // cache_size (negative = KiB) and mmap keep large cold-index passes off
            // disk: with the default ~2 MiB cache, inserting tens of thousands of
            // FTS5 rows in one transaction thrashes the page cache and indexing
            // degrades super-linearly. temp_store=MEMORY keeps FTS merge scratch in
            // RAM. These are tuning only — correctness is identical.
            "PRAGMA journal_mode=WAL;\n\
             PRAGMA synchronous=NORMAL;\n\
             PRAGMA foreign_keys=ON;\n\
             PRAGMA busy_timeout=5000;\n\
             PRAGMA cache_size=-65536;\n\
             PRAGMA temp_store=MEMORY;\n\
             PRAGMA mmap_size=268435456;",
        )?;
        let idx = Self {
            conn,
            path,
            ann: std::cell::RefCell::new(HashMap::new()),
            ann_order: std::cell::RefCell::new(Vec::new()),
        };
        idx.migrate()?;
        let _ =
            idx.sync_classification_policy_version(tetonic_domain::CLASSIFICATION_POLICY_VERSION)?;
        Ok(idx)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_versions (\n\
                 version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL\n\
             );",
        )?;
        let applied: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |r| r.get(0),
        )?;

        if applied < 1 {
            self.conn.execute_batch(
                "CREATE TABLE files (\n\
                     path           TEXT PRIMARY KEY,\n\
                     workspace_root TEXT NOT NULL,\n\
                     rel            TEXT NOT NULL,\n\
                     content_hash   TEXT NOT NULL,\n\
                     mtime          INTEGER,\n\
                     lang           TEXT NOT NULL,\n\
                     size           INTEGER NOT NULL,\n\
                     indexed_at     TEXT NOT NULL,\n\
                     embed_state    TEXT NOT NULL DEFAULT 'none'\n\
                 );\n\
                 CREATE INDEX idx_files_ws ON files(workspace_root);\n\
                 \n\
                 CREATE TABLE symbols (\n\
                     id         INTEGER PRIMARY KEY,\n\
                     file_path  TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE,\n\
                     kind       TEXT NOT NULL,\n\
                     name       TEXT NOT NULL,\n\
                     signature  TEXT NOT NULL DEFAULT '',\n\
                     start_line INTEGER NOT NULL,\n\
                     end_line   INTEGER NOT NULL,\n\
                     parent_id  INTEGER\n\
                 );\n\
                 CREATE INDEX idx_symbols_name ON symbols(name);\n\
                 CREATE INDEX idx_symbols_file ON symbols(file_path);\n\
                 \n\
                 CREATE TABLE imports (\n\
                     id        INTEGER PRIMARY KEY,\n\
                     file_path TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE,\n\
                     target    TEXT NOT NULL,\n\
                     line      INTEGER NOT NULL\n\
                 );\n\
                 \n\
                 CREATE TABLE chunks (\n\
                     id          INTEGER PRIMARY KEY,\n\
                     file_path   TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE,\n\
                     start_line  INTEGER NOT NULL,\n\
                     end_line    INTEGER NOT NULL,\n\
                     kind        TEXT NOT NULL,\n\
                     symbol_name TEXT\n\
                 );\n\
                 CREATE INDEX idx_chunks_file ON chunks(file_path);\n\
                 \n\
                 CREATE VIRTUAL TABLE fts_chunks USING fts5 (\n\
                     content,\n\
                     symbol_name,\n\
                     path UNINDEXED,\n\
                     rel UNINDEXED,\n\
                     workspace_root UNINDEXED,\n\
                     chunk_id UNINDEXED,\n\
                     tokenize = 'unicode61'\n\
                 );",
            )?;
            self.conn.execute(
                "INSERT INTO schema_versions(version, applied_at) VALUES (1, ?1)",
                params![now()],
            )?;
        }

        if applied < 2 {
            // Semantic layer: one vector per (chunk, embedding model). Stored as a
            // raw little-endian f32 BLOB. FK to chunks so a re-indexed file's stale
            // vectors are cascaded away (content change → re-embed). Brute-force
            // cosine search for now; `sqlite-vec` is a drop-in behind the queries.
            self.conn.execute_batch(
                "CREATE TABLE vec_chunks (\n\
                     chunk_id   INTEGER NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,\n\
                     model_id   TEXT NOT NULL,\n\
                     dim        INTEGER NOT NULL,\n\
                     vec        BLOB NOT NULL,\n\
                     created_at TEXT NOT NULL,\n\
                     PRIMARY KEY (chunk_id, model_id)\n\
                 );\n\
                 CREATE INDEX idx_vec_model ON vec_chunks(model_id);",
            )?;
            self.conn.execute(
                "INSERT INTO schema_versions(version, applied_at) VALUES (2, ?1)",
                params![now()],
            )?;
        }

        if applied < 3 {
            // M2-2: track classification policy version so cached embeddings can be
            // invalidated when rules change.
            self.conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS index_metadata (\n\
                     key   TEXT PRIMARY KEY,\n\
                     value TEXT NOT NULL\n\
                 );",
            )?;
            self.conn.execute(
                "INSERT INTO schema_versions(version, applied_at) VALUES (3, ?1)",
                params![now()],
            )?;
        }
        if applied < 4 {
            // FTS path is UNINDEXED. Maintain an ordinary indexed lookup so replacing
            // one file does not scan every other file's full-text rows.
            let tx = self.conn.unchecked_transaction()?;
            tx.execute_batch("CREATE TABLE IF NOT EXISTS fts_file_rows (
                row_id INTEGER PRIMARY KEY,
                file_path TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_fts_file_rows_path ON fts_file_rows(file_path);
            INSERT OR REPLACE INTO fts_file_rows(row_id, file_path) SELECT rowid, path FROM fts_chunks;")?;
            tx.execute(
                "INSERT INTO schema_versions(version, applied_at) VALUES (4, ?1)",
                params![now()],
            )?;
            tx.commit()?;
        }
        Ok(())
    }

    /// Stored classification policy version (M2-2), if any.
    pub fn stored_classification_policy_version(&self) -> Result<Option<u32>> {
        let v: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM index_metadata WHERE key = 'classification_policy_version'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        Ok(v.and_then(|s| s.parse().ok()))
    }

    /// Sync index metadata to the current classification policy version.
    /// Returns `true` when embeddings were invalidated due to a policy bump.
    pub fn sync_classification_policy_version(&self, current: u32) -> Result<bool> {
        let stored = self.stored_classification_policy_version()?;
        if stored == Some(current) {
            return Ok(false);
        }
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM vec_chunks", [])?;
        tx.execute(
            "INSERT INTO index_metadata(key, value) VALUES ('classification_policy_version', ?1)\n\
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![current.to_string()],
        )?;
        tx.commit()?;
        self.ann.borrow_mut().clear();
        self.ann_order.borrow_mut().clear();
        Ok(stored.is_some())
    }

    /// (Re)index a workspace tree, touching only files whose content changed and
    /// dropping rows for files that disappeared. Respects `.gitignore` (via the
    /// `ignore` crate, same as the tools). I/O failures roll back the pass so an
    /// incomplete inventory cannot purge existing rows as if files were deleted.
    pub fn index_workspace(&self, root: &Path) -> Result<IndexStats> {
        self.index_workspace_inner(root, true)
    }

    /// Recover from incomplete notifications, including same-size rapid edits.
    pub(crate) fn reconcile_workspace(&self, root: &Path) -> Result<IndexStats> {
        self.index_workspace_inner(root, false)
    }

    fn index_workspace_inner(&self, root: &Path, trust_metadata: bool) -> Result<IndexStats> {
        let start = std::time::Instant::now();
        let ws = storage_path_key(&root.to_string_lossy());
        let walk_root = Path::new(&ws);
        let mut stats = IndexStats::default();

        // Existing (content_hash, mtime, size) per file for this workspace. mtime+
        // size drive a cheap "unchanged" fast path that avoids reading the file at
        // all; content_hash is the fallback when stat metadata moved.
        let mut known = load_known_for_workspace(&self.conn, &ws)?;
        if !trust_metadata {
            for (_, mtime, _) in known.values_mut() {
                *mtime = None;
            }
        }

        let tx = self.conn.unchecked_transaction()?;
        let mut seen: HashSet<String> = HashSet::new();
        // Never index our own DB (or its WAL/SHM/journal siblings) if it happens
        // to live inside the workspace tree.
        let self_db = self.path.to_string_lossy().to_string();

        // Bulk-load fast path. When this workspace has no rows yet, every file is
        // a fresh insert and there is nothing to delete — so we can defer the
        // expensive per-row maintenance that makes a cold pass scale
        // superlinearly: FTS5 segment auto-merging and the heavy secondary
        // B-tree indexes. We drop those indexes + disable automerge now, then
        // rebuild the indexes and run a single FTS5 'optimize' at the end. It is
        // all inside this transaction, so a crash rolls back cleanly (no
        // half-built indexes). The incremental path (known not empty) is untouched.
        let cold = known.is_empty();
        if cold {
            tx.execute_batch(
                "DROP INDEX IF EXISTS idx_symbols_name;\n\
                 DROP INDEX IF EXISTS idx_symbols_file;\n\
                 DROP INDEX IF EXISTS idx_chunks_file;",
            )?;
            tx.execute(
                "INSERT INTO fts_chunks(fts_chunks, rank) VALUES('automerge', 0)",
                [],
            )?;
        }

        for entry in ignore::WalkBuilder::new(walk_root)
            .git_ignore(true)
            .require_git(false)
            .build()
        {
            let entry = match entry {
                Ok(e) => e,
                Err(error) => return Err(std::io::Error::other(error.to_string()).into()),
            };
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            let abs = entry.path();
            stats.seen += 1;
            seen.insert(storage_path_key(&abs.to_string_lossy()));

            match index_file_at_path(&tx, &ws, walk_root, abs, &known, &self_db)? {
                FileIndexOutcome::Unchanged => stats.unchanged += 1,
                FileIndexOutcome::Indexed { symbols } => {
                    stats.symbols += symbols;
                    stats.indexed += 1;
                }
                FileIndexOutcome::Skipped => stats.skipped += 1,
            }
        }

        // Files that vanished since last pass: drop them (cascades symbols/etc).
        for path in known.keys() {
            if !seen.contains(path) {
                purge_file_at_path(&tx, path)?;
                stats.deleted += 1;
            }
        }

        // Close out the bulk-load fast path: merge FTS5 down to a single segment
        // once (instead of incrementally per insert), restore default automerge
        // so the incremental path stays healthy, and rebuild the secondary
        // indexes in one shot.
        if cold {
            tx.execute("INSERT INTO fts_chunks(fts_chunks) VALUES('optimize')", [])?;
            tx.execute(
                "INSERT INTO fts_chunks(fts_chunks, rank) VALUES('automerge', 4)",
                [],
            )?;
            tx.execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);\n\
                 CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_path);\n\
                 CREATE INDEX IF NOT EXISTS idx_chunks_file ON chunks(file_path);",
            )?;
        }

        tx.commit()?;
        stats.elapsed_ms = start.elapsed().as_millis();
        Ok(stats)
    }

    /// Incrementally index only the listed paths (watcher / editor save path).
    /// Missing paths are purged from the index. Directory events fall back to
    /// [`Self::index_workspace`] (rare vs single-file saves).
    pub fn index_paths(&self, root: &Path, paths: &[PathBuf]) -> Result<IndexStats> {
        if paths.is_empty() {
            return Ok(IndexStats::default());
        }
        let start = std::time::Instant::now();
        let ws = storage_path_key(&root.to_string_lossy());
        let walk_root = Path::new(&ws);

        for p in paths {
            if p.exists() && p.is_dir() {
                return self.reconcile_workspace(root);
            }
        }

        let mut known = load_known_for_workspace(&self.conn, &ws)?;
        for (_, mtime, _) in known.values_mut() {
            *mtime = None;
        }
        let self_db = self.path.to_string_lossy().to_string();
        let tx = self.conn.unchecked_transaction()?;
        let mut stats = IndexStats::default();
        let mut purged: HashSet<String> = HashSet::new();

        for p in paths {
            let abs = if p.is_absolute() {
                p.clone()
            } else {
                walk_root.join(p)
            };
            let abs_str = storage_path_key(&abs.to_string_lossy());

            if !abs.exists() {
                let to_drop: Vec<String> = if known.contains_key(&abs_str) {
                    vec![abs_str.clone()]
                } else {
                    let prefix = format!("{abs_str}/");
                    known
                        .keys()
                        .filter(|k| k.starts_with(&prefix))
                        .cloned()
                        .collect()
                };
                for path in to_drop {
                    if purged.insert(path.clone()) {
                        purge_file_at_path(&tx, &path)?;
                        stats.deleted += 1;
                    }
                }
                continue;
            }

            stats.seen += 1;
            match index_file_at_path(&tx, &ws, walk_root, &abs, &known, &self_db)? {
                FileIndexOutcome::Unchanged => stats.unchanged += 1,
                FileIndexOutcome::Indexed { symbols } => {
                    stats.symbols += symbols;
                    stats.indexed += 1;
                }
                FileIndexOutcome::Skipped => stats.skipped += 1,
            }
        }

        tx.commit()?;
        stats.elapsed_ms = start.elapsed().as_millis();
        Ok(stats)
    }

    /// New paths need the workspace walker to apply ignore rules. Existing
    /// indexed files can take the bounded incremental update path.
    pub(crate) fn index_watcher_paths(&self, root: &Path, paths: &[PathBuf]) -> Result<IndexStats> {
        for path in paths {
            if !file_path_exists(&self.conn, &storage_path_key(&path.to_string_lossy()))? {
                return self.reconcile_workspace(root);
            }
        }
        self.index_paths(root, paths)
    }

    /// Garbage-collect workspaces whose root directory no longer exists on disk
    /// (e.g. deleted temp dirs, removed checkouts). The shared `index.db`
    /// otherwise accumulates dead roots forever. Returns the number of roots
    /// pruned. Best-effort and safe to call routinely: the index is disposable,
    /// so a wrongly-pruned (e.g. transiently unmounted) root just re-indexes
    /// later. `files` rows cascade to symbols/chunks/imports/vec_chunks via FK;
    /// `fts_chunks` has no FK, so it is cleared explicitly per root.
    pub fn prune_missing_workspaces(&self) -> Result<usize> {
        let roots: Vec<String> = {
            let mut stmt = self
                .conn
                .prepare("SELECT DISTINCT workspace_root FROM files")?;
            let v = stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            v
        };
        let mut pruned = 0usize;
        let tx = self.conn.unchecked_transaction()?;
        for root in &roots {
            if Path::new(root).exists() {
                continue;
            }
            tx.execute(
                "DELETE FROM fts_chunks WHERE workspace_root = ?1",
                params![root],
            )?;
            tx.execute("DELETE FROM files WHERE workspace_root = ?1", params![root])?;
            pruned += 1;
        }
        tx.commit()?;
        // A dropped root can invalidate cached ANN indexes keyed by that root.
        if pruned > 0 {
            self.ann.borrow_mut().clear();
            self.ann_order.borrow_mut().clear();
        }
        Ok(pruned)
    }
}

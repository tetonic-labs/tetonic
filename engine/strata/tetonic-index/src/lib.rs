//! lokai-index ? local code intelligence (structural + keyword + semantic).

mod host;
mod parse;
mod query;
mod schema;
mod semantic;
pub mod skeleton;
mod types;
mod util;
mod watcher;

pub use host::{FilesystemCodeIndex, IndexTextSkeleton};

pub use skeleton::skeletonize;
pub use types::{
    Hit, IndexError, IndexStats, IndexStatus, Lang, OutlineRow, PendingChunk, Result, SymbolRow,
    MAX_CHUNK_CHARS, MAX_FILE_BYTES, MAX_SIG_CHARS,
};
pub use watcher::{watch_index_blocking, IndexWatcher};

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use semantic::AnnIndex;

/// Minimum indexed files before code retrieval is considered healthy for a workspace.
pub const INDEX_HEALTHY_MIN_FILES: i64 = 10;

/// Stable workspace/file key for SQLite (strip Windows `\\?\` verbatim prefix).
pub fn workspace_storage_key(path: impl AsRef<Path>) -> String {
    schema::storage_path_key(&path.as_ref().to_string_lossy())
}

/// The local code index. Owns a single SQLite connection to `index.db`.
///
/// **Not [`Sync`]** ? one `Connection` and an in-memory ANN cache. Share across
/// threads only behind a [`std::sync::Mutex`] (see [`IndexWatcher`]). The agent
/// tool loop uses a per-thread cache of `Rc<Index>` on the main thread.
pub struct Index {
    pub(crate) conn: Connection,
    pub(crate) path: PathBuf,
    pub(crate) ann: RefCell<HashMap<String, AnnIndex>>,
    /// FIFO order of ANN cache keys for bounded eviction.
    pub(crate) ann_order: RefCell<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use semantic::{is_config_path, is_test_chunk, rerank_score};

    #[test]
    fn workspace_storage_key_strips_windows_verbatim_prefix() {
        #[cfg(windows)]
        {
            assert_eq!(workspace_storage_key(r"\\?\C:\lokai-ws"), r"C:\lokai-ws");
        }
        #[cfg(not(windows))]
        {
            let key = workspace_storage_key(r"\\?\C:\lokai-ws");
            assert!(key.contains("lokai-ws"), "{key}");
        }
    }

    #[test]
    fn indexes_and_queries_rust_and_python() {
        let dir = std::env::temp_dir().join(format!("lokai-index-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("calc.rs"),
            "pub struct Calc;\nimpl Calc {\n    pub fn add(a: i32, b: i32) -> i32 { a + b }\n}\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("greet.py"),
            "import os\n\nclass Greeter:\n    def greet(self, name):\n        return f\"hi {name}\"\n",
        )
        .unwrap();

        // index.db lives outside the workspace (as it does in real use).
        let db_path =
            std::env::temp_dir().join(format!("lokai-index-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let idx = Index::open(&db_path).unwrap();
        let ws = dir.to_string_lossy().to_string();

        let first = idx.index_workspace(&dir).unwrap();
        assert!(first.indexed >= 2, "indexed both source files");
        assert!(
            first.symbols >= 4,
            "Calc, add (method), Greeter, greet (method)"
        );

        // Re-index is incremental: nothing changed.
        let second = idx.index_workspace(&dir).unwrap();
        assert_eq!(second.indexed, 0);
        assert!(second.unchanged >= 2);

        // Structural lookups.
        let calc = idx.find_definition(&ws, "Calc").unwrap();
        assert_eq!(calc.len(), 1);
        assert_eq!(calc[0].kind, "struct");

        let add = idx.find_definition(&ws, "add").unwrap();
        assert_eq!(add[0].kind, "method");

        let greeter = idx.find_definition(&ws, "Greeter").unwrap();
        assert_eq!(greeter[0].kind, "class");

        // Outline nests the method under the class.
        let outline = idx.outline(&ws, "greet.py").unwrap();
        let greet = outline.iter().find(|o| o.name == "greet").unwrap();
        assert_eq!(greet.depth, 1);

        // Keyword search finds the python file by an identifier.
        let hits = idx.search(&ws, "Greeter", 10).unwrap();
        assert!(hits.iter().any(|h| h.rel == "greet.py"));

        // Edit a file -> only that one re-indexes.
        std::fs::write(
            dir.join("calc.rs"),
            "pub struct Calc;\nimpl Calc {\n    pub fn add(a: i32, b: i32) -> i32 { a + b }\n    pub fn sub(a: i32, b: i32) -> i32 { a - b }\n}\n",
        )
        .unwrap();
        let third = idx.index_workspace(&dir).unwrap();
        assert_eq!(third.indexed, 1);
        assert_eq!(idx.find_definition(&ws, "sub").unwrap().len(), 1);

        let status = idx.status(&ws).unwrap();
        assert_eq!(status.files, 2);

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn fts_queries_with_operators_dont_error() {
        let dir = std::env::temp_dir().join(format!("lokai-fts-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("a.py"),
            "def alpha():\n    return 'beta or gamma'\n",
        )
        .unwrap();
        let db = std::env::temp_dir().join(format!("lokai-fts-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db);
        let idx = Index::open(&db).unwrap();
        let ws = dir.to_string_lossy().to_string();
        idx.index_workspace(&dir).unwrap();

        // None of these adversarial inputs may error (they used to: `fts5: syntax error`).
        for q in [
            "foo OR",
            "alpha OR beta",
            "NEAR(a b)",
            "a AND (b",
            "\"unterminated",
            "rel:lib.rs",
            "*",
            "^",
            "a* b*",
            "   ",
            "OR AND NOT NEAR",
            "col:\"x\" OR 1=1",
        ] {
            let r = idx.search(&ws, q, 10);
            assert!(r.is_ok(), "query {q:?} should not error, got {r:?}");
        }
        // And a real token still finds the file.
        assert!(idx
            .search(&ws, "alpha", 10)
            .unwrap()
            .iter()
            .any(|h| h.rel == "a.py"));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn rerank_demotes_tests_and_config_below_real_code() {
        // A test fn with a *higher* raw cosine should still lose to a real impl
        // method once the rerank penalties/boosts apply.
        let test_fn = rerank_score(
            0.62,
            "atmos/lokai-egress/src/lib.rs",
            "enrolled_node_is_allowed",
            "function",
            "fn enrolled_node_is_allowed() { assert!(g.authorize(\"10.0.0.5\").is_ok()); }",
        );
        let real_fn = rerank_score(
            0.50,
            "atmos/lokai-egress/src/lib.rs",
            "authorize",
            "method",
            "fn authorize(&self, host: &str) -> Result<IpAddr, EgressError> { /* real logic */ }",
        );
        assert!(
            real_fn > test_fn,
            "impl method should outrank the test ({real_fn} vs {test_fn})"
        );

        // A Cargo.toml chunk that name-drops a dependency should sink below code.
        let toml = rerank_score(
            0.63,
            "strata/lokai-index/Cargo.toml",
            "",
            "file",
            "tree-sitter = ...",
        );
        let code = rerank_score(
            0.55,
            "strata/lokai-index/src/lib.rs",
            "extract",
            "function",
            "let mut parser = Parser::new();",
        );
        assert!(
            code > toml,
            "real code should outrank Cargo.toml ({code} vs {toml})"
        );

        assert!(is_config_path("a/b/Cargo.toml"));
        assert!(!is_config_path("a/b/lib.rs"));
        assert!(is_test_chunk("x/tests/y.rs", "", ""));
        assert!(is_test_chunk("x/lib.rs", "test_thing", ""));
        assert!(!is_test_chunk("x/lib.rs", "authorize", "let x = 1;"));
    }

    #[test]
    fn stores_and_ranks_embeddings_by_cosine() {
        let dir = std::env::temp_dir().join(format!("lokai-vec-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.py"), "def alpha():\n    return 1\n").unwrap();
        std::fs::write(dir.join("b.py"), "def beta():\n    return 2\n").unwrap();

        let db_path =
            std::env::temp_dir().join(format!("lokai-vec-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let idx = Index::open(&db_path).unwrap();
        let ws = dir.to_string_lossy().to_string();
        idx.index_workspace(&dir).unwrap();

        let model = "test-embed";
        // Everything is pending before we embed; nothing after.
        let pending = idx.pending_embeddings(&ws, model, 100).unwrap();
        assert!(pending.len() >= 2, "two chunks await embedding");

        // Assign deterministic 2-D vectors so the ranking is checkable: the chunk
        // whose content mentions "alpha" points east, "beta" points north.
        for p in &pending {
            let v = if p.content.contains("alpha") {
                [1.0f32, 0.0]
            } else {
                [0.0f32, 1.0]
            };
            idx.store_embedding(p.chunk_id, model, &v).unwrap();
        }
        assert!(idx.pending_embeddings(&ws, model, 100).unwrap().is_empty());

        let (embedded, total) = idx.embedding_status(&ws, model).unwrap();
        assert_eq!(embedded, total);

        // A query pointing east should rank the "alpha" chunk first.
        let hits = idx.semantic_search(&ws, model, &[1.0, 0.0], 5).unwrap();
        assert_eq!(hits[0].rel, "a.py");
        assert!(hits[0].score > 0.99);

        // Re-indexing an edited file drops its stale vectors (FK cascade).
        std::fs::write(dir.join("a.py"), "def alpha():\n    return 42\n").unwrap();
        idx.index_workspace(&dir).unwrap();
        let pending = idx.pending_embeddings(&ws, model, 100).unwrap();
        assert_eq!(pending.len(), 1, "only the edited file needs re-embedding");

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn ann_path_agrees_with_brute_force_top_hit() {
        // Build a small embedded corpus, then call the ANN path *directly*
        // (bypassing the size threshold) and confirm its top hit matches the
        // exact brute-force result. Guards the ANN wiring (build, query,
        // shortlist rerank, cache signature) without needing >4000 vectors.
        let dir = std::env::temp_dir().join(format!("lokai-ann-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("east.py"), "def east():\n    return 1\n").unwrap();
        std::fs::write(dir.join("north.py"), "def north():\n    return 2\n").unwrap();
        std::fs::write(dir.join("diag.py"), "def diag():\n    return 3\n").unwrap();

        let db_path =
            std::env::temp_dir().join(format!("lokai-ann-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let idx = Index::open(&db_path).unwrap();
        let ws = dir.to_string_lossy().to_string();
        idx.index_workspace(&dir).unwrap();

        let model = "test-embed";
        for p in &idx.pending_embeddings(&ws, model, 100).unwrap() {
            let v: [f32; 2] = if p.content.contains("east") {
                [1.0, 0.0]
            } else if p.content.contains("north") {
                [0.0, 1.0]
            } else {
                [
                    std::f32::consts::FRAC_1_SQRT_2,
                    std::f32::consts::FRAC_1_SQRT_2,
                ]
            };
            idx.store_embedding(p.chunk_id, model, &v).unwrap();
        }

        let q = [1.0f32, 0.0];
        let sig = idx.ann_signature(&ws, model).unwrap();
        let ann_hits = idx.semantic_search_ann(&ws, model, &q, 3, sig).unwrap();
        let brute_hits = idx.semantic_search_brute(&ws, model, &q, 3).unwrap();
        assert_eq!(ann_hits[0].rel, "east.py");
        assert_eq!(ann_hits[0].rel, brute_hits[0].rel);

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&db_path);
    }

    /// Windows canonicalize can store `\\?\` paths; a later index pass must not
    /// hit UNIQUE on `files.path` when the alias drifts.
    #[test]
    fn reindex_survives_windows_path_alias() {
        if !cfg!(windows) {
            return;
        }
        let dir = std::env::temp_dir().join(format!("lokai-alias-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("lib.rs"), "pub fn hello() -> i32 { 42 }\n").unwrap();

        let db_path =
            std::env::temp_dir().join(format!("lokai-alias-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let idx = Index::open(&db_path).unwrap();

        let abs = dir.join("lib.rs");
        let abs_verbatim = format!(r"\\?\{}", abs.to_string_lossy());
        let ws_verbatim = format!(r"\\?\{}", dir.to_string_lossy());
        idx.conn
            .execute(
                "INSERT INTO files(path, workspace_root, rel, content_hash, mtime, lang, size, indexed_at, embed_state)\n\
                 VALUES (?1, ?2, 'lib.rs', 'deadbeef', NULL, 'rust', 1, '2020-01-01', 'none')",
                params![abs_verbatim, ws_verbatim],
            )
            .unwrap();

        let stats = idx.index_workspace(&dir).unwrap();
        assert!(stats.indexed >= 1, "re-index should replace aliased row");

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn index_paths_only_touches_listed_files() {
        let dir = std::env::temp_dir().join(format!("lokai-paths-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..20 {
            std::fs::write(
                dir.join(format!("f{i}.rs")),
                format!("pub fn f{i}() {{}}\n"),
            )
            .unwrap();
        }
        let db_path =
            std::env::temp_dir().join(format!("lokai-paths-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let idx = Index::open(&db_path).unwrap();
        let ws = dir.to_string_lossy().to_string();

        idx.index_workspace(&dir).unwrap();
        std::fs::write(dir.join("f0.rs"), "pub fn f0() { 1 }\n").unwrap();

        let stats = idx.index_paths(&dir, &[dir.join("f0.rs")]).unwrap();
        assert_eq!(stats.seen, 1, "only one path visited");
        assert_eq!(stats.indexed, 1);

        let _ = std::fs::remove_file(dir.join("f1.rs"));
        let del = idx.index_paths(&dir, &[dir.join("f1.rs")]).unwrap();
        assert_eq!(del.deleted, 1);
        assert_eq!(idx.status(&ws).unwrap().files, 19);

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn known_files_query_is_workspace_scoped() {
        let dir1 = std::env::temp_dir().join(format!("lokai-ws1-{}", std::process::id()));
        let dir2 = std::env::temp_dir().join(format!("lokai-ws2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir1);
        let _ = std::fs::remove_dir_all(&dir2);
        std::fs::create_dir_all(&dir1).unwrap();
        std::fs::create_dir_all(&dir2).unwrap();
        std::fs::write(dir1.join("a.rs"), "pub fn a() {}\n").unwrap();
        std::fs::write(dir2.join("b.rs"), "pub fn b() {}\n").unwrap();

        let db_path =
            std::env::temp_dir().join(format!("lokai-ws-scope-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let idx = Index::open(&db_path).unwrap();
        idx.index_workspace(&dir1).unwrap();
        idx.index_workspace(&dir2).unwrap();

        std::fs::write(dir2.join("b.rs"), "pub fn b() { 1 }\n").unwrap();
        let stats = idx.index_paths(&dir2, &[dir2.join("b.rs")]).unwrap();
        assert_eq!(stats.seen, 1);
        assert_eq!(stats.indexed, 1);

        let _ = std::fs::remove_dir_all(&dir1);
        let _ = std::fs::remove_dir_all(&dir2);
        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn indexes_typescript_and_javascript() {
        let dir = std::env::temp_dir().join(format!("lokai-ts-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("app.ts"),
            "export class App {\n  greet(name: string): string {\n    return `hi ${name}`;\n  }\n}\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("util.js"),
            "export function sum(a, b) {\n  return a + b;\n}\n",
        )
        .unwrap();

        let db_path = std::env::temp_dir().join(format!("lokai-ts-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let idx = Index::open(&db_path).unwrap();
        let ws = dir.to_string_lossy().to_string();
        idx.index_workspace(&dir).unwrap();

        let app = idx.find_definition(&ws, "App").unwrap();
        assert_eq!(app.len(), 1);
        assert_eq!(app[0].kind, "class");

        let sum = idx.find_definition(&ws, "sum").unwrap();
        assert_eq!(sum.len(), 1);
        assert_eq!(sum[0].kind, "function");

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn respects_gitignore() {
        let dir = std::env::temp_dir().join(format!("lokai-gitignore-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".gitignore"), "ignored/\n").unwrap();
        std::fs::create_dir_all(dir.join("ignored")).unwrap();
        std::fs::write(dir.join("ignored/hidden.rs"), "pub fn hidden() {}\n").unwrap();
        std::fs::write(dir.join("visible.rs"), "pub fn visible() {}\n").unwrap();

        let db_path =
            std::env::temp_dir().join(format!("lokai-gitignore-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let idx = Index::open(&db_path).unwrap();
        let ws = dir.to_string_lossy().to_string();
        idx.index_workspace(&dir).unwrap();

        assert_eq!(idx.find_definition(&ws, "visible").unwrap().len(), 1);
        assert!(idx.find_definition(&ws, "hidden").unwrap().is_empty());
        assert_eq!(idx.status(&ws).unwrap().files, 1);

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn skips_oversized_files() {
        use crate::types::MAX_FILE_BYTES;

        let dir = std::env::temp_dir().join(format!("lokai-big-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let big = "x".repeat((MAX_FILE_BYTES + 1024) as usize);
        std::fs::write(dir.join("big.txt"), big).unwrap();
        std::fs::write(dir.join("small.rs"), "pub fn small() {}\n").unwrap();

        let db_path =
            std::env::temp_dir().join(format!("lokai-big-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let idx = Index::open(&db_path).unwrap();
        let ws = dir.to_string_lossy().to_string();
        let stats = idx.index_workspace(&dir).unwrap();
        assert!(stats.skipped >= 1);
        assert_eq!(idx.status(&ws).unwrap().files, 1);

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn embedding_queries_match_aliased_workspace_root() {
        let dir = std::env::temp_dir().join(format!("lokai-embed-alias-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.py"), "def alpha():\n    return 1\n").unwrap();

        let db_path =
            std::env::temp_dir().join(format!("lokai-embed-alias-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let idx = Index::open(&db_path).unwrap();
        let ws_stored = if cfg!(windows) {
            format!(r"\\?\{}", dir.to_string_lossy())
        } else {
            format!("alias-{}", dir.to_string_lossy())
        };
        let ws_query = dir.to_string_lossy().to_string();
        idx.index_workspace(&dir).unwrap();
        idx.conn
            .execute(
                "UPDATE files SET workspace_root = ?1 WHERE workspace_root = ?2",
                params![ws_stored, ws_query],
            )
            .unwrap();

        let model = "alias-model";
        let pending = idx.pending_embeddings(&ws_query, model, 100).unwrap();
        assert_eq!(
            pending.len(),
            1,
            "pending_embeddings must honor aliased roots"
        );
        idx.store_embedding(pending[0].chunk_id, model, &[1.0, 0.0])
            .unwrap();
        let (emb, total) = idx.embedding_status(&ws_query, model).unwrap();
        assert_eq!(emb, total);

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn find_mentions_excludes_definition_chunk() {
        let dir = std::env::temp_dir().join(format!("lokai-mentions-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("a.rs"),
            "pub fn target() {}\nfn caller() { target(); }\n",
        )
        .unwrap();

        let db_path =
            std::env::temp_dir().join(format!("lokai-mentions-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let idx = Index::open(&db_path).unwrap();
        let ws = dir.to_string_lossy().to_string();
        idx.index_workspace(&dir).unwrap();

        let hits = idx.find_mentions(&ws, "target", 10).unwrap();
        assert!(
            hits.iter().any(|h| h.preview.contains("caller")),
            "should find mention in caller"
        );
        assert!(
            !hits.iter().any(|h| h.symbol_name == "target"),
            "definition chunk should be excluded"
        );

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn find_definition_prefers_production_over_tests() {
        let dir = std::env::temp_dir().join(format!("lokai-def-rank-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("tests")).unwrap();
        std::fs::write(dir.join("src/run.rs"), "pub fn run() { 1 }\n").unwrap();
        std::fs::write(
            dir.join("tests/run_test.rs"),
            "fn run() { assert!(true); }\n",
        )
        .unwrap();

        let db_path =
            std::env::temp_dir().join(format!("lokai-def-rank-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let idx = Index::open(&db_path).unwrap();
        let ws = dir.to_string_lossy().to_string();
        idx.index_workspace(&dir).unwrap();

        let defs = idx.find_definition(&ws, "run").unwrap();
        assert!(defs.len() >= 2);
        assert!(
            defs[0].rel.replace('\\', "/").contains("src/run.rs"),
            "production definition should rank first, got {}",
            defs[0].rel
        );

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn classification_policy_bump_invalidates_cached_embeddings() {
        use tetonic_domain::CLASSIFICATION_POLICY_VERSION;

        let dir = std::env::temp_dir().join(format!("lokai-class-policy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.rs"), "fn main() {}\n").unwrap();

        let db_path =
            std::env::temp_dir().join(format!("lokai-class-policy-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let idx = Index::open(&db_path).unwrap();
        let ws = dir.to_string_lossy().to_string();
        idx.index_workspace(&dir).unwrap();
        let model = "test-embed";
        let pending = idx.pending_embeddings(&ws, model, 100).unwrap();
        assert!(!pending.is_empty());
        idx.store_embedding(pending[0].chunk_id, model, &[1.0, 0.0])
            .unwrap();
        let (emb_before, _) = idx.embedding_status(&ws, model).unwrap();
        assert_eq!(emb_before, 1);

        // Simulate an older index built under a prior policy version.
        idx.conn
            .execute(
                "UPDATE index_metadata SET value = '0' WHERE key = 'classification_policy_version'",
                [],
            )
            .unwrap();
        let invalidated = idx
            .sync_classification_policy_version(CLASSIFICATION_POLICY_VERSION)
            .unwrap();
        assert!(invalidated);
        let (emb_after, _) = idx.embedding_status(&ws, model).unwrap();
        assert_eq!(emb_after, 0);

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&db_path);
    }
}

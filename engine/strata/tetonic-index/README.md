# lokai-index

Local code intelligence: tree-sitter symbols (Rust, Python, TypeScript, JavaScript), FTS5 keyword search, optional embeddings + HNSW semantic search. Disposable `index.db`.

## Role in the stack

Built by CLI (`lokai --index`) or auto at daemon init. When present, `lokai-tools` exposes `find_definition`, `search_code`, `outline`, `find_mentions` (`find_references` is a deprecated alias). The orchestrator briefing reads index stats.

## Key API

- `Index::open`, `index_workspace`, `index_paths`, `status`
- Query: `find_definition`, `find_mentions`, `outline`, `search`, `semantic_search`
- Embeddings: store/query vectors per symbol chunk
- `IndexWatcher` — debounced filesystem re-index via `index_paths` (AR1-3); serializes DB access with `Mutex<Index>`

## Threading

`Index` is **not `Sync`**. The watcher holds `Arc<Mutex<Index>>`; the agent tool loop uses a per-thread `Rc<Index>` cache on the main thread.

## Semantic search scope

`semantic_search` is available from **`tetonic-cli`** (`--search --semantic`) and benches — **not** an agent tool in v1 (sync tool loop; embeddings are async/CLI-driven). Corpora ≤ 4000 vectors use exact cosine; larger workspaces build an in-memory HNSW index on demand (LRU cache, max 8 graphs) and rebuild when embeddings change.

## Dependencies

None (foundation crate). Used by `lokai-tools`, `lokai-orchestrator`, bins.

## Product plan

| ID | Feature | Status |
|----|---------|--------|
| X1–X5 | Symbol graph, FTS, embeddings, ANN, rerank | Done |
| X8 / D6 | Live index watcher | Done (path-level incremental, AR1-3) |

## Tests

`cargo test -p lokai-index` — Rust/Python/TS/JS indexing, FTS safety, rerank, ANN vs brute force, gitignore, watcher integration, workspace alias handling, Windows `\\?\` key stripping (H4-1).

## Related docs

- [code-index-v1](../../../docs/implementation/contracts/code-index-v1.md)

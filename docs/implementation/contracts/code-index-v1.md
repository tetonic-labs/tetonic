# Code index & retrieval — v1 (clean-slate design)

**Status:** **Implemented (v1 core).** All three layers ship: the **structural** symbol graph (tree-sitter, Rust + Python), the **keyword** FTS5/BM25 index, and the **semantic** layer (local embeddings + vector search), in `engine/crates/lokai-index` over the disposable `index.db` (schema v2), driven incrementally by content hash and queryable from `lokai-cli` (`--index`, `--embed`, `--index-status`, `--def`, `--refs`, `--outline`, `--search [--semantic]`). Retrieval is **wired into the agent loop as tools** (`find_definition`, `search_code`, `outline`, `find_references`); **`semantic_search` is CLI/bench-only in v1** (not an agent tool — embedding is orchestrated outside the sync tool loop). The **live watcher** (D6 / AR1-3) debounces notify events and calls `index_paths` (single-file incremental) instead of re-walking the whole tree; full `index_workspace` remains the CLI cold-path and directory-change fallback.
**Owner:** `engine/crates/lokai-index` (Rust).
**Relates to:** `performance-and-scale.md` §4.1; `pivot-agentic-code-editor.md` §3.3/§10; consumed by `lokai-core` via `lokai-tools`; embeddings obey `egress-guard-v1`; storage is the disposable `index.db` from `memory-store-v2`.

> **Implementation status (2026-06-27).** Built: `index.db` (files/symbols/imports/chunks + `fts_chunks` FTS5 + `vec_chunks` for embeddings, schema **v2**); tree-sitter extraction for **Rust** and **Python** (functions/methods/structs/enums/traits/impls/classes/consts/…), with all other text files keyword-indexed as a whole-file chunk; **incremental** re-index via a deterministic content hash (+ deletion sweep); `find_definition` / `find_references` (best-effort keyword) / `outline` / `search` (BM25 with `snippet()` previews) / `semantic_search` (cosine) / `status`, all **workspace-scoped** in a shared `index.db`. **Embeddings** are produced by the local runtime via `OllamaProvider::embed` (`/api/embed`) **through the egress guard** — they never leave the machine — and stored as little-endian f32 BLOBs keyed by `(chunk_id, model_id)`; a content change cascades the stale vector away (re-embed on next `--embed`); a model change re-embeds everything. **Agent retrieval tools** (`find_definition`/`search_code`/`outline`/`find_references`) are advertised only when an index is wired in (`Tools::with_index`), so the model sees them only when usable. Verified on this repo's `engine/`: 16 files / 281 symbols / 290 chunks indexed in ~150 ms, all 290 embedded with `nomic-embed-text`; a natural-language semantic query ("block outbound connections unless local") top-ranks `EgressGuard` while keyword search returns nothing; a live agent run chose `find_definition` as its first tool. Divergences from the sketch below: `impl` blocks are recorded as symbols (for outline) but excluded from `find_definition`; `files` carries `workspace_root` + `rel` so one `index.db` can hold multiple projects; the default embedder is the **egress-guarded Ollama embedder**, not in-process ONNX (see the `Embedder` table note); vectors use **brute-force cosine**, not `sqlite-vec`, at this scale; embedding is orchestrated by the composition root (CLI) feeding vectors into sync index storage, so `lokai-index` keeps **zero network surface**. **Reranker (added after first eval):** a real eval found cosine alone top-ranking unit tests (over `authorize`) and `Cargo.toml` (over the tree-sitter parser); a heuristic rerank (test/config penalties + symbol-kind boost, unit-tested) fixed both — `authorize` and the `extract` parser now surface in the top-3 — without re-embedding (query-time only). A capability preflight now rejects non-tool models (e.g. `phi3:mini`) before a session is opened.

## Why this exists

A cloud agent leans on a dedicated indexing/embedding/retrieval service so it can find the right code cheaply and keep the model's context tight. We must provide the same capability **locally**, on abundant CPU/RAM/disk, so the scarce GPU only ever runs the coder model. This is the layer that lets a *weak local model* punch above its weight: it never sees the whole repo, only the **few highest-value chunks** for the task.

Three properties are non-negotiable:
1. **Incremental from v1** — re-index only what changed (per-file content hash + mtime). Full re-index on every open is the thing that makes local tools feel terrible.
2. **Cheapest-layer-first** — exact structural lookups (free) before keyword (cheap) before embeddings (expensive). Most "search" an agent does is really structural.
3. **Local-only** — no cloud embedding API, no hosted vector DB; embeddings run on owned compute behind the Egress Guard.

## The three layers (cheapest-first)

| Layer | Mechanism | Cost | Answers |
|---|---|---|---|
| **Structural** | tree-sitter → symbol graph | free (no LLM, no vectors) | "where is `X` defined / who calls it / outline of this file" |
| **Keyword** | SQLite **FTS5** (BM25) | cheap | identifier / literal / phrase search |
| **Semantic** | embeddings + **`sqlite-vec`** | expensive (embed cost) | fuzzy intent: "where do we handle auth" |

Retrieval is **hybrid**: candidates from all available layers are merged and **reranked**; semantic is used last and only when structural+keyword are insufficient. On a freshly-opened repo the semantic layer may be cold — the index serves **structural-only** until background embedding catches up, and says so.

## Storage (`index.db` — disposable, rebuildable)

Lives in the disposable `index.db` (separate from the precious `lokai.db`; see `memory-store-v2`). Safe to delete to reclaim space or recover from corruption — it rebuilds from source.

```
files(path PK, content_hash, mtime, lang, size, indexed_at, embed_state)
symbols(id PK, file_path FK, kind, name, signature, start_line, end_line, parent_id)
  -- kind: function|method|struct|class|trait|impl|enum|const|module|...
refs(id PK, file_path FK, symbol_name, line)        -- call/use sites (best-effort)
imports(id PK, file_path FK, target, line)
fts_chunks                                          -- FTS5 virtual table (BM25 keyword)
vec_chunks(chunk_id FK, model_id, dim, vec BLOB, created_at)  -- embeddings (brute-force cosine today; sqlite-vec later)
chunks(id PK, file_path FK, start_line, end_line, kind, content_hash)
```

> **As built (v2):** `vec_chunks` is a plain table (one f32-BLOB row per `(chunk_id, model_id)`) with an FK to `chunks(id) ON DELETE CASCADE`, searched by brute-force cosine — fine at local-repo scale and dependency-free/portable. Moving to `sqlite-vec`/HNSW is a behind-the-query swap when a repo gets large enough to need it. The `refs` table isn't materialized yet — `find_references` is served from the keyword index for now.

- `files.content_hash` + `mtime` drive incrementality; `embed_state` (`none|stale|fresh`) lets retrieval know whether the semantic layer is usable for that file.
- `chunks` are **symbol-aware** (a function/class is a chunk), not blind fixed-size windows — retrieval returns meaningful units and the line ranges to read.
- `index.db` pragmas mirror `memory-store`: WAL, `synchronous=NORMAL`, `busy_timeout`. Index writes batch in transactions.

## Incremental indexing

```
file event (notify crate)  ─▶  hash(content)
        │
        ├─ hash == stored      ─▶  skip (no work)
        └─ hash changed/new    ─▶  re-parse (tree-sitter) ─▶ upsert symbols/refs/imports/chunks
                                   └─ mark embed_state = stale ─▶ background embed queue
deleted file               ─▶  cascade delete its rows
```

- A **debounced file-watcher** (`notify`) feeds a work queue; only changed files are touched.
- **Background, prioritized:** open + recently-edited files first; the rest trickle in. Never blocks a user action.
- **Bounded:** large/binary/generated files (and `.gitignore`d paths) are skipped via the same `ignore` crate the tools use.

## The `Retriever` trait (what `lokai-core` depends on)

`lokai-core` depends on this narrow trait, never on FTS/vectors/tree-sitter directly — so ranking can improve without touching agent code (the discipline that made `InferenceProvider` pay off).

```rust
#[async_trait]
pub trait Retriever: Send + Sync {
    /// Hybrid retrieve: structural + keyword + (if warm) semantic, merged & reranked.
    async fn retrieve(&self, q: &Query, budget: RetrieveBudget) -> Result<Vec<Hit>, IndexError>;

    /// Exact structural lookups (free; no LLM, no vectors).
    fn find_definition(&self, name: &str) -> Result<Vec<SymbolRef>, IndexError>;
    fn find_references(&self, name: &str) -> Result<Vec<SymbolRef>, IndexError>;
    fn outline(&self, path: &str) -> Result<Vec<SymbolRef>, IndexError>;

    /// Freshness so the ContextBuilder/Inspector can be honest about coverage.
    fn status(&self) -> IndexStatus; // files indexed, embed coverage %, queue depth
}

pub struct Query { pub text: String, pub intent: Intent } // Intent: Definition|Usage|Semantic|Mixed
pub struct RetrieveBudget { pub max_hits: u32, pub max_tokens: u32 } // retrieval respects the token budget
pub struct Hit { pub path: String, pub start_line: u32, pub end_line: u32,
                 pub score: f32, pub source: HitSource, pub preview: String }
pub enum HitSource { Symbol, Keyword, Semantic }
```

`retrieve` returns **line-ranged hits with a preview**, not file dumps — the `ContextBuilder` decides what to actually inline under its token budget, and the agent can `read_file` the exact range for more.

## The `Embedder` trait (pluggable, capacity-aware)

```rust
#[async_trait]
pub trait Embedder: Send + Sync {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, IndexError>;
    fn dim(&self) -> usize;
    fn id(&self) -> &str; // model id — stored so a model change invalidates vectors
}
```

| Impl | Default? | Where it runs | Notes |
|---|---|---|---|
| Ollama embedder (`OllamaProvider::embed`) | **yes (as built)** | local/enrolled runtime, **egress-guarded** | Current default: reuses the runtime we already require, keeps embeddings behind the one sanctioned socket owner. Default model `nomic-embed-text`. |
| `CpuOnnxEmbedder` | planned | CPU (`fastembed`/ONNX, e.g. `bge-small`) | Deferred: `fastembed` fetches model files out-of-band, which would bypass the Egress Guard. Lands once we ship/point at bundled local ONNX weights so it stays in-process with no network. |
| `GpuOllamaEmbedder` (tiered) | planned | enrolled embed-tier node | Fabric-policy variant of the Ollama embedder for multi-node setups. |

> **As built:** rather than a formal `Embedder` trait inside `lokai-index` (which would drag a network dependency into the network-free index crate), embedding is orchestrated by the **composition root** (`lokai-cli`): it pulls `pending_embeddings`, calls the egress-guarded provider, and writes vectors back via the sync `store_embedding`. The trait lands when retrieval moves into `lokai-core`. The `model_id` is recorded per vector; **changing the embedding model re-embeds everything** (a query filters by `model_id`, so old-model vectors are simply ignored), and a content change cascades the stale vector away. Selection will become policy-driven from the fabric snapshot (`performance-and-scale` §5.1).

## Privacy & boundary guarantees

- Embeddings are **local-only**: the as-built Ollama embedder uses the **egress-guarded** client (verified: every embed request logs `decision=Allow reason=builtin loopback`), so it can only reach `{ localhost, enrolled nodes }`; the planned `CpuOnnxEmbedder` will be in-process (no network at all). `lokai-index` itself has **no network dependency** by construction.
- `index.db` holds **derived project content** (symbols, chunk text, vectors) — it stays on-disk, is covered by the same purge/encryption story as `memory-store`, and is **never transmitted**.
- No cloud embedding API and no hosted vector DB exist as options (Charter / `INV-1`).

## Error & degradation model

```rust
pub enum IndexError { NotReady, ParseFailed { path: String }, EmbedFailed(String), Db(String) }
```

- The index is **best-effort**: if it is unavailable or cold, `lokai-core` falls back to the live tools (`grep`/`glob`/`read_file`) — the agent still works, just with less help. Index failure **never** blocks the user.
- `status()` lets the Context Inspector show coverage honestly ("semantic index 60% warm").

## Compatibility & extensibility

- Additive-only within v1: new `symbols.kind`s, new `HitSource`s, new `Intent`s; consumers tolerate unknowns.
- Swapping the vector store (e.g. `sqlite-vec` → HNSW) is an implementation change behind `Retriever`, not a contract change.
- Candidate later capabilities (out of v1): cross-file type resolution, an LSP bridge for precise refs, call-graph queries, per-repo learned conventions.

## Mission alignment

| Principle | How honored |
|---|---|
| Capable | Cheapest-first hybrid retrieval lets a weak local model see only the highest-value code — punching above its weight. |
| Private by architecture | Embeddings local-only; derived content never leaves the box; no cloud index. |
| Sovereignty | Your repo is *understood but never uploaded*; the index is owned, purgeable, rebuildable. |
| Durable | A narrow `Retriever`/`Embedder` surface insulates the agent from churn in the embedding/vector ecosystem. |

---

**Last updated:** 2026-06-27 (v1 core implemented: structural + keyword + semantic layers, egress-guarded local embeddings, and agent-facing retrieval tools).

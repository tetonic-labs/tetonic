# lokai-bench

Repeatable **CPU** micro-benchmark of hot paths — no LLM, no network.

For end-to-end **agentic** graded tasks (live model), see [../../bench/README.md](../../bench/README.md).

## Purpose

Measure indexing, retrieval, grep, semantic search, and tokenizer throughput on a synthetic repo. Used for local perf tuning; D10 (bench CI gate) is deferred.

## Usage

```bash
cargo run -p lokai-bench --release -- [num_files] [embed_dim]
```

Defaults: 1500 files, 768-dim embeddings.

## What it measures

- Cold / warm / incremental index build
- `find_definition`, `search`, `outline`
- Parallel grep
- Embedding store + brute-force semantic search
- Heuristic tokenizer throughput

## Dependencies

- `lokai-index`, `lokai-tools`, `lokai-core`

## Tests

None — benchmark binary only. Results may be saved under `engine/bench/results_*.json`.

## Related docs

- [performance-and-scale.md](../../../docs/implementation/performance-and-scale.md)

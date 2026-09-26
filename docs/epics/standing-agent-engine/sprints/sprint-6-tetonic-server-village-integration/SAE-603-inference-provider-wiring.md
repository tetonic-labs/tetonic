# SAE-603 — Inference Provider Wiring

| Field        | Value                                              |
|--------------|----------------------------------------------------|
| **Ticket**   | SAE-603                                            |
| **Sprint**   | Sprint 6 — Tetonic Server + Village Integration    |
| **Epic**     | Standing Agent Engine                              |
| **Type**     | Feature                                            |
| **Priority** | P0 — Critical Path                                 |
| **Estimate** | 3 pts                                              |
| **Depends**  | SAE-601, SAE-403 (Inference Routing)               |

---

## Objective

Wire `OllamaProvider` from `tetonic-inference` into `tetonic-server` so the agent's brain has a real inference backend. The previous Village runner bypassed the engine and called Ollama directly via HTTP fetch — this ticket replaces that with the proper engine path.

## Acceptance Criteria

- [ ] `tetonic-server` initializes an `OllamaProvider` from `tetonic-inference` on startup.
  - Configurable Ollama base URL via `--ollama-url` (default `http://127.0.0.1:11434`).
  - Configurable model tag via `--model`.
- [ ] Provider is wrapped in `Arc<dyn InferenceProvider>`.
- [ ] `SingleModelBrain` from `tetonic-runtime` is constructed using the provider.
  - Context window size (`num_ctx`) configurable via `--num-ctx` (default: `8192`).
- [ ] Model prewarming: on startup, issue a single warmup inference call (or use `OllamaProvider`'s built-in prewarm if available) to ensure the model is loaded before the agent loop starts.
- [ ] Startup log confirms: `inference ready | model={model} | provider=ollama | url={url}`.
- [ ] If Ollama is unreachable at startup, log an error and exit with a clear message (don't silently hang).

## Implementation Notes

### Dependencies to add to `tetonic-server/Cargo.toml`
```toml
tetonic-inference = { path = "../../atmos/tetonic-inference" }
tetonic-runtime = { path = "../../core/tetonic-runtime" }
```

### Initialization Flow

```rust
// 1. Create OllamaProvider
let provider = Arc::new(
    OllamaProvider::new(&args.ollama_url, &args.model)
);

// 2. Verify connectivity
provider.health_check().await?;

// 3. Create SingleModelBrain
let brain = Arc::new(SingleModelBrain::new(
    provider.clone(),
    &args.model,
    args.num_ctx,
));

info!(model = %args.model, "inference ready");
```

### Qwen3.5 Compatibility Notes (from prior session)
- `format: 'json'` returns empty responses from `qwen3.5:latest` — do NOT use forced JSON mode.
- `qwen3.5` emits `<think>...</think>` blocks — the brain or a post-processing layer should strip these before JSON parsing.
- Inference latency is ~20-30s for first call (model loading), ~2-5s thereafter.

## Out of Scope

- Dual-process brain (System 1 + System 2) — no fast model available yet
- Remote/pooled inference routing — standalone mode only
- Model auto-selection or multi-model support

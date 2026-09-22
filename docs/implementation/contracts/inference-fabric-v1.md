# Inference fabric — v1 (clean-slate design)

**Status:** **Partially implemented** — local `OllamaProvider`, `PooledProvider`
(enrolled workers + failover), async `fabric_snapshot()`, NDJSON streaming on
`/v1/chat` (AR1-2), and M5-3 worker trust/placement with transport-boundary
revalidation. Legacy chat is represented as a typed `JobEnvelope`; immediately
before send the coordinator rechecks trust/policy epoch, fresh capabilities,
model digest, workspace version, and artifact-store digests. Uncached workers
fail closed. Typed embed/compute/artifact-bearing remote transports do not exist
yet and are not claimed as shipped. Tier routing via `model_tier` on sessions;
`ModelWant`/`resolve_model()`
remain future work.
**Owner:** `engine/crates/lokai-inference` (Rust).
**Relates to:** `pivot-agentic-code-editor.md` §4.3; all inference traffic egresses through `egress-guard-v1`; tool schemas from `coding-tools-v1`.

## Why this exists

The product must run unchanged on a single 24GB workstation **and** on a self-built cluster the user grows over time — "fully utilizing the available system architecture" without ever re-architecting the agent. That is only possible if the engine never names a host, a port, or a runtime. It depends on **one narrow trait**; everything about scheduling, placement, and scale-out lives behind it.

This contract also pins a privacy property: **every implementation obtains its network client from `lokai-egress`** and therefore can only reach `localhost` + enrolled, user-owned nodes. The fabric is the most likely place a "just call the cloud" shortcut could sneak in; the trait makes that structurally impossible.

## The trait

```rust
#[async_trait]
pub trait InferenceProvider: Send + Sync {
    /// Stream a chat completion. Tool schemas + format/grammar travel in the request.
    async fn chat(&self, req: ChatRequest) -> Result<ChatStream, InferenceError>;

    /// Current capacity of the fabric (drives scheduling + scaling policy).
    fn capabilities(&self) -> FabricSnapshot;

    /// Map a logical model request to a concrete model on a concrete node.
    fn resolve_model(&self, want: ModelWant) -> Result<ModelPlacement, InferenceError>;

    /// Optional embedding path (local only); same egress + boundary rules.
    async fn embed(&self, req: EmbedRequest) -> Result<EmbedResponse, InferenceError>;
}
```

The engine (`lokai-core`) holds a `dyn InferenceProvider`. It never knows whether that is one Ollama or a 12-node cluster.

### `ChatRequest`

```rust
pub struct ChatRequest {
    pub model: ModelWant,          // tier or explicit name
    pub messages: Vec<Message>,
    pub tools: Option<ToolSchemas>,// coding-tools-v1 schemas
    pub format: Option<JsonSchema>,// constrained-decoding grammar
    pub options: SampleOptions,    // temperature, top_p, seed, num_ctx, ...
    pub hint: ResourceHint,        // est_prompt_tokens, est_output_tokens, priority
}

pub enum ModelWant {
    Tier(ModelTier),               // Router | Coder | Critic | Embed (logical roles)
    Named(String),                 // explicit, e.g. "qwen3-coder:30b"
}

pub struct ResourceHint {
    pub est_prompt_tokens: u32,
    pub est_output_tokens: u32,
    pub priority: Priority,        // Interactive | Background
}
```

`ModelTier` is the key indirection: the engine asks for a **role** (the cheap fast `Router`, the strong `Coder`, the `Critic`), and the fabric resolves each to whatever model/node is best given current capacity. On a single node they may all map to one model; on a cluster they spread across GPUs.

### `FabricSnapshot`

```rust
pub struct FabricSnapshot {
    pub nodes: Vec<NodeInfo>,
    pub effective_concurrency: u32, // max agents that can infer in parallel right now
    pub generated_at: DateTime,
}

pub struct NodeInfo {
    pub id: String,                 // WorkerEnrollment.id (see node-enrollment-v1 / estate-v1)
    pub label: String,              // user-facing
    pub vram_total_mb: u32,
    pub vram_free_mb: u32,
    pub resident_models: Vec<String>,
    pub queue_depth: u32,
    pub healthy: bool,
}
```

`effective_concurrency` is what lets a cluster be *used*: the swarm executor (`lokai-core`) runs at most this many agents/specialists in parallel, so independent branches of a swarm fan out across nodes instead of serializing.

> **Honest single-node value.** On one GPU, `effective_concurrency ≈ 1` — agents serialize at the inference layer, so a single-node swarm is *sequential specialists*, not parallel speed. Multi-agent **quality** (specialization, critique) is available on one box; multi-agent **speed** is a **cluster** property. The router therefore defaults to a single agent and escalates deliberately (`performance-and-scale` §4.3–4.4).

### `ModelPlacement`

```rust
pub struct ModelPlacement {
    pub node_id: String,
    pub model: String,             // concrete resolved model
    pub resident: bool,            // already loaded? (affinity hint)
}
```

## Implementations (same trait, growing capacity)

| Provider | Phase | Behavior |
|---|---|---|
| `LocalOllamaProvider` | **A** | One node, native `/api/chat` + `format` grammar via the egress-guarded client. `fabric_snapshot()` probes `/api/tags` + `/api/ps`. |
| `PooledProvider` | **F / shipped** | Wraps loopback Ollama + enrolled workers (`RemoteNodeProvider`). Placement by model tier, turn affinity, `gates_ok`, queue depth; failover on busy/preempt/timeout. **Snapshot TTL cache** (default 3s, `LOKAI_FABRIC_SNAPSHOT_TTL_SECS`) amortizes worker probes across agent steps (AR1-2). |
| `ClusterRuntimeProvider` | **F** | Delegates to a distributed serving runtime (**vLLM** tensor/pipeline-parallel, **Ray Serve**, or **llama.cpp RPC**) for models too big for one GPU. We *consume* the runtime's API; we do not implement parallelism. Appears to the engine as one logical large-capacity node. |

A deployment may compose these (e.g. a `PooledProvider` whose members include a `ClusterRuntimeProvider` for the 70B+ tier and plain Ollama nodes for the router tier).

## Scheduling & scaling policy

`capabilities()` feeds a policy that maps capacity → behavior:

- **Single constrained node (the 24 GB default)** ⇒ **minimize resident models**: Router / Coder / Critic resolve to **one resident model** (same weights, different system prompt + sampling). Loading a second model evicts the first (multi-second stalls), so tier-splitting is a *capacity privilege*, not the default. The default `Coder` is sized to leave VRAM headroom for KV cache + CPU-side embeddings (`performance-and-scale` §3).
- **More VRAM on a node** ⇒ allow a larger default `Coder` tier model (auto-upgrade only when `FabricSnapshot` reports the headroom).
- **More nodes** ⇒ raise `effective_concurrency` (more parallel agents) and pin hot models per node (affinity).
- **Mixed hardware** ⇒ the `Router`/`Critic` tiers land on small/fast nodes; the `Coder` tier on the strongest GPU; a dedicated **`Embed` tier** node may serve GPU embeddings. A homelab keeps a **model zoo** hot, pinned per node.
- **Interconnect awareness (at scale)** ⇒ once work spans nodes, the LAN is far slower than within-box memory, so placement is **data-locality-aware**: prefer the node already holding the relevant KV/context over the marginally-less-loaded one; avoid shipping large prompts across links. (At the floor this is moot; on a homelab it becomes a primary scheduling input — see `performance-and-scale` §2.)
- **Backpressure** ⇒ when all nodes are saturated, `Background`-priority requests queue while `Interactive` (the user's active chat) preempts.

The **effort policy** (hop/critic budgets, swarm fan-out) becomes **capacity-aware**: a single laptop runs a tight serial loop; a cluster runs a wider, deeper swarm in the same wall-clock time.

### KV-cache prefix reuse (the biggest inference win)

The largest single speedup on local hardware is **not** re-prefilling a prompt the runtime has already seen. Providers therefore treat each `ChatRequest` as **`[stable prefix] + [volatile suffix]`**:

- **Stable prefix** — system prompt + tool schemas + pinned repo facts — kept **byte-identical** across turns so llama.cpp/Ollama prompt cache (and vLLM automatic prefix caching) can skip re-prefilling thousands of tokens.
- **Volatile suffix** — retrieved chunks, the latest turn, timestamps — appended after; never allowed to leak into the prefix.

`lokai-core` is responsible for constructing messages in this order; the model session is kept **warm** (`keep_alive`) and the prefix **pre-warmed** on session start. Providers should preserve message order and avoid reordering that would invalidate the cache.

### Embedding placement (`embed`)

`embed()` is **pluggable and capacity-aware** (`performance-and-scale` §5.1): on a single constrained node it defaults to a **CPU/ONNX embedder** (off the GPU critical path so it never evicts the coder model); GPU embeddings are used only when the snapshot shows headroom or an `Embed`-tier node exists. Either way embeddings are local-only and egress-guarded.

## Privacy & boundary guarantees

- Every provider's HTTP client comes from `lokai-egress`; reachable destinations are exactly `{ localhost, enrolled nodes }`.
- A node only appears in `FabricSnapshot` after **enrollment** (`node-enrollment-v1`), which is the same action that adds its egress allow rule. There is no way to point the fabric at a non-enrolled host.
- **Remote nodes** are reached via `fabric-transport-v1` over mTLS to a worker's **ingress fabric listener** — not raw Ollama on a LAN port.
- No model I/O, prompt, or embedding ever transits a non-owned endpoint. Embeddings are local (`embed` hits the same local/enrolled runtimes).

### Remote placement (homelab + Circle)

- **`RemoteNodeProvider`** implements `chat()` by sending `FabricJob` to `/v1/chat` on an enrolled worker; see [`circle-implementation-plan.md`](../circle-implementation-plan.md) §1.1.
- **Turn affinity:** steps of the same user turn prefer the same peer while healthy (prefix stability + connection reuse).
- **Local preference:** coordinator places on local GPU when tier and latency SLO are met before using a remote node.

## Error model

```rust
pub enum InferenceError {
    NoCapableModel { tier: ModelTier }, // no resident/available model for the role
    AllNodesBusy,                       // backpressure; caller may queue/retry
    NodeUnreachable { node_id: String },// egress-allowed but down → failover
    EgressDenied { host: String },      // a non-enrolled host was attempted (should never happen)
    RuntimeError(String),
}
```

- `NodeUnreachable` triggers failover in `PooledProvider`; the run continues if another node can serve the tier.
- `EgressDenied` is a *contract violation alarm* — it means something tried to reach outside the boundary; it is logged loudly and surfaced.

## Implementation status (2026-06-28)

| Surface | Shipped | Notes |
|---------|---------|-------|
| `chat()` + streaming | ✅ | Local Ollama streams tokens; remote fabric streams NDJSON (`event: token\|done`) when `Accept: application/x-ndjson` or `options.stream` (AR1-2) |
| `embed()` | ✅ local `/api/embed` | Remote via fabric (future) |
| `fabric_snapshot()` → `FabricSnapshot` | ✅ | Async; local + remote merge in `PooledProvider`; **TTL cache** between agent steps |
| `ModelWant` / `resolve_model()` | ❌ | Session `model_tier` + profile models used instead |
| `PooledProvider` / failover | ✅ | N0.4 + AR1-1 timeouts + AR1-2 cache; **failover redaction** (SEC2-E2-028); `prompt_redacted` on provenance |
| `ActiveJobRegistry` | ✅ | Session-scoped jobs; `cancel_session` on RPC cancel; auto `finish_job` via guard; cap 512 |
| Worker capacity on fabric | ✅ | `/v1/capacity/status` on workers; merged into `NodeInfo.capacity` |
| `fabric/status` RPC | ✅ | Forces snapshot refresh (invalidates TTL cache) |

**Context on the wire:** Remote fabric jobs carry **trimmed** step context from `build_context()` (system prefix + recent messages + tool results), not the full in-memory transcript. Compaction state stays on the coordinator.

**Code today:** `lokai-inference` exports fabric types in `fabric.rs`; `build_compute_plane` in `lokaid` swaps local-only vs pooled. Job ids are `job_{ms}_{seq}` (AR1-2). Legacy single-json `/v1/chat` responses remain supported.

## Compatibility & extensibility

- The trait is the stable surface. New providers are **purely additive** (Phase F adds two without touching `lokai-core`).
- `ChatRequest`, `FabricSnapshot`, etc. are additive-only within v1; consumers tolerate unknown fields.
- New `ModelTier` roles (e.g. a dedicated `Embed` or future `Vision` tier) may be added; providers map unknown tiers to a sensible default.

## Mission alignment

| Principle | How honored |
|---|---|
| Sovereignty / scalable | One workstation → owned cluster with zero engine changes; the user's hardware is fully utilized. |
| Private by architecture | All inference is egress-guarded and boundary-locked; cloud shortcuts are structurally impossible. |
| Capable | Tiered model roles + capacity-aware concurrency let a swarm exploit whatever compute exists. |
| Durable | A narrow trait insulates the product from churn in the local-inference runtime landscape. |

---

**Last updated:** 2026-08-11 (M5-3 transport-boundary typed placement and state revalidation).

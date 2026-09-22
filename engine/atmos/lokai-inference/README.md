# lokai-inference

Inference providers: local Ollama, remote worker nodes, pooled placement, and an opt-in hosted Chat Completions adapter. All production HTTP egress goes through `lokai-egress`; fabric transport uses pinned mTLS.

## Hosted inference mechanics

`hosted::HostedChatProvider` implements the existing `InferenceProvider` contract.
It supports buffered text completions, ordered function-tool conversations, optional
JSON schema output, token usage, and hosted provenance. It does not enroll a fabric
worker, execute tools, or change local defaults. See [hosted integration](HOSTED.md)
for construction, ownership, and current limits.

## Role in the stack

### Local startup warm-up

CLI and daemon bootstrap overlap an empty local model load with the remaining
startup work. `OllamaProvider::prewarm_with_context` includes the configured
context size and a 30-minute residency request. It does not prefill the agent's
system prompt or guarantee that a model stays resident under memory pressure.
Only the selected startup model is warmed, avoiding loading every catalog entry.

Warm-up failures propagate and are logged by bootstrap. Successful requests are
debounced for 60 seconds per provider instance using model, context size, and
residency settings; concurrent warm-ups are serialized. Failure or cancellation
does not leave a successful cache entry. The generic `prewarm` API still works
without an explicit context setting.

Debug tracing records warm-up elapsed time and, for local chat completion,
`load_ms`, `prompt_eval_ms`, and `eval_ms`. These distinguish loading from first
prompt processing and token generation. Startup model discovery uses a single
list request for reachability and inventory. Repository indexing still completes
before startup returns; it has not been moved to an unowned background task.

The daemon and CLI hold `Arc<dyn InferenceProvider>`. Swapping local-only Ollama for a `PooledProvider` (coordinator + enrolled workers) is the single compute-plane extension point.

## Modules

| Module | Purpose |
|--------|---------|
| `lib.rs` | `OllamaProvider`, chat/embed types |
| `fabric.rs` | Job/snapshot/receipt types |
| `attempt.rs` | Active job/attempt registry (AC2-7), session cancel, bounded size |
| `compute_registry.rs` | Authorized compute targets (AC2-8) |
| `placement_engine.rs` | M5-3 trust/capability placement for chat and typed jobs |
| `worker_eligibility.rs` | Dispatch-time trust, freshness, revocation, and model checks |
| `dispatch.rs` | Full-payload classification, local redaction/rescan, common job guard |
| `pooled.rs` | Local + remote routing, turn affinity, failover redaction |
| `remote.rs` | mTLS worker node provider |
| `fabric_client.rs`, `owner_activity.rs` | Worker push + owner preemption |
| `tls_handshake.rs` | TLS 1.3 handshake signature verification (SEC-001) |

## Key API

- `InferenceProvider::chat`, `fabric_snapshot`
- `ChatRequest`, `ChatResponse`, `Message`, `ToolCall`, `GenUsage`, `InferenceProvenance`
- `ActiveJobRegistry` — attempt lifecycle plus dispatched-input tracking for post-dispatch trust incidents; `begin_or_bind_attempt` honors a coordinator-leased `fabric.attempt_id` and does not mint a competing id
- `PooledProvider::chat` — uses `req.fabric.attempt_id` when present; remote `chat_on_fabric` round-trips that id
- `PooledProvider::cancel_session_jobs` — wired from RPC `session/cancel`
- `ComputeTargetRegistry`, `AuthorizedComputeTarget`
- `PlacementRequest`, `evaluate_placement`, `placement_request_from_chat`
- `placement_request_from_job`, `evaluate_typed_job_placement`
- `NETWORK_DENY_ALL_CAPABILITY` — Infer jobs with tools require enforced sandbox network denial
- `PooledProvider`, `RemoteNodeProvider`
- `FabricSnapshot`, `NodeInfo` (`models_verified` gates placement when model list empty)
- `build_tool_call_format` — structured tool-call JSON for D7

## Threading & lifecycle

- `PooledProvider::chat` registers each step in `ActiveJobRegistry` and always removes the job on exit (`JobFinishGuard`).
- Session cancel drops in-flight fabric jobs for that session id.
- Registry caps at 512 entries with FIFO eviction of oldest jobs.

## Dependencies

- `lokai-egress` — all outbound HTTP; fabric TCP only after `ensure_allowed`
- `lokai-enroll` — worker TLS identity

## Product plan

| ID | Feature |
|----|---------|
| I1 | `InferenceProvider` + `OllamaProvider` |
| D7 | `ChatRequest.response_format` / Ollama `format` |
| N0 | Fabric job/snapshot/receipt types |
| N1.2 | Turn affinity in `FabricCallMeta` |
| D13 | Pooled homelab fabric (shipped) |
| M5-3 | Worker trust, fail-closed capabilities, typed transport placement, retry/queue invalidation, and post-dispatch incident reporting (Done) |

## Tests

`cargo test -p lokai-inference` — 83 tests covering fabric serde, registry
lifecycle, TLS handshake, pooled placement/failover, leased attempt-id bind on
remote chat, full-payload classification, trust downgrade, uncached/expired
capabilities, redaction/rescan, post-dispatch incident bindings, legacy chat,
typed embed/compute placement, model digests, and verification requirements.

## Related docs

- [inference-fabric-v1](../../../docs/implementation/contracts/inference-fabric-v1.md)
- [fabric-transport-v1](../../../docs/implementation/contracts/fabric-transport-v1.md)

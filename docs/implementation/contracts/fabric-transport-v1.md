# Fabric transport — v1

**Status:** Draft (N0.0 gate — write before N0.1 code).  
**Owner:** `engine/crates/lokai-inference` (client) + `engine/crates/lokai-node` (server).  
**Relates to:** [`inference-fabric-v1.md`](./inference-fabric-v1.md), [`node-enrollment-v1.md`](./node-enrollment-v1.md), [`ingress-guard-v1.md`](./ingress-guard-v1.md), [`estate-v1.md`](./estate-v1.md), [`PRODUCT-PLAN.md`](../../PRODUCT-PLAN.md) (L1–L6, V1–V6).

## Why this exists

Remote inference crosses an enrolled link. This contract defines the **wire format** between coordinator (egress client) and worker (ingress server): jobs, results, receipts, health, and policy push — all over **mTLS**, never raw Ollama on the LAN.

## Transport

- **Protocol:** HTTPS (TLS 1.3) with **mutual authentication** and pinned peer keys from enrollment.
- **Path prefix:** `/v1/` on the worker **fabric listener** only (see `ingress-guard-v1` Fabric mode).
- **Streaming:** `POST /v1/chat` uses NDJSON or SSE chunks mirroring local Ollama streaming shape enough for `RemoteNodeProvider` to reuse assembly logic.
- **Redirects:** disabled on client (egress guard).

## Core types

### `FabricJob` (coordinator → worker)

```rust
pub struct FabricJob {
    pub job_id: String,              // stable; links receipt + audit
    pub attempt_id: Option<String>,  // coordinator attempt; worker echoes on result (AC2-7)
    pub estate_id: String,           // OwnerIdentity id
    pub session_id: Option<String>,  // coordinator session when owner job
    pub agent_id: String,
    pub step_index: u32,
    pub model: String,
    pub tier: Option<String>,        // ModelTier name when used
    pub messages: Vec<Message>,      // slice needed for this forward pass
    pub tools: Vec<ToolSchema>,      // stable prefix may repeat; see turn affinity
    pub options: SampleOptions,
    pub priority: JobPriority,       // OwnerInteractive | OwnerBackground | Circle*
    pub data_class: DataClass,       // private | personal | circle_ok (D1)
    pub disclosure_tier: DisclosureTier,
    pub audit_envelope: Option<AuditEnvelope>, // when tier >= summary/auditable
    pub circle_id: Option<String>,
    pub consumer_peer_id: Option<String>,
    pub policy_epoch: u64,           // worker snapshot epoch
    pub turn_affinity: Option<String>, // prefer same worker for turn
}
```

### `FabricJobResult` (worker → coordinator, trailer)

```rust
pub struct FabricJobResult {
    pub job_id: String,
    pub attempt_id: Option<String>, // must match active coordinator attempt when set
    pub message: Message,            // final assistant message + tool_calls
    pub usage: GenUsage,
    pub status: JobStatus,           // ok | error | canceled | preempted
    pub error: Option<String>,
}
```

### `ComputeReceipt` (settled record — both sides)

Metadata only on the default receipt; optional link to `audit_envelope_id`. See [`PRODUCT-PLAN.md`](../../PRODUCT-PLAN.md) § Compute receipt ledger.

| Field | Notes |
|-------|--------|
| `job_id`, `direction`, `kind` | `local` \| `estate` \| `circle_out` \| `circle_in` |
| `disclosure_tier` | For consent audit |
| `prompt_tokens`, `eval_tokens`, `duration_ms` | From runtime |
| `status`, timestamps | Append-only |

Coordinator persists to `fabric_ledger` (`memory-store-v2`); worker to `worker_ledger` in **`worker.db`** (LD4).

**L7:** Local loopback steps write `kind: local` receipts — every settled inference step, no sampling v1.

### `AuditEnvelope` (optional)

See [`estate-v1.md`](./estate-v1.md) § Audit keys. Ciphertext sealed to worker **audit public key** when `disclosure_tier >= auditable`.

## HTTP routes (fabric listener)

| Method | Path | Auth | Purpose |
|--------|------|------|---------|
| POST | `/v1/chat` | mTLS + ingress authorize | Run one inference job (stream) |
| GET | `/v1/health` | mTLS | Liveness + queue depth |
| GET | `/v1/capabilities` | mTLS | Models, VRAM, resident weights, `worker_capabilities` (incl. sandbox) |
| GET | `/v1/models/verified` | mTLS | Ollama `/api/tags` only (`{ ok, models: [{ name, digest? }] }`) for coordinator probes (R7-2) |
| POST | `/v1/negotiate` | mTLS | Version + feature negotiation (R7-3); see [`fabric-compatibility-matrix.md`](./fabric-compatibility-matrix.md) |
| POST | `/v1/estate/policy` | mTLS owner channel | **V2 out of scope** — returns `501 not_implemented`. Workers are policy-passive; coordinator is sole enforcement (placement, revoke, Infer redaction). |
| GET | `/v1/ledger` | mTLS owner channel | **V2 out of scope** — returns `501 not_implemented` (reserved surface). |
| POST | `/v1/revoke` | mTLS | Revocation push (best-effort) |

Circle peer jobs use the same `/v1/chat` with `CirclePeerGrant` authorization at ingress (N2).

## Error codes (HTTP + body)

| Code | Meaning |
|------|---------|
| 401 | mTLS / peer not authorized |
| 403 | Policy: disclosure mismatch, cap exceeded, data class denied |
| 409 | Stale `policy_epoch` |
| 503 | Worker saturated / owner preempting |
| 504 | Job timeout |

Body: `{ "error": "...", "code": "disclosure_required" | "cap_exceeded" | ... }`.

## Turn affinity & context shipping (N1.2)

- Coordinator sends the **messages slice required for this step** (stable prefix + volatile suffix per `inference-fabric-v1`).
- **`FabricCallMeta`** on `ChatRequest` carries `session_id`, `agent_id`, `step_index`, and `turn_id` (one per `chat/send`).
- **`PooledProvider`** resets placement affinity when `turn_id` changes; within a turn, `turn_affinity` on `FabricJob` prefers the worker used on step *k* for steps *k+1…* while healthy.
- **Tool schemas** repeat in each job payload in v1 (intentional — worker needs them for the forward pass). Prefix stability in `lokai-core` avoids churning system prompt mid-turn.
- **Combined mode** (`lokaid --combined`): coordinator stdio + fabric listener share one process; local `chat/send` signals in-process `WorkerScheduler` (no network hop for owner activity).
- No cross-node KV migration in v1; Ollama `keep_alive` on the same peer amortizes prefix when affinity holds.
- Bench: [`engine/bench/remote_fabric.py`](../engine/bench/remote_fabric.py) — wire bytes/step estimate + local tok/s baseline.

## Privacy

- Tool schemas may be large; document in audit receipts, not duplicated in gossip.
- Workers default **no prompt persistence**; audit envelope is opt-in per disclosure tier.
- All bytes cross enrolled mTLS only.

## Implementation phases

| Phase | Scope |
|-------|--------|
| N0.0 | Types + serde tests; no listener |
| N0.4 | `/v1/chat`, `/v1/health`, `/v1/capabilities` for homelab |
| N1.2 | Turn affinity + `FabricCallMeta`; combined mode owner signal |
| N0.4+ | `fabric_ledger` / `worker_ledger` persist |
| N2.3 | `disclosure_tier`, audit envelope, circle job fields |
| N2.2 | Not on this transport — gossip is separate mesh |

---

**Last updated:** 2026-06-27

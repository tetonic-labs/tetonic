# lokai-node

Worker fabric ingress — the only crate that binds fabric listen ports. mTLS, default-deny accept, job scheduler, revocation.

## Role in the stack

Runs on worker machines via `lokaid --node`. Accepts inference jobs from the coordinator over mutually authenticated TLS. Scheduler preempts Circle work when owner activity is signaled (N1.1).

## Modules

| Module | Purpose |
|--------|---------|
| `server.rs` | Fabric HTTP listener, connection limits |
| `conn.rs` | Shared mTLS connection handler + post-route ingress audit |
| `fabric.rs` | `/v1/*` routes, body limits, chat/jobs/revoke |
| `fabric_chat.rs` | `/v1/chat`, typed `/v1/jobs`, `/v1/jobs/cancel`, `/v1/jobs/lease` |
| `lease_table.rs` | Soft leases + expiry cancel (R7-1 heartbeat) |
| `scheduler.rs` | Owner vs Circle priority, preemption, per-job cancel |
| `tls.rs`, `trust.rs` | mTLS, cert pinning, live revoke, per-pin estate binding |
| `event.rs` | Ingress audit (memory + `worker.db`) |
| `limits.rs` | Max body size, max connections, log retention |
| `revoke.rs`, `bind.rs` | Revoke push client, listen bind policy |

## Key API

- `FabricServer`, `FabricListenConfig`, `resolve_listen_host`, `DEFAULT_BIND_HOST`
- `WorkerScheduler` — `signal_owner_activity`, `cancel_job`, `cancel_running`
- `TrustStore`, `push_revoke`
- `MAX_FABRIC_BODY_BYTES`, `MAX_FABRIC_CONNECTIONS`
- `DEFAULT_FABRIC_PORT` (9471) in `lokai-enroll`; enrollment default port 9470 lives in `lokai-enroll`

## R7-1 typed Infer

Default capability advertisement is `WorkerCapabilities::typed_infer_profile`
(`legacy_v1_chat_only: false`, cancel/lease/heartbeat claimed). Typed routes:
`POST /v1/jobs`, `/v1/jobs/cancel`, `/v1/jobs/lease`.
Missed renewals expire the soft lease and cancel the running job.

## R7-2 capability inventory

`GET /v1/capabilities` fills `worker_capabilities.sandbox` from
`lokai_sandbox::platform_backend().capabilities()` (bool → Enforced/Unsupported).
`GET /v1/models/verified` returns only what Ollama `/api/tags` lists
(`{ ok, models: [{ name, digest? }] }`) for coordinator probe comparison.

## R7-3 version negotiation

`POST /v1/negotiate` runs `negotiate_versions` against
`MIN_SUPPORTED_VERSION`/`MAX_SUPPORTED_VERSION` and immutable security features.
Skew or invalid JSON fails closed with a typed fabric error. Compatibility
matrix: `docs/implementation/contracts/fabric-compatibility-matrix.md`.

## Deferred routes (V2 out of scope)

- `POST /v1/estate/policy` — `501 not_implemented`. Workers do not enforce owner estate policy locally; the coordinator is the sole policy enforcement point (pre-dispatch placement, revoke, Infer redaction).
- `GET /v1/ledger` — `501 not_implemented` (reserved; not part of V2).

## Dependencies

- `lokai-enroll`, `lokai-inference`, `lokai-memory`, `lokai-egress`, `lokai-capacity`, `lokai-sandbox`

## Product plan

| ID | Feature |
|----|---------|
| N0.2 | Fabric listener / ingress |
| N0.3 | Revocation |
| N1.1 | Scheduler / preemption |
| R7-1 | Typed fabric Infer (**Done**) |
| R7-2 | Live capability inventory + verified models (**Done**) |

## Tests

`cargo test -p lokai-node` — scheduler (incl. `cancel_job`), TLS, fabric ingress auth, policy deny audit, mock chat, DB persistence.

## Related docs

- [ingress-guard-v1](../../../docs/implementation/contracts/ingress-guard-v1.md)
- [fabric-transport-v1](../../../docs/implementation/contracts/fabric-transport-v1.md)

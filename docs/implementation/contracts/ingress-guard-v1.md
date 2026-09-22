# Ingress Guard — v1

**Status:** Draft (N0.0 gate — write before N0.2 code).  
**Owner:** `engine/crates/lokai-node` (planned crate).  
**Relates to:** [`egress-guard-v1.md`](./egress-guard-v1.md) (symmetric outbound), [`node-enrollment-v1.md`](./node-enrollment-v1.md), [`fabric-transport-v1.md`](./fabric-transport-v1.md), [`estate-v1.md`](./estate-v1.md), [`circle-v1.md`](./circle-v1.md).

## Why this exists

Egress Guard protects **outbound** connections. Workers must accept **inbound** fabric connections without exposing inference to the open LAN. Ingress Guard is the symmetric chokepoint: **default-deny accept** until mTLS + authorization succeed.

> **CI rule:** Only `lokai-node` may `TcpListener::bind` for fabric/enrollment ports (coordinator stdio RPC never binds a network listener).

## Two listener modes

| Mode | When | Accepts | Closes |
|------|------|---------|--------|
| **Enrollment** | `lokaid --node --enroll` (TTL e.g. 15 min) | PAKE handshake only — **no** `/v1/chat` | After success or TTL |
| **Fabric** | Worker serving (`--node` + contribute / homelab serve) | mTLS + `/v1/*` fabric API only | When serving off or process exit |

Pre-enrollment workers **never** expose the inference API.

## Authorization pipeline (fabric mode)

```
TCP accept → TLS handshake → peer_id from client cert
  → TrustStore::authorize_ingress(peer_id, FabricJob headers)
  → route handler
```

| Job source | Ingress checks |
|------------|----------------|
| **Owner** (estate) | Client cert maps to pinned `OwnerIdentity` / coordinator key; owner priority queues |
| **Circle** (N2+) | `CirclePeerGrant` + circle epoch + contribution snapshot + disclosure tier + data class |

Homelab N0: owner path only. Circle grants additive on same listener.

## `IngressEvent` (audit)

Mirrors `EgressEvent` shape for the Network Activity Panel / privacy gate:

| Field | Notes |
|-------|--------|
| `ts`, `peer_id`, `remote_addr` | |
| `decision` | `allow` \| `deny` |
| `reason` | e.g. `unknown_cert`, `disclosure_mismatch`, `cap_exceeded` |
| `route` | `/v1/chat`, etc. |
| `job_id` | When applicable |

Persisted to worker `ingress_log` (worker DB); coordinator pulls for estate view.

## Bind policy

- Default bind: **not** `0.0.0.0` without explicit opt-in + warning (N4.2).
- Prefer Tailscale/WireGuard underlay; document in deployment guide.
- Rate limits + max body size (N4.3).

## Worker policy snapshot (enforcement)

Signed worker-policy snapshots are **not implemented** and are **not a source of truth**. Ingress does not currently reject circle jobs from a coordinator-signed snapshot type. Do not treat `estate-v1` sketches as live persist.

Failed push / fail-open rules for a future snapshot path are out of scope here.

## Emergency local controls

- `lokaid --node circle-off` — stop circle ingress without coordinator (physical/SSH access).
- Un-enroll / revoke epoch — deny all peers immediately.

## Privacy gate (N0.2)

CI test: unauthenticated probe to fabric port → **deny** + logged (homelab profile, not deferred to N3).

---

**Last updated:** 2026-06-27

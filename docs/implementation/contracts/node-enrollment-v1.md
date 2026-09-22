# Node enrollment — v1 (estate model)

**Status:** Draft (N0.0 — revised for estate; implement in N0.1).  
**Owner:** `lokai-enroll` + `lokai-egress` (allowlist) + `lokai-node` (ingress) + `lokai-memory` (persistence).  
**Relates to:** [`estate-v1.md`](./estate-v1.md), [`egress-guard-v1.md`](./egress-guard-v1.md), [`ingress-guard-v1.md`](./ingress-guard-v1.md), [`inference-fabric-v1.md`](./inference-fabric-v1.md), [`fabric-transport-v1.md`](./fabric-transport-v1.md), Circle **`CirclePeerGrant`** ([`circle-v1.md`](./circle-v1.md)).

## Why this exists

A single workstation never needs remote enrollment. The moment a user adds **their own** GPU server or second box, the software must reach *those* machines — and **only** those. Enrollment is the deliberate, user-driven act that adds a worker to the **estate** and widens the Egress Guard allowlist. It must be explicit, mutually authenticated, and revocable.

The threat it addresses: typing an IP to "add a node" must not enroll a machine the user does not control (typo, MITM, rogue LAN service). Enrollment proves **both** sides share a secret the user established out-of-band.

## Trust model

- The user is the certificate authority for their **estate**. No external PKI, cloud, or account.
- Trust is established by a **pre-shared enrollment secret** moved between machines (copy/paste, QR, USB) — never via a third party.
- After enrollment, coordinator and worker hold **pinned credentials**; subsequent fabric connections require mutual proof (TOFU **gated by shared secret**, not blind TOFU).

## Roles (estate)

| Role | Who |
|------|-----|
| **OwnerIdentity** | Long-lived operator key for the estate (`estate_<hex>`). See [`estate-v1.md`](./estate-v1.md). |
| **Coordinator** | Machine the user drives (editor + `lokaid`). Authors policy; holds egress allowlist + `WorkerEnrollment` records. |
| **Worker** | Headless `lokaid --node` (or combined mode). Exposes fabric API only after enrollment + explicit serve. |

One estate may contain **many workers** (GPU box, embed node, etc.). Homelab v1 may still run **one** worker — the model is N workers from day one.

## Enrollment handshake

```
1. User starts `lokaid --node --enroll` on the worker.
   → worker prints a one-time ENROLLMENT CODE (secret, worker pubkey fingerprint,
     audit pubkey fingerprint, addr, expiry).
2. User runs `lokai estate worker add <code>` on the coordinator (or editor UI).
3. Coordinator connects to worker enrollment listener (HTTPS when the code pins a TLS cert, else plain HTTP on loopback):
   - Coordinator proves knowledge of the pre-shared secret via HMAC proof (`secret` never sent on the wire).
   - Coordinator signs the request with its Ed25519 key (`REQUEST_SIGN_CONTEXT || worker_pk || coordinator_pk || label || secret_proof`).
   - Worker returns pinned worker/audit pubkeys and fabric TLS cert; coordinator **must reject** any field mismatch vs the code.
   - Fabric traffic after enrollment uses **mTLS** separately (`fabric-transport-v1`).
4. On success:
   - coordinator stores WorkerEnrollment under OwnerIdentity.
   - coordinator adds egress AllowRule (UserEnrolled, pinned addr + fabric port).
   - worker pins coordinator pubkey(s) under OwnerIdentity; accepts owner jobs only from pinned keys.
5. One-time code expires (single use); enrollment listener closes.
6. User enables fabric serving on worker when ready (`--node` serve / contribute).
```

Failure (wrong secret, expired code, pubkey mismatch) aborts with **no** allow rule.

## Stored credential

```rust
/// Per-worker enrollment record on the coordinator.
/// Supersedes the older bilateral `NodeCredential` name in docs.
pub struct WorkerEnrollment {
    pub id: String,                    // "worker_<hex>"
    pub estate_id: String,               // OwnerIdentity.id
    pub label: String,                 // "4090-box / vLLM"
    pub addr: SocketAddr,                // pinned address (hostname resolved at enroll)
    pub worker_pubkey: PublicKey,
    pub audit_pubkey: PublicKey,         // for AuditEnvelope (V2+)
    pub fabric_port: u16,
    pub coordinator_pubkeys: Vec<PublicKey>, // v1: typically one; multi-coordinator v1.1
    pub enrolled_at: DateTime,
    pub last_seen: Option<DateTime>,
}
```

Worker side stores symmetric pins: `OwnerIdentity` id, coordinator pubkey(s), revocation epoch.

Credentials live in `lokai-memory` (coordinator) / worker-local store; **never** exported with project snapshots.

> **Legacy alias:** older docs and code may say `NodeCredential` — treat as one `WorkerEnrollment` row; migrate naming in N0.1.

## Threat model — enrollment listener (AR1-4)

The v1 enrollment listener speaks **HTTPS (pinned worker cert) or plain HTTP on loopback** + HMAC proof + Ed25519 request signature on a short-lived TCP port. Fabric traffic after enrollment uses **mTLS** separately (`fabric-transport-v1`).

| Surface | Default | Risk if misconfigured |
|---------|---------|------------------------|
| Enrollment bind | `127.0.0.1` via `resolve_listen_host()` | LAN attacker could attempt handshake guessing if bound to `0.0.0.0` |
| Advertised host in code | `LOKAI_ADVERTISE_HOST` or LAN guess | Coordinator reaches worker; unrelated to listener bind |
| `LOKAI_BIND_ALL=1` | Binds enrollment **and** fabric to all interfaces | Requires firewall; enrollment still refused unless loopback or `LOKAI_ALLOW_LAN_ENROLL=1` |

**Minimum guard (implemented):** `run_enrollment_server` and `lokaid --node --enroll` refuse to start plain HTTP enrollment unless the bind host is loopback, unless the operator sets `LOKAI_ALLOW_LAN_ENROLL=1` on a trusted LAN.

**Homelab checklist when coordinator and worker are on different machines:**

1. Keep enrollment on loopback (`127.0.0.1`) — copy the enrollment code file to the coordinator (USB, SSH, shared folder).
2. Set `LOKAI_ADVERTISE_HOST` to the worker's reachable LAN/tailnet IP for fabric only.
3. Do **not** set `LOKAI_BIND_ALL=1` unless inbound ports are firewalled to coordinator IP only.
4. Prefer tailnet/VPN over exposing enrollment or fabric to the whole LAN.

**Stretch (implemented AR2-1):** TLS on the enrollment listener reusing the worker fabric cert, with the cert pinned in the enrollment code. Plain HTTP remains supported when no TLS cert is embedded (tests only).

## Stored credential — reserved fields

- **`audit_pubkey`:** Captured at enroll for future signed `AuditEnvelope` verification (V2+). Persisted on coordinator and returned in handshake; not validated on every fabric job in v1.
- **`coordinator_pubkeys_json`:** JSON snapshot of coordinator Ed25519 keys at enroll time. Live fabric trust uses pinned TLS client certs in `TrustStore`; this column is audit/metadata only.

## Connection rules (post-enrollment)

- Fabric connections use **mTLS** with pinned peer certs (`fabric-transport-v1`).
- Egress `AllowRule` scoped to **pinned resolved address + fabric port** (resolve-then-pin per `egress-guard-v1`).
- Workers appear in `FabricSnapshot.nodes` by `WorkerEnrollment.id`; unreachable → `healthy: false`, failover in `PooledProvider`.
- Enrollment listener and fabric listener are **separate modes** (`ingress-guard-v1`).

## Revocation

- **Coordinator un-enroll:** delete `WorkerEnrollment`, remove egress rule, drop live connections; best-effort `POST /v1/revoke` to worker.
- **Worker:** revocation **epoch** per coordinator; reject stale epoch on every `FabricJob`.
- **Circle (N2+):** `CirclePeerGrant` revocation is separate from estate un-enroll — see [`circle-v1.md`](./circle-v1.md).

## What crosses an enrolled link

- **Inference traffic** (prompts, model I/O, embeddings) and **fabric control** (health, capabilities, policy push, ledger pull).
- Encrypted mTLS; logged in egress (coordinator) and ingress (worker) audit streams.
- Workers **do not** persist prompts to disk by default; optional audit envelope per disclosure tier.

## Out of scope (v1)

- Automatic discovery (mDNS) — enrollment stays explicit.
- Multi-tenant / family estates (one OS user = one estate).
- Coordinator HA.
- **Circle peer enrollment** — uses invite + `CirclePeerGrant`, not this handshake (N2).

## Compatibility & extensibility

- `WorkerEnrollment` and handshake payloads are additive-only within v1.
- v2 may add rotation, attestation, discovery; core invariant unchanged: **reachable iff user enrolled it under their estate.**

## Mission alignment

| Principle | How honored |
|---|---|
| Sovereignty first | User is their own CA; no external trust anchor. |
| Private by architecture | Only explicit enrollment widens egress; mutually authenticated + pinned. |
| Verifiable | Every enrolled link is a named destination in Network Activity Panel. |
| Durable | Pure-local credentials; offline; no vendor. |

---

**Last updated:** 2026-06-28 (AR1-4 threat model + reserved fields).

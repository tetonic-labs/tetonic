# Estate — v1 (owned compute domain)

**Status:** Draft (N0.0 gate — aligns enrollment + policy before N0.1).  
**Owner:** Coordinator `lokaid` + worker `lokaid --node`; persistence in [`memory-store-v2.md`](./memory-store-v2.md).  
**Relates to:** [`node-enrollment-v1.md`](./node-enrollment-v1.md), [`policy-engine-v1.md`](./policy-engine-v1.md), [`PRODUCT-PLAN.md`](../../PRODUCT-PLAN.md) (E1–E2, trust tiers).

## Why this exists

Users run **many machines** (desktop coordinator, headless GPU servers, embed nodes) under **one operator**. The product must offer **one management plane** for enrollments, resource caps, and circle contribution — without trusting the LAN. An **estate** is that domain.

Circle peers are **not** in your estate; they have their own. Circle is an overlay on resources you explicitly attach.

## Vocabulary

| Term | Meaning |
|------|---------|
| **OwnerIdentity** | Long-lived local operator key (`estate_<hex>`); labels "mine" in audit |
| **Estate** | All `WorkerEnrollment`s + policy under one OwnerIdentity |
| **Coordinator** | Machine running editor + `lokaid` (stdio RPC); authors policy |
| **Worker** | Headless `lokaid --node`; replaceable advertised execution capacity (Infer is the only capability V3 implements remotely, not the meaning of Worker — INV-WORKER-001); enforces ingress snapshot |
| **Combined** | One process: coordinator + local inference + optional fabric ingress |

**Primary homelab pattern:** coordinator on daily driver; **worker-only** on GPU server(s).

## Trust objects

```rust
pub struct OwnerIdentity {
    pub id: String,
    pub label: String,
    pub operator_pubkey: PublicKey,  // signs policy snapshots
    pub created_at: DateTime,
}

pub struct WorkerEnrollment {
    pub id: String,                   // "worker_<hex>"
    pub estate_id: String,
    pub label: String,
    pub addr: HostPort,               // prefer stable hostname
    pub worker_pubkey: PublicKey,
    pub audit_pubkey: PublicKey,      // audit envelope encryption (V2)
    pub fabric_port: u16,
    pub coordinator_pubkeys: Vec<PublicKey>, // v1: one or more pinned coordinators
    pub enrolled_at: DateTime,
    pub last_seen: Option<DateTime>,
}

// Signed worker-policy snapshots are not implemented and are not a source of truth.
```

> **Naming:** `WorkerEnrollment` supersedes the older bilateral **`NodeCredential`** name in docs; same homelab role, extended for N workers and multiple coordinator keys.

## Enrollment flow (summary)

1. Worker: `lokaid --node --enroll` → one-time code (secret, worker keys, addr, TTL).
2. Coordinator: `lokai estate worker add <code>` → PAKE, pin keys, egress allow rule, store `WorkerEnrollment`.
3. Worker pins `OwnerIdentity` / coordinator pubkeys; enrollment listener closes.
4. User starts fabric serving on worker when ready.

See [`node-enrollment-v1.md`](./node-enrollment-v1.md) for handshake detail.

## Policy authority

| Layer | Authoritative store | Worker enforcement |
|-------|---------------------|-------------------|
| Egress allowlist | Coordinator TrustStore | — |
| Placement | Coordinator PooledProvider | — |
| Circle contribution caps | Coordinator writes | **Not implemented** — ingress does not consume a signed worker-policy snapshot (not SoT) |
| Owner preemption | Both | Worker scheduler queues |

Coordinator `POST /v1/estate/policy` snapshot push is **not implemented** and is not a live persist path.

## CLI / RPC (planned)

```
lokai estate status
lokai estate worker add <code>
lokai estate worker set <id> [--min-disclosure ...] [--owner-reserve ...]
lokai estate policy export|import   # v1 split-brain avoidance
lokai estate ledger [--worker ...]
```

RPC additions: `estate/*` namespace (D3); until then CLI talks to local store + worker HTTP.

## Circle overlay

Same worker may hold:

- **Owner** jobs from your coordinator(s)
- **Circle** jobs from peers (via `CirclePeerGrant`, N2)

Scheduler: `Owner*` preempts `Circle*`. Contribution per `(worker, circle)` in snapshot.

## Multi-coordinator (v1.1)

Same estate may pin **multiple coordinator pubkeys** (desktop + laptop). Policy epoch monotonic per worker; export/import `estate.toml` (**ES4**, LD10) avoids split-brain in v1.

## Persistence (LD4)

| Host | Database | Path |
|------|----------|------|
| Coordinator | `lokai.db` | `<data_dir>/lokai/lokai.db` — audit + `fabric_ledger` |
| Worker | `worker.db` | `<data_dir>/lokai/worker.db` — enrollments, `worker_ledger`, ingress log |

Same `lokai-memory` crate; shared ledger table definitions ([`memory-store-v2.md`](./memory-store-v2.md)).

## Out of scope (v1)

- Family/shared estate (one OS user = one estate)
- Coordinator HA / failover
- mDNS auto-discovery

---

**Last updated:** 2026-06-27 (persistence LD4; ES4 export/import)

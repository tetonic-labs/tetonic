# Circle — v1 (protocol sketch)

**Status:** Draft (N2+ gate; gossip and governance detail expand in N2–N4).  
**Owner:** `engine/crates/lokai-circle` (planned).  
**Relates to:** [`circle-charter.md`](../circle-charter.md), [`circle-implementation-plan.md`](../circle-implementation-plan.md), [`estate-v1.md`](./estate-v1.md), [`fabric-transport-v1.md`](./fabric-transport-v1.md), [`ingress-guard-v1.md`](./ingress-guard-v1.md), [`PRODUCT-PLAN.md`](../../PRODUCT-PLAN.md) (V1–V6, L5, C*).

## Why this exists

Circle is **opt-in compute pooling** among trusted peers. Each member runs their own **estate**; Circle adds grants, discovery, disclosure agreements, and gossip — without merging trust domains.

## Trust objects (Circle-specific)

```rust
pub struct Circle {
    pub id: String,
    pub name: String,
    pub governance_pubkey: PublicKey,
    pub created_at: DateTime,
}

pub struct CirclePeerGrant {
    pub circle_id: String,
    pub peer_id: String,              // peer's estate operator id or circle member id
    pub peer_pubkey: PublicKey,
    pub disclosure_tier: DisclosureTier, // bilateral minimum for jobs from this peer
    pub max_concurrent: u32,
    pub gpu_hour_cap: Option<f64>,
    pub epoch: u64,                   // bumps on RemoveMember / revoke
    pub expires_at: Option<DateTime>,
}
```

Homelab uses **`WorkerEnrollment` only**. Circle adds **`CirclePeerGrant`** on workers for inbound peer jobs — same fabric listener, different `authorize_ingress` branch.

## Bilateral disclosure match

Before routing a job to peer P:

1. Consumer declares `disclosure_tier` + `data_class`.
2. Discovery returns only peers whose **advertised minimum** ≤ job tier and who accept the data class.
3. Ingress re-checks grant + epoch + snapshot caps (**fail closed**).

Incompatible resources are **omitted** from `discover_pool_resources` (V6); ingress is backup enforcement.

## Job fields (circle)

See `FabricJob` in [`fabric-transport-v1.md`](./fabric-transport-v1.md): `circle_id`, `consumer_peer_id`, `priority: Circle*`, optional `AuditEnvelope`.

## Gossip (outline — N2.2)

Separate mesh from fabric HTTP:

- Membership announcements, ledger summaries, capability ads (no prompt content).
- Signed with circle governance keys; out of scope for N0.

## Governance events

| Event | Effect |
|-------|--------|
| `InviteMember` | Pending grant |
| `AcceptInvite` | Active grant both sides |
| `RemoveMember` | Epoch bump; delete grants |
| `UpdateContribution` | Worker snapshot push |

## Revocation

Symmetric with estate: epoch on grants; worker `circle-off` local kill switch; coordinator drops outbound circle allow rules.

##### Discovery UX (V7 — locked)

- **Disclosure / data-class mismatch:** resource **omitted** from catalog (not grayed, not listed as "blocked").
- **Budget exhausted** (terms otherwise match): **omitted** by default; optional setting `discovery.show_budget_exhausted` shows one grayed row with reason.
- **Estate fabric** (`fabric/status`): all your workers always visible to your coordinator (no consent filter).

## RPC / CLI (planned N2+)

```
lokai circle create|join|leave|status
lokai circle discover          # optional B12 RPC wrapper; internal API is canonical
lokai circle contribute set ...
lokai circle ledger
```

---

**Last updated:** 2026-06-27

# Policy engine — v1

**Status:** Implemented (D1, 2026-06-27).  
**Owner:** `engine/crates/lokai-policy`.  
**Relates to:** [`estate-v1.md`](./estate-v1.md), [`fabric-transport-v1.md`](./fabric-transport-v1.md), [`circle-v1.md`](./circle-v1.md), [`agent-rpc-v1.md`](./agent-rpc-v1.md).

## Why this exists

Before any job leaves the coordinator — local or remote, owner or circle — the engine must answer: **may this content go to this destination at this disclosure tier?** Policy is centralized so tools, fabric placement, and ingress enforcement share one truth.

## Core checks

```rust
pub fn check_remote_inference(ctx: &PolicyContext, job: &FabricJobDraft) -> PolicyDecision;
pub fn check_tool(ctx: &PolicyContext, tool: &str, args: &Value) -> PolicyDecision;
pub fn check_data_class(class: DataClass, dest: &Destination) -> PolicyDecision;
pub fn evaluate_action(action: &ProposedAction) -> ActionPolicyOutcome;
```

| Input | Decides |
|-------|---------|
| `DataClass` | `private` \| `personal` \| `circle_ok` |
| `DisclosureTier` | `metadata_only` … `auditable` |
| `Destination` | `Local` \| `EstateWorker { id }` \| `CirclePeer { circle, peer }` |
| Session floor | `restrict_data_class(session, job)` — most restrictive wins |

## Data classes (D1)

| Class | Remote default |
|-------|----------------|
| `private` | **Never** off coordinator (secrets, `.env`, keys) |
| `personal` | Owner estate workers; circle peers only in `PolicyMode::Full` |
| `circle_ok` | Estate workers always; circle peers in `Full` mode |

Classifier runs at session start (bounded tree walk for `.env` / keys + goal keywords). Client override cannot loosen below floor (`apply_data_class_floor`).

## Homelab vs Circle

| Mode | Estate workers | Circle peers / jobs |
|------|----------------|---------------------|
| `EstateStub` | Allowed for `personal` / `circle_ok` | **Denied** |
| `Full` | Allowed | Allowed when disclosure tier sufficient |

**Private data never remote in any mode.**

## RPC surface

- `policy/get` — mode, toggles, workspace classifier floor, `policy_epoch`
- `policy/set` — mode, `verify_allowed`, `mutations_allowed`; bumps epoch for worker snapshots

## Worker policy snapshots

Not implemented. Coordinator `policy/set` still bumps `policy_epoch` for settings; that is not a signed worker snapshot persist or ingress SoT. Do not treat a `WorkerPolicySnapshot` type as live.

## Persistence

Coordinator: `policy_settings` in `lokai.db` (schema v6) — mode, verify/mutation toggles. Session `data_class` column validated on write.

---

**Last updated:** 2026-06-27

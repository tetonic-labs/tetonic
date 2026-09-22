# lokai-policy

Unified policy engine (D1): data-class rules, remote inference placement, tool gates, and execution-contract evaluation.

## Role in the stack

`PooledProvider` asks `PolicyEngine::check_remote_inference` before sending jobs to workers. `EngineRuntime` attaches policy to every production agent. `SessionHost` classifies workspace data class at session start. Signed worker-policy push to nodes is **not implemented** and is not a live store.

## Modules

| Module | Purpose |
|--------|---------|
| `mode.rs` | `PolicyMode` — `EstateStub` (homelab) vs `Full` (Circle) |
| `classify.rs` | Session classifier, data-class floor, disclosure defaults |
| `fabric.rs` | `Destination`, `FabricJobDraft`, `PolicyContext` |
| `placement.rs` | M5-3 trust × data-class matrix and stable reason codes |
| `engine.rs` | `PolicyEngine` — remote, tool, and action evaluation |
| `shell.rs` | Destructive shell command blocklist |
| `hosted.rs` | Explicit full-payload hosted disclosure ceiling; disabled by default, secrets always denied |

## Key API

- `PolicyEngine` — `check_remote_inference`, `check_data_class`, `evaluate_action`
- `PolicySettings` — persisted mode + verify/mutation toggles
- `PolicyMode`, `PolicyContext`, `FabricJobDraft`, `Destination`
- `classify_session`, `apply_data_class_floor`, `restrict_data_class`, `default_disclosure_tier`
- `trust_permits_data_class`, `placement_to_dispatch_local_only`

### Invariants

- **`Secret` is always local-only** — no project or worker setting can override it
- External-untrusted workers are denied repository source by default
- **Circle routing** requires `PolicyMode::Full` and adequate `disclosure_tier`
- **`run_shell`** — policy blocks destructive patterns; approval hook still required
- **`spawn_agent`** blocked for `private` sessions

## Dependencies

- `lokai-domain` — `DataClass`, `DisclosureTier`, `PolicyEvaluator`, `ProposedAction`

## RPC (`policy/get`, `policy/set`)

| Field | Purpose |
|-------|---------|
| `mode` | `estate_stub` \| `full` |
| `verify_allowed` | Gate verify-before-finish |
| `mutations_allowed` | Gate `edit_file` / `write_file` |
| `policy_epoch` | Bumped on every `policy/set` for fabric worker snapshots |
| `default_data_class` | Workspace classifier floor from `policy/get` |

Persisted in `lokai.db` `policy_settings` (schema v6).

## Product plan

| ID | Feature | Status |
|----|---------|--------|
| D1 | Policy engine | Done |
| N0–N1 | EstateStub homelab | Done |
| N2 | Full + Circle | Done (engine); worker push not implemented |
| D3 | Worker policy snapshot | Not implemented; not a live store |
| M5-3 | Trust matrix + project placement policy | Done |

## Tests

`cargo test -p lokai-policy` — 47 tests: class×destination and trust matrices,
project policy invariants, classifier tree walk, shell blocklist, action gates,
trait impls, and pooled callback integration.

## Related docs

- [policy-engine-v1](../../../docs/implementation/contracts/policy-engine-v1.md)
- [agent-rpc-v1](../../../docs/implementation/contracts/agent-rpc-v1.md)

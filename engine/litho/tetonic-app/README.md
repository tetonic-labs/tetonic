# tetonic-app

Application kernel: workflow decisions for managed runs, estate, capacity, policy, and approvals. The local workspace host and `tetonic-cli` operator commands hold `Arc<Application>` and must not reimplement owned workflows.

`local_workspace::LocalWorkspace` composes the first single-owner web connection:
durable local work and agent registration, guarded local model discovery, bounded
execution of a selected agent, authorized result inspection, and cancellation.
Agent preferences can lower host limits but never grant capabilities. It reuses
this kernel and its store; it is not a fleet dispatcher.
See [the local UI contract](../../../docs/implementation/contracts/local-ui-v1.md).

## Role

| Service | Owns |
|---------|------|
| `InitializationService` | Runtime bootstrap (artifacts, GC, scanners) |
| `RunService` / `turn_execution` | Identity jobs, inspection, replay, cancellation; shared finalization, event and workspace hooks |
| `PolicyService` / `ApprovalService` | Policy mode + approvals |
| `EstateService` + `estate_enrollment` | Enroll/remove/status; coordinator key + enrollment egress reload |
| `CapacityService` | Optimize, doctor, status, profiles |

## Resource operation inputs

Work creation and delegation, brief updates, scoped message publication, approvals,
effort recording, workstation enrollment/claims, delegated execution grants and
hierarchical stops use named Rust inputs exported from `resources`. These inputs
adapt the existing service methods; they do not introduce another resource store
or execution path.

Credentials remain separate method arguments. Services derive the principal from
verified authority and retain their scope, retry, version and generation checks.
Device secrets authorize worker claims; a live `DelegationParent` remains required
for delegated execution. Neither is supplied as ordinary request data. These are
Rust API inputs, not serialized HTTP contracts. Existing CLI flags, local UI
payloads and database formats are unchanged.

## Named exceptions (R26 / M1-2)

Every residual production decision that stays in `tetonicd` / `tetonic-cli` must appear here. **No silent dual paths.** New exceptions require a README row before merge.

| Exception | Owner string | Why not migrated | Gate / removal |
|-----------|--------------|------------------|----------------|
| Egress CRUD (CLI `/egress`, optional RPC) | `M1-4 transport infra` | Persist allow/remove is infra; default-deny RPC; R26 OOS beyond inventory | Document only until egress service sprint |
| Worker trust set/get (CLI + fabric RPC) | `estate trust adapter` | Persist + live registry invalidation still split; migrate to `EstateService` later | No second grant algorithm in a third bin |
| Secret override grant/revoke RPC | `secrets adapter` | Durable store mutation in handler; no app service yet | Do not add CLI duplicate grant path |
| CLI resume-if-running heuristic | `tetonic-cli session UX` | Terminal resume predicate; app owns rehydrate | Do not redefine `RESUME_MESSAGE_CAP` (gated) |
| CapacityJobRuntime busy/cancel/job flags | `tetonicd CapacityJobRuntime` | Daemon exclusive lease for optimize job | Existing `tetonicd_no_capacity_workflow` |
| Initialize compute/index/caps assembly | `tetonicd initialize adapter` | Transport/infra seam around `Application::from_bootstrap*` | — |
| Fabric/node/enroll/serve modes | `tetonicd node` | Explicit remainder OOS | — |
| CLI offline index/history/time-travel/project | `tetonic-cli offline` | Infra leftover | `cli_infra_leftovers` |
| Worker `--worker` fabric capacity GET | `tetonic-cli capacity` | Fabric transport after app coordinator load | Uses `estate_enrollment` helpers |
| TUI `/ps` `/evict` Ollama HTTP | `tetonic-cli chat` | Local inspector infra | `cli_inspector_no_command` |

### Migrated (must not reappear in bins)

| Helper | Owner | Gate |
|--------|-------|------|
| `reload_enrollment_egress` | `tetonic_app::estate_enrollment` | `no_duplicate_enrollment_helpers` |
| `load_or_create_coordinator` | `tetonic_app::estate_enrollment` | `no_duplicate_enrollment_helpers` |
| `RESUME_MESSAGE_CAP` | `tetonic_app::resume` | `no_duplicate_resume_cap` |

## Tests

```bash
cargo test -p tetonic-app
cargo test -p tetonic-arch-gate
cargo run -p tetonic-arch-gate -- verify package
```

## Product plan

| ID | Status |
|----|--------|
| M1-2 Application kernel ownership | **Complete** (R26 exception inventory + enrollment helper collapse) |
| R26 M1-2 exception closeout | **Done** |

The local UI also supports explicit prompt-only OpenAI/Anthropic agents, native
vault credential setup/rotation/removal, and provider-specific failure messages.
Hosted calls share broker admission and managed-run lifecycle; they never enter
local/worker model routing. See the local UI contract for disclosure boundaries.

LocalWorkspace::submit_in_conversation accepts an optional parent task ID. Follow-ups reuse authorized same-agent history within a bounded prompt, preserving per-turn execution grants, cancellation, and idempotency.

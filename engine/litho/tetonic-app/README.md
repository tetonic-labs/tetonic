# lokai-app

Application kernel: workflow decisions for sessions, runs, estate, capacity, policy, and approvals. Transport adapters (`lokaid`, `lokai-cli`) hold `Arc<Application>` and must not reimplement owned workflows.

Live model/provider changes use the shared application operation described in
[inference selection](INFERENCE-SELECTION.md). Changes are revision-checked and
accepted between turns; portals select registered profiles and never construct providers.

## Role

| Service | Owns |
|---------|------|
| `InitializationService` | Runtime bootstrap (artifacts, GC, scanners) |
| `SessionService` / `SessionLiveStore` | Session lifecycle + turn admission |
| `RunService` / `turn_execution` | Turn plan/run/complete, spawn |
| `PolicyService` / `ApprovalService` | Policy mode + approvals |
| `EstateService` + `estate_enrollment` | Enroll/remove/status; coordinator key + enrollment egress reload |
| `CapacityService` | Optimize, doctor, status, profiles |
| `resume` | Message rehydrate + `RESUME_MESSAGE_CAP` |

## Named exceptions (R26 / M1-2)

Every residual production decision that stays in `lokaid` / `lokai-cli` must appear here. **No silent dual paths.** New exceptions require a README row before merge.

| Exception | Owner string | Why not migrated | Gate / removal |
|-----------|--------------|------------------|----------------|
| Egress CRUD (CLI `/egress`, optional RPC) | `M1-4 transport infra` | Persist allow/remove is infra; default-deny RPC; R26 OOS beyond inventory | Document only until egress service sprint |
| Worker trust set/get (CLI + fabric RPC) | `estate trust adapter` | Persist + live registry invalidation still split; migrate to `EstateService` later | No second grant algorithm in a third bin |
| Secret override grant/revoke RPC | `secrets adapter` | Durable store mutation in handler; no app service yet | Do not add CLI duplicate grant path |
| CLI resume-if-running heuristic | `lokai-cli session UX` | Terminal resume predicate; app owns rehydrate | Do not redefine `RESUME_MESSAGE_CAP` (gated) |
| CapacityJobRuntime busy/cancel/job flags | `lokaid CapacityJobRuntime` | Daemon exclusive lease for optimize job | Existing `lokaid_no_capacity_workflow` |
| Initialize compute/index/caps assembly | `lokaid initialize adapter` | Transport/infra seam around `Application::from_bootstrap*` | — |
| Fabric/node/enroll/serve modes | `lokaid node` | Explicit remainder OOS | — |
| CLI offline index/history/time-travel/project | `lokai-cli offline` | Infra leftover | `cli_infra_leftovers` |
| Worker `--worker` fabric capacity GET | `lokai-cli capacity` | Fabric transport after app coordinator load | Uses `estate_enrollment` helpers |
| TUI `/ps` `/evict` Ollama HTTP | `lokai-cli chat` | Local inspector infra | `cli_inspector_no_command` |

### Migrated (must not reappear in bins)

| Helper | Owner | Gate |
|--------|-------|------|
| `reload_enrollment_egress` | `lokai_app::estate_enrollment` | `no_duplicate_enrollment_helpers` |
| `load_or_create_coordinator` | `lokai_app::estate_enrollment` | `no_duplicate_enrollment_helpers` |
| `RESUME_MESSAGE_CAP` | `lokai_app::resume` | `no_duplicate_resume_cap` |

## Tests

```bash
cargo test -p lokai-app
cargo test -p lokai-arch-gate
cargo run -p lokai-arch-gate -- verify package
```

## Product plan

| ID | Status |
|----|--------|
| M1-2 Application kernel ownership | **Complete** (R26 exception inventory + enrollment helper collapse) |
| R26 M1-2 exception closeout | **Done** |

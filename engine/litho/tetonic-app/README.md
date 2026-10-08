# tetonic-app

Application composition and authorized product services for Tetonic. This crate
connects work, saved agents, capabilities and context to the existing managed
execution engine. It does not own a second run journal or executor lifecycle.

## Where changes belong

| Owner | Responsibility |
|---|---|
| `host` | Shared application dependencies, storage and diagnostics configuration |
| `errors::PublicFailureV1` | Versioned safe failure categories and recovery guidance; transport status mapping stays in the adapter |
| `work::WorkService` | Work shaping, plans, submission, human questions, continuation and inspection |
| `workspace::WorkspaceServices` | Scoped agent editing, providers and capability operations |
| `resources` | Authenticated identity, grants, context, work and control operations |
| `resources::registered` | Authorized revision/job preparation, harness assembly and supported reconstruction |
| `execution` | Run-service adapter, audit interface, managed observation hooks and finalization effects |
| `events` | Product observations and redacted agent-step projection |
| `team_work_controller` | Assignment readiness, parallel dispatch and contribution collection |
| `coding_pack` | Retained coding-specific strategy integration |
| `estate_enrollment`, capacity and policy services | Existing operator and infrastructure operations |

`local_workspace::LocalWorkspace` remains the public compatibility name for
`WorkService`. Local bootstrap composes the single-owner host and defaults;
scoped resource operations still check current authority. The local HTTP adapter
lives in `tetonic-cli`.

`execution::RunService` adapts the existing `tetonic-run::ManagedRunService`.
The latter owns admission, attempts, leases, cancellation and accepted outcomes.
The public `turn_execution` and `turn_attestation` modules retain compatibility
reexports; new callers should use `execution` and `events::agent_steps`.

Credentials remain separate from named resource-operation inputs. Request bodies,
saved tool names and team membership cannot manufacture grants. A live managed
`DelegationParent` remains required for governed child execution.

## Architecture and tests

- [System overview](../../../docs/architecture/README.md)
- [Ownership map](../../../docs/architecture/ownership.md)
- [Host configuration](../../../docs/architecture/host-configuration.md)
- [Scoped work services](../../../docs/architecture/work-services.md)
- [Execution, harness and inference boundaries](../../../docs/architecture/execution-boundaries.md)
- [Durable state, budgets and public contracts](../../../docs/architecture/durable-state-and-contracts.md)
- [Local UI contract](../../../docs/implementation/contracts/local-ui-v1.md)

From `engine`, run `cargo test -p tetonic-app` and
`cargo run -p tetonic-arch-gate -- verify package`.

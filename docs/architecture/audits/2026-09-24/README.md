# Tetonic distributed-agent architecture audit

Audit started 2026-09-24 and completed 2026-09-25. Baseline: `eae7db27315b145bfad82921426ff44960d11c4b`.

**Verdict: preserve the existing execution foundations, but converge the standing-agent product onto one durable authority and execution path before expanding distribution.** The repository contains real job supervision, inference brokering, security boundaries, and storage. Standing-agent orchestration is substantially less integrated than the names and comments suggest. A local working demonstration is not yet proof of safe distributed ownership or recoverable agent continuity.

Deliverables:

- [Findings and evidence](findings.md): 24 prioritized findings, including concrete defects.
- [Target architecture](target-architecture.md): domain model, authority boundaries, execution protocol, and placement design.
- [Migration sequence](migration-plan.md): ordered work packages and objective acceptance gates.
- [Crate inventory](inventory.json): all 32 workspace crates and their internal dependencies.
- [Executable audit probes](probes/src/lib.rs): ten independent reproductions using the actual library APIs.

## Scope and method

Inventoried all 32 crates across Core, Mantle, Strata, Atmos, Litho, and Tooling: 659 Rust files, 174,947 lines including tests. Reviewed dependency direction, public contracts, entry points, the main execution paths, storage and recovery, broker/fabric boundaries, new standing-agent modules, CI/release configuration, and representative tests. Searched call sites to distinguish exported abstractions from production wiring. This is a repo-wide architectural sweep with targeted deep review, **not a claim that every line received manual review**.

No production code was changed. No local experiment restart, deployment, push, or external service operation was performed. Existing untracked design/sprint files were left alone. The audit harness is an independent Cargo workspace outside `engine/`; it does not become a production dependency.

## Evidence standards

- **Reproduced:** a probe invokes real library code and demonstrates the unwanted behavior.
- **Code-confirmed:** the cited implementation directly establishes the finding; no runtime reproduction was attempted.
- **Integration gap:** implementation exists, but inspected production composition/call sites do not connect it to the claimed behavior.
- **Design risk:** a necessary distributed-system property is not established by the reviewed path. This is not an assertion of a deployed exploit.

P0 means a blocker to enabling distributed agent ownership or relying on the named safety/durability guarantee. It does not imply the loopback Village experiment is already exposed to a hostile cluster. P1 means the next architecture/convergence work; P2 means follow-on operational and maintainability work.

## Validation actually performed

| Check | Result | Meaning |
|---|---|---|
| `cargo run -p tetonic-arch-gate --` | Passed | Existing static architecture checks are satisfied; their coverage has gaps described in F19. |
| `cargo fmt --all -- --check` | Failed, differences in 32 files | Formatting debt is real, including recent standing-agent work. No formatting was applied during the audit. |
| `cargo test --workspace` | Failed during compilation, exit 101 | Missing `tetonic_app` / other crate metadata and rlib errors. No full workspace test pass is claimed. Root cause remains unresolved; this may involve local build artifacts and is not classified as a source defect. |
| `cargo test -p tetonic-domain --lib -j 1` | 30 passed | Existing domain unit suite passes despite the additional contract defects reproduced below. |
| Independent audit harness | 10 passed | **Passing means the ten unwanted behaviors reproduced**, not that the engine is correct. |

The host had approximately 1.8 GB free after compilation. A clean rebuild was not attempted; deleting existing build artifacts was outside this audit. Full Clippy, Linux/macOS runtime validation, distributed partition testing, load testing, and long-running model evaluation were not completed.

Raw command output remains locally in `workspace-tests.log`, `domain-tests.log`, `format-check.log`, and `probes.log`; logs are ignored by Git. Reproduce the harness from the repo root with:

```powershell
cargo test --offline --manifest-path docs/architecture/audits/2026-09-24/probes/Cargo.toml --target-dir engine/target
```

The harness has a lockfile. Its characterization assertions must be replaced/inverted when fixes land; they must never become a CI requirement to preserve defective behavior.

## What should survive the redesign

1. `tetonic-run`: command-driven transitions, attempt leases, durable events, deduplication, finalization ownership, and explicit recovery. Extend the lifecycle model without turning an immortal agent into one immortal task.
2. `tetonic-broker`: admission, reservations, placement revalidation, bounded queues, circuit/fallback concepts. Use this for standing-agent inference instead of another scheduler.
3. Fabric result validation: worker/coordinator identity, attempt/task/input binding, revocation checks, signed result handling, quarantine. Reuse these principles for agent ownership and external effects.
4. `WorkScope`: cancellation closes admission while quiescence tracks actual outstanding work. Carry that distinction through standing-agent shutdown and migration.
5. Policy, egress, redaction, sandbox, transaction, and artifact components: valuable implementation investments. Make their use part of production composition and expose their actual limitations.
6. Existing typed IDs, neutral adapter interfaces, and local SQLite implementation: useful starting points. Keep standalone simple while making ownership semantics identical across deployment modes.

## Coverage map

| Area | Crates inventoried | Review emphasis |
|---|---|---|
| Core | core, domain, runtime, policy, sandbox, secrets, telemetry, transaction | Execution paths, action authorization, cancellation, contracts, checkpoint implementation, confinement and mutation boundaries. |
| Mantle | run, broker, orchestrator, node, capacity, enroll, server | Lifecycle authority, leases, fleet API wiring, compute admission, worker ingress, startup and configuration. |
| Strata | memory, artifact, context, index | Transactional state, local storage contract, provenance/quarantine, context selection; index reviewed primarily as a coding capability boundary. |
| Atmos | inference, egress, fabric-client, fabric-protocol, rpc | Provider wiring, token accounting, transport/result validation, trust boundary and compatibility. |
| Litho | app, tools, lsp, lokai-cli, lokaid | Composition ownership, coding product coupling, operator/fleet APIs, public API/release entry points; CLI/LSP internals sampled rather than exhaustively audited. |
| Tooling | arch-gate, eval, bench | Enforcement coverage, CI invocation, behavioral versus implementation tests, readiness claims. |

The largest concentration is `tetonic-app` (33,110 Rust lines including tests, 25 internal production dependencies). Size is not itself a defect; this concentration explains why engine bootstrapping remains tied to a coding application.

## Recommended investment order

Correct misleading guarantees and concrete defects; establish a canonical agent/activation/attempt model; converge the local server onto durable supervision and compute admission; implement transactional inbox/outbox and effect reconciliation; prove failover with one coordinator and two runners; then address high availability, capacity, and richer cognition. More fleet endpoints or more node-role enums will not substitute for those proofs.

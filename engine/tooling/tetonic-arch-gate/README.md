# tetonic-arch-gate

The engine's static architecture checks (`ARCH-*`) and engineering verification
entry point (`QG-*`). This is development tooling, not a runtime service.

## Use

From `engine/`:

```sh
cargo run -p tetonic-arch-gate
cargo run -p tetonic-arch-gate -- quality
cargo run -p tetonic-arch-gate -- verify fast --crate tetonic-app
cargo run -p tetonic-arch-gate -- verify package
cargo run -p tetonic-arch-gate -- verify full
cargo test -p tetonic-arch-gate --lib
```

Fast runs formatting, architecture, quality and optional package compilation.
Package adds workspace/all-target Clippy with warnings denied. Full additionally
runs workspace tests. Run behavioral suites for the changed owner; package
verification alone does not run them.

## Navigate the checks

| Source | Responsibility |
|---|---|
| [lib.rs](src/lib.rs) | Architecture aggregation and shared scans |
| [product_boundaries.rs](src/product_boundaries.rs) | Current required owners and product lifecycle/assembly boundaries |
| [ids.rs](src/ids.rs) | Stable IDs, explanations and repair guidance |
| [quality.rs](src/quality.rs) | Direct dependency, model-literal and documentation checks |
| [verify.rs](src/verify.rs) | Cargo command tiers and combined findings |
| [tests.rs](src/tests.rs) and inline module tests | Deliberate violations and legitimate fixture cases |

`ARCH-OWNER-001` fails for missing current owners instead of accepting an absent
scan path. `ARCH-OWNER-002` checks current transports and work/resource services
for direct lifecycle or agent construction outside the established assembly
owner. `ARCH-FS-001` scans current tool code for known workspace mutation bypasses.
Existing journal ownership, network/process, dependency and wiring checks remain.

These are scoped source checks, not complete Rust semantic analysis. Some retained
checks still target retired daemon, RPC or eval paths; finding no files there is
not evidence about today's product API. Do not advertise those retired entry
points as supported. When moving an owner, move its checks and negative tests as
well, then run behavior tests through the current entry point.

See [quality policy](../../../docs/engineering/QUALITY-GATE.md),
[documented debt](../../../docs/engineering/QUALITY-DEBT.md), and the
[current architecture](../../../docs/architecture/README.md). Frontend boundary
checks live in [web/architecture.mjs](../../../web/architecture.mjs), using its
TypeScript import graph; do not add a second Rust checker for browser behavior.

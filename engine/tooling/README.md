# Tooling

Development checks and retained benchmark support.

| Package | Current responsibility |
|---|---|
| [tetonic-arch-gate](tetonic-arch-gate/) | Engineering gate and static architecture/quality checks |
| [tetonic-bench](tetonic-bench/) | Retained benchmarking support |

This is a package group, not an independently deployable service or a strict
layering rule. Use the [current ownership map](../../docs/architecture/ownership.md)
and Cargo manifests for dependencies and change routing. A retained library is
not automatically exposed in the connected product.

Use `cargo run -p tetonic-arch-gate -- verify package` from `engine/` and choose
behavior tests for the affected owner. The retired eval binary is not a current
verification command. Read [quality policy](../../docs/engineering/QUALITY-GATE.md)
for check scope and known limits.

# Tooling Layer (`engine/tooling/`)

## Purpose
The **Tooling** layer provides developer tooling, architecture enforcement gates, quality linters, benchmark suites, and model evaluation harnesses.

## Packages
- [`lokai-arch-gate`](./lokai-arch-gate): Automated architecture invariant verification, size limit audits (`ARCH-SIZE-001`), and formatting gates.
- [`lokai-eval`](./lokai-eval): Automated evaluation suite testing model capabilities, tool usage, and security defenses.
- [`lokai-bench`](./lokai-bench): Micro-benchmarks for CPU-intensive runtime paths.

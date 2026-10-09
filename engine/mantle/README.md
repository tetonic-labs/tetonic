# Mantle

Managed execution, compute brokering and inference worker machinery.

| Package | Current responsibility |
|---|---|
| [tetonic-run](tetonic-run/) | Managed execution, durable supervision and finalization |
| [tetonic-orchestrator](tetonic-orchestrator/) | Retained coding/router strategies; not the current team-work controller |
| [tetonic-broker](tetonic-broker/) | Compute admission, scheduling and inference/process broker adapters |
| [tetonic-capacity](tetonic-capacity/) | Hardware/model capacity and topology |
| [tetonic-node](tetonic-node/) | Worker service machinery and inference ingress |
| [tetonic-enroll](tetonic-enroll/) | Node enrollment, identities and trust transport |

This is a package group, not an independently deployable service or a strict
layering rule. Use the [current ownership map](../../docs/architecture/ownership.md)
and Cargo manifests for dependencies and change routing. A retained library is
not automatically exposed in the connected product.

The current product team-work controller lives in `tetonic-app`. The retained
`tetonic-orchestrator` library is not that controller. Inference worker machinery
does not provide replicated agent ownership or a Keeper deployment.

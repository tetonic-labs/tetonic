# Atmos

Provider adapters, authorized egress and inference fabric transport.

| Package | Current responsibility |
|---|---|
| [tetonic-inference](tetonic-inference/) | Provider adapters, model transport and pooling |
| [tetonic-egress](tetonic-egress/) | Outbound transport authorization and guard |
| [tetonic-fabric-client](tetonic-fabric-client/) | Client transport for enrolled compute targets |
| [tetonic-fabric-protocol](tetonic-fabric-protocol/) | Fabric messages and delivery contracts |

This is a package group, not an independently deployable service or a strict
layering rule. Use the [current ownership map](../../docs/architecture/ownership.md)
and Cargo manifests for dependencies and change routing. A retained library is
not automatically exposed in the connected product.

Provider selection does not grant tool or network access. Retain controlled
egress and secret handling when adding transport. The retired editor RPC package
is not part of this group or a supported product entry point.

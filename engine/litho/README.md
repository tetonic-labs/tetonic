# Litho

Product interfaces, scoped application services and concrete tool integrations.

| Package | Current responsibility |
|---|---|
| [tetonic-cli](tetonic-cli/) | `tetonic` commands and local HTTP adapter |
| [tetonic-app](tetonic-app/) | Host composition, authorized services, work coordination and projections |
| [tetonic-tools](tetonic-tools/) | Concrete local tool effects and tool-host integration |
| [tetonic-lsp](tetonic-lsp/) | Retained language-server client integration |

This is a package group, not an independently deployable service or a strict
layering rule. Use the [current ownership map](../../docs/architecture/ownership.md)
and Cargo manifests for dependencies and change routing. A retained library is
not automatically exposed in the connected product.

`tetonic` exposes `ui`, `job`, `control` and `estate`. The local HTTP adapter
calls existing scoped application services; it does not own execution lifecycle.
Agent construction belongs to the registered assembly owner in `tetonic-app`.

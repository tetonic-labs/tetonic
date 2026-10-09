# Strata

Durable records, artifact payloads, context compilation and retained indexing.

| Package | Current responsibility |
|---|---|
| [tetonic-memory](tetonic-memory/) | Durable SQLite records, migrations and transactional integrity |
| [tetonic-artifact](tetonic-artifact/) | Artifact payload storage |
| [tetonic-context](tetonic-context/) | Context compilation and retrieval interfaces |
| [tetonic-index](tetonic-index/) | Retained code indexing and search machinery |

This is a package group, not an independently deployable service or a strict
layering rule. Use the [current ownership map](../../docs/architecture/ownership.md)
and Cargo manifests for dependencies and change routing. A retained library is
not automatically exposed in the connected product.

`tetonic-memory` is organized into control, execution, context, usage and
artifacts. Its transactions preserve integrity across those areas; moving a
module must not split an atomic operation or create a competing run journal.

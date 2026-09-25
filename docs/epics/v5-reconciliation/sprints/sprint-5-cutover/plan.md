# Sprint 5 — Product cutover and deletion

## REC-501: One server configuration and supported client path

Depends on: common runtime stable; remote settings depend on sprint 4. Converge server configuration and import experiment definitions. Route CLI/stdio compatibility through common control services. Update packaging, releases, health/readiness, migrations, shutdown/drain, and operational documentation. List unsupported modes explicitly.

Acceptance: clean install and upgrade exercise actual server/client artifacts; legacy config migration is deterministic; unsupported options fail; readiness reflects storage/control dependencies; graceful drain is tested; old supported CLI/RPC operations preserve behavior or return an explicit migration error.

Retirement: D09/D12 parsers and independent bootstraps. Rename crates only if it helps the public product, not as a substitute for consolidation.

## REC-502: Extract optional coding behavior and close retirement register

Depends on: all replacement gates relevant to a removal. Move specialist prompts/router/critic, coding completion policy, repository indexing/LSP and coding tools behind selected harness/capability composition. Remove unsupported speculative strategies after focused consumer review. Retain useful evaluation fixtures and integrity tests. Migrate legacy fabric peers before removing adapters; preserve history readers.

Acceptance: noncoding agent starts with no coding identity, tool pack, repository or critic policy; coding harness still completes representative work under common enforcement; retired exports have no supported callers; architecture gates prohibit bypass composition and cross-layer authority; all D items are closed or have a named deferred compatibility reason.

Commit boundary: capability extraction, caller cutover, removals, delivery updates. Exit: a removal report with source references and validation results, not merely a list of renamed modules.

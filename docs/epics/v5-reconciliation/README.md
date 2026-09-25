# V5 MVP epic — persistent, governed agent teams

Date: 2026-09-25. Source baseline: `6b9b7817d178911b4ca6d030860bc471f8d73ef5`.

## Decision

Consolidate Tetonic into a general-purpose platform for persistent agent teams: users create teams, assign goals or ongoing responsibilities, and let them work within enforced authority, scoped knowledge and resource limits. People can collaborate or intervene without watching every agent. Local/workstation/server execution and inference location are independent choices.

The current product scope is [MVP product intent](mvp-product.md), [MVP system contracts](mvp-system-contracts.md) and the [MVP sprint sequence](sprints/README.md). These supersede earlier assumptions that shared interaction is out of scope, whole-agent execution belongs only on servers, or production HA can be implicitly deferred. Earlier source findings remain evidence; old REC tickets are reference material, not a second active backlog.

The primary problem is fragmented authority, not a shortage of subsystems. Reconcile the existing managed execution path with fleet resources; do not build another run manager beside it. Remove the disconnected implementations after replacements are proven. Keep coding and world interaction as optional harness/capability implementations.

## Deliverables and evidence limits

- [Product charter and scope cuts](mvp-product.md): MVP commitments, exclusions, deployment milestones and unresolved decisions.
- [Product-driven system contracts](mvp-system-contracts.md): privacy, huddles, delegation, workstation execution and stop semantics.
- [Reuse/removal map](mvp-reuse-and-removal.md): reuse existing assets and eliminate obsolete authority, not useful capability.

- [Findings and source evidence](findings.md): traced seams, live dependencies, and concrete defects relevant to reconciliation.
- [Target contracts and migration decisions](reconciliation.md): authority, persistence, composition, and cutover design.
- [Retirement register](retirement.md): specific code to replace, isolate, or remove, with deletion gates.
- [All workspace packages](packages.md): manifest-derived dependency inventory and package-level disposition.
- [All tracked files](file-inventory.csv): mechanical census, size and SHA-256; package-level dispositions are inherited recommendations, not a semantic verdict on each file.
- [Machine-readable inventory](inventory.json) and [generator](inventory.py).
- [Validation results](validation.json) and [validator](validate.py): file hashes, Cargo workspace membership and local links.
- [Sequenced implementation tickets](sprints/README.md).
- [Quality double-pass review](quality-review.md): corrections, migration hazards and scenario acceptance matrix.
- [Operator configuration requirements](operator-configuration.md): mandatory configurable telemetry, logging and storage, plus production/distributed operational contracts.

Method: source reads at composition roots and critical lifecycle paths, repository-wide symbol/caller searches, manifest dependency inspection, tracked-file census, and delivery workflow inspection. Earlier architecture documents were not treated as evidence for runtime behavior. This is an initial reconciliation audit, not an exhaustive semantic proof or a compiler-derived whole-program call graph. No production runtime was started and no production code was changed. Dynamic, macro-generated, external library consumers and untracked files are not proven absent by lexical searches. Individual deletion PRs must repeat caller/build checks.

Existing untracked `docs/design/` and standing-agent sprint-6 work were preserved. The target image supplied in the conversation is the design brief; this document translates its boxes into ownership contracts rather than assuming the picture proves an implementation exists.

Inventory totals: **988 tracked files, 677 Rust files, 32 workspace packages**. All packages have a proposed disposition; the 12 findings below come from focused source review of integration and authority boundaries. Counts are at the stated baseline and exclude this report itself.

## Priorities

1. Establish one identity/definition model and trusted organization context.
2. Expose actual durable services through an authenticated API and reconcile desired state into managed executions.
3. Put both coding and world harnesses through this path, including all effect and inference enforcement.
4. Remove independent fleet state, fake running status, detached operator controls and the world-specific server composition.
5. Consolidate ownership with existing lease/delivery contracts; introduce remote whole-agent execution explicitly.
6. Retire old product shells and speculative strategies only after their useful responsibilities have destinations.

Do not begin with crate renaming, a new consensus implementation, wholesale deletion of coding packages, or exposing the existing in-memory REST-shaped dispatcher over HTTP. Each would leave the authority problem unresolved.

## Deployment milestones

A local product preview proves the experience on one machine. A distributed MVP candidate adds enrolled workstations and remote workers. A production release requires the chosen HA topology to pass failure tests before advertising HA; MVP-001 decides the backend/coordination design and MVP-701/702 implement and validate it. Single-authority local storage remains an explicitly non-HA profile, never shared across machines. Durable storage is required for recoverable operation.

The product proof is a small team pursuing a goal, proposing a huddle, dividing work, protecting private context, parking blocked work, escalating limits/approvals, and surviving stop/restart with truthful outcomes. Exercise coding and a noncoding external-tool/environment workload. The Village may supply the latter; game development remains outside the engine epic.

## Completion standard

Reconciliation is complete when all production activation doors converge on the managed lifecycle; organization isolation reaches every data/effect boundary; observed state comes from accepted execution events; obsolete authorities are removed; and restart, stale-worker, approval, cancellation, and uncertain-effect tests pass. A renamed module or a passing existing unit test alone is not completion.

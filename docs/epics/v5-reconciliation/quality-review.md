# Quality double-pass review

Reviewed audit commit: `8d2ebbf`. Source baseline remains `6b9b7817d178911b4ca6d030860bc471f8d73ef5`; this review changes planning/validation only.

## Pass 1 — Evidence and disposition accuracy

Re-read managed executor/claim/cancellation/finalization paths, fleet supervisor/creation paths, Keeper caller references, broker budget structures, local executor adapter, and actual release packaging. Rechecked constructor searches. The core fragmentation findings hold, with these corrections:

| Issue found | Correction |
|---|---|
| Plan could imply the executor adapter must be created from scratch | Existing LocalAgentAttemptExecutor is reused; managed service injection is what must change |
| Cancellation rewrite could drop quiescence guarantees | Preserve WorkScope draining, effect lifetime and manager-owned completion claims |
| Worker proposal omitted execution affinity | Explicit LocalSet/spawn_local hosting decision; do not require existing harnesses to become Send by assertion |
| Fleet described as an independent production authority | Clarified that it is a detached prototype which must not become a competing authority |
| Diagnostic buffers described too broadly as state authorities | Consolidate schemas/projections; retain bounded buffers where useful |
| Stream/composite adapters treated as potentially redundant too early | Defer deletion until supported transport/integration requirements are established |
| Release claim cited build CI rather than release packaging | Added the actual release workflow as evidence |

Additional evidence: `managed/execution.rs` compares current stored identity to admitted identity, including definition binding; app `identity_job.rs` enables up to two speculative attempts; `managed/finalization.rs` owns completion claims and effect ordering. Those details materially constrain migration.

## Pass 2 — Sequencing and failure safety

Walked create/update/activate/approve/cancel/restart/remote-failover through the proposed sequence and checked deletion prerequisites against consumers.

| Hazard | Required plan change |
|---|---|
| D01/D04 deletion scheduled before runtime/operator cutover | Replacements start early; final removal waits through sprint 3 |
| Updating an identity's definition invalidates already-admitted work | Separate mutable default revision from immutable admitted revision; test queued/running/waiting updates |
| Coding speculation inherited by effectful world agents | Default generic activations to one attempt; require explicit safe effect profile for speculation |
| API exposure precedes complete tenant enforcement | Restrict early deployment to a single-organization development profile; gate multi-tenant release on all reachable boundaries |
| Budget work deferred while execution is enabled | Require bounded time/output/concurrency and existing admission before activation |
| Single-controller assumption enforced only by deployment convention | Reject a second writer to the same store using a reviewed mechanism |
| Durable resources plus in-memory fleet during migration drift apart | One mutable authority; compatibility projections only, no dual writes |
| Cancellation request mistaken for effects having stopped | Distinct requested/acknowledged/quiescent/unknown outcomes and bounded escalation |
| Lease renewal trusted during partition or skew | Authority-assigned validity, stale-generation tests, fail-closed new effects after expiry |
| Backups mentioned without actual rollback semantics | Expand/migrate/switch/contract; rehearsal with supported binary/schema versions and explicit write-loss limitations |

## Scenario acceptance matrix

These are future implementation acceptance checks, not tests claimed to have run in this audit.

| Scenario | First required ticket | Expected result |
|---|---|---|
| Two agents share one definition | REC-101 | Distinct identity/state/grants; shared immutable recipe |
| Definition changes while activation waits | REC-101 / REC-201 | Old work stays pinned or explicitly canceled; no accidental rebinding |
| Duplicate API activation after response loss | REC-102 / REC-201 | One admitted logical activation; retry returns its identity |
| Second controller opens the same authoritative store | REC-102 | Rejected or fenced; never two independent writers |
| Cancel during blocking tool execution | REC-201 | Preserve quiescence tracking; unknown external outcome reported if necessary |
| Lost durable world event among newer snapshots | REC-202 | Durable events retained/redelivered independently of snapshot coalescing |
| Two speculative attempts issue a mutation | REC-201 / REC-202 | Disallowed by default; explicit profile required |
| Restart with approval pending | REC-301 | Durable request restored; exact action/ownership/policy revalidated |
| Cross-tenant artifact or event cursor | REC-301 / REC-302 | Denied without leaking contents or sensitive error data |
| Old worker returns after reassignment | REC-401 | Stale result rejected and new mediated effects denied |
| External effect succeeds but acknowledgment is lost | REC-402 | Reconcile or mark unknown; no blind retry |
| Roll back after database migration | REC-501 | Supported version path or explicit restore procedure and data-loss boundary |

## Validation scope

The validator now checks exact tracked-path coverage against the recorded Git baseline, file sizes/hashes, workspace package membership and per-package counts, Rust counts, local links and Markdown trailing whitespace. No runtime tests were run: no runtime code changed. Mermaid source was reviewed but not rendered in this pass. Hashes describe the captured working-tree bytes; a checkout with different line-ending normalization must recapture rather than silently treating changed bytes as equal.

Remaining decisions are explicit: authentication integration, supported harness recovery profiles, memory retention, compatibility versions, lease timing and initial worker execution environment. The revised plan is suitable for the contract/characterization sprint; it is not authorization to delete code before the listed gates or proof of production guarantees.

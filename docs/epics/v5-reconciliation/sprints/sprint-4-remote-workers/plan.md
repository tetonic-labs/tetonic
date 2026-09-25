# Sprint 4 — Distributed execution ownership

Historical REC work package. Scheduling and scope are superseded by the [MVP sequence](../README.md); retain applicable technical safeguards as reference.

## REC-401: Durable assignments and worker lifecycle

Depends on: sprint 3. Audit existing run leases, fabric lease table and registry prototype together. Choose one assignment-generation authority and persist issuance. Add authenticated worker registration, supported-harness capabilities, drain, heartbeat, assignment claim and stale result rejection. Reuse enrollment/TLS only where its trust semantics match the new contract.

Acceptance: killing/restarting controller does not reset fencing; delayed old worker cannot acquire a new mediated effect or commit a result; concurrent assignment requests converge; partitions produce explicit stale/unknown states; inference-only workers are never assigned whole-agent jobs. Document single-control-plane availability limits.

Test lease expiry under clock skew, controller partition, duplicate delivery and second-controller startup. Control-plane unavailability must stop new effects when grants expire without fabricating terminal outcomes. Include bounded queue recovery and reconnect backoff; heartbeat renewal alone is not assignment ownership.

Retirement: D06 prototype registry once tests target the integrated service.

## REC-402: Remote harness execution and effect recovery

Depends on: REC-401. Define remote execution protocol separately from Infer jobs. Include definition/input verification, artifact preparation, constrained credentials, progress, cancel and supported checkpoint behavior. Track issued effects, receipts and uncertain outcomes; use idempotency only for tools that actually implement it.

Acceptance: worker loss before dispatch, after effect dispatch and before acknowledgment each produce correct recovery; unsupported recovery requires intervention; workspace persistence is declared; stale workers cannot mutate controlled resources through a still-valid old grant. Include an independently failing tool service in tests. Never claim exactly-once external effects from message dedup alone.

Exit: one control authority with multiple real agent workers and failure evidence; separate Keeper deployment remains conditional on a reviewed consistency protocol.

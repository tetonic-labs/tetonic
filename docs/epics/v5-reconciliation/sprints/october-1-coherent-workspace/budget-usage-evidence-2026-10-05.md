# Work budgets and usage — October 5, 2026

Status: implemented locally, verification recorded below. No coordination gate
or parent sprint ticket is closed. The checkout already contains extensive
uncommitted reconciliation and UI work; this evidence describes the budget slice.

## Product behavior

- Usage is an overlay in the existing team/map workspace. It shows reported
  tokens, held allowances, pending/unconfirmed requests and per-work detail.
  Unknown usage is explicitly a lower bound. Historical untracked work is labeled.
- The owner can set a default token allowance for new requests, bounded by the
  existing host and agent limits. Existing requests are not silently enlarged.
  Follow-ups, exploration and plan generation each retain their own allowance.
- A conversation's inspector also exposes its individual request usage.
  Figures come from the engine snapshot, not logs, browser estimates or sample data.
- Saving requires an exact engine receipt. Lost-response retries retain one
  request ID. Concurrent changes use revisions; the user can reload the saved
  allowance. Disconnected figures are labeled stale and editing is disabled.

## Reused engine path

`LocalWorkspace -> ResourceService -> team work activation -> existing managed
registered executor -> WorkUsageProvider -> existing BrokerInferenceProvider`.

The wrapper preserves the broker, egress checks, provider selection, execution
authority and process broker. It introduces no scheduler, provider replacement or
independent work model. SharedStore adds schema v57 accounting tables alongside
the existing v55 work envelopes, reservations and delegation allocation ledger.

Before a provider call, an immediate transaction validates the work's scope and
activation mapping, live task/attempt claim, unexpired lease and current authority.
The attempt reserves the work's available share once. Each invocation has a
durable pending record before it leaves the engine. A second funded call is
refused while the first has no complete report. Subsequent calls share the
remaining reported-token allowance. Output generation is capped to that remainder.

Provider reports are persisted before returning tool calls. Conflicting duplicate
reports cannot rewrite history. Missing counts and overruns fail funded execution.
Cancellation or a lost response never implies zero usage. Complete reports plus
terminal quiescence under the original task/lease fence allow settlement;
reservation history is retained and the unused amount becomes available again.
Readers only observe; they cannot settle or take ownership.

The shared activation-ID helper retains the established hash mapping for work
request IDs containing an agent suffix. Scope checks use that same mapping rather
than inventing a new correlation rule.

## Validation

- `cargo test -p tetonic-memory -p tetonic-app --lib -j 2 --quiet`:
  163 memory tests and 191 application tests passed.
- Memory regressions cover concurrent connections, exact/conflicting reports,
  current claim/scope/lease, stale-owner settlement, no early release, incomplete
  usage retained across restart, overruns, setting authority/revisions and
  immutable old allowances.
- A real managed application/provider HTTP test exercises successful settlement,
  missing reports and an overrun. It verifies the outbound generation cap, actual
  terminal state, unchanged allowance after a default edit, idempotent submission
  with no second provider call and persisted counts after reopening the database.
- All 102 frontend tests passed, including lower-bound usage, old records,
  lost-save-response retry and an older engine without the new fields.
- TypeScript and Vite production build passed.
- Six focused CLI/local API tests passed; the Tetonic binary built successfully.
- Live Ollama (`qwen3.5:latest`) check in the isolated local workspace on ports
  3001/5174: saved a 3,000-token allowance through the UI, submitted one real
  request, observed the 3,000-token hold while running, then 359 input + 111 output
  = 470 reported tokens, zero held and 2,530 unused tokens returned. Reload retained
  the setting and counts; conversation inspection showed the same records.
- At 390px the allowance editor remained usable with no document horizontal
  overflow. The existing header remains horizontally scrollable. The temporary
  viewport override was reset. Screenshot: local-only
  `.lokai/budget-usage-check/usage-live.png`.
- Automatic approval review rejected the command to restart/replace the existing
  port-3000 engine, giving only "blocked by policy". That process and its database
  were left untouched. Live validation instead used a fresh database and separate
  engine/preview, without bypassing the blocked restart. The original port-5173
  workspace still requires a permitted update to the rebuilt engine binary.
- `git diff --check` passed under the repository's normal CRLF handling. No push
  or commit was made from the mixed pre-existing working tree.

## Remaining budget work

This is token accounting and a reported-usage guardrail, not guaranteed provider
billing control. A current prompt can exceed the allowance before the provider
reports it; adapter-internal retries are not individual ledger calls. There is no
dollar conversion, price catalog, organization/team periodic pool, compute/time
accounting, automatic top-up, budget escalation workflow or automatic recovery of
holds after a crash between reporting and settlement.

Parent and child allocations use the existing shared ledger. The current parent
attempt reserves all of its available share on its first inference call. Dynamic
sharing/reallocation and child runtime admission must be completed together before
enabling plan dispatch. This work does not claim distributed exactly-once billing,
live multi-agent spend enforcement or completion of COORD-A.

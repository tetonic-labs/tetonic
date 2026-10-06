# Fleet steering and current integration contracts

Date: October 5, 2026. Continues the [storage API follow-up](storage-api-evidence-2026-10-05.md).

## Scope and behavior

`FleetSupervisor::inject_steering` previously held both organization and agent
registry read locks while awaiting a bounded perception-channel send. A full
agent inbox could prevent registry writers from proceeding. The supervisor now
selects the squad and snapshots its recipients under short locks, then releases
those locks before delivering steering. This uses the existing fleet owner and
channels; it introduces no additional controller or execution path.

The regression fills an inbox and manually polls a second steering request until
it blocks. It checks that both registries remain writable, registers unrelated
organization/agent entries, inspects fleet status, and engages emergency stop.
Draining the inbox lets the original pending send complete; a closed inbox is
reported as zero successful deliveries. The regression failed on the old code
with `organization registry locked` and passes after the fix. All 65 orchestrator
library tests pass. The fix is committed as `41b14df`.

Delivery still uses sequential channel backpressure and a snapshot of recipients.
This change does not establish fair fan-out, timeout semantics, durable steering,
or distributed delivery guarantees.

## Reconciliation of the 19 prior integration failures

| Prior failure | Treatment |
|---|---|
| 14 assertions reading retired `tetonicd` source files | Follow the active `tetonic-cli::local_ui` transport and `tetonic-app::LocalWorkspace` owner. Retain CLI/eval submission checks. Assert that the HTTP handler delegates submission and does not construct execution hosts, spawn agent execution, or own conversation state. |
| Old approval/cancel RPC checks within those 14 | Follow HTTP delegation into credential-scoped, proposal-digest-bound approval resolution and cancellation of the task's actual managed run. Retain session/broker cancellation checks for the CLI path. |
| Old daemon tool-event check within those 14 | Check the active CLI `ApplicationEvent` consumer's tool-call/result mapping. This does **not** assert a raw event stream exists in the local HTTP UI; the removed RPC interface is not restored. |
| Two obsolete inline implementation assertions | Follow `session_classifier_fabric` and its run/task/attempt and disclosure fields; follow typed compute installation through `attach_compute_lifecycle` to the existing private supervisor. |
| Two competing-finalizer fixtures rejected at admission | Opt these scenarios into two simultaneous attempts using the existing `CreateRun.speculation` contract. A test-only forwarding supervisor changes that creation option; the real supervisor still owns all admission, leases, claims, persistence and replay. Ordinary fixtures and production defaults retain the one-attempt limit. The original failure/driver/claim assertions remain. |
| Blanket runtime secret-scanner dependency ban | Permit outbound scanning at `brain.rs` while prohibiting scanner use in other production runtime modules. Preserve the repository-walking/context-building prohibitions. Add behavioral tests proving completion messages, tool schemas, and perception state are redacted before provider delivery; the completion request observer receives exactly the redacted request fields. |

No integration test was ignored or deleted. The source checks still have the
limitations of source checks; they are not substitutes for live product trials.
The scanner fixtures use a public documentation example credential, never a live
secret. Provider/observer redaction tests use local recording providers and do not
call an external model.

## Verification

- Seven updated application integration suites: **179 passed, zero failed**.
- `cargo test -j 2 -p tetonic-app -p tetonic-runtime --lib --tests --no-fail-fast`:
  **632 passed, one failed, two ignored** on the initial broad run. All 393
  application integration tests passed, including all 19 previously failing
  cases. All 34 runtime tests passed, including both new redaction tests.
- That broad run exposed a separate timing assumption in the application library:
  the two-agent plan test read 820 held tokens just after its execution snapshot
  became completed. `registered_executor.rs` settles usage in its completion
  watcher, after managed finalization. The plan and human-handoff tests now wait
  for that independent durable settlement with a ten-second deadline, then retain
  their exact total-usage and zero-held-token assertions. A missing settlement
  still fails. Production accounting and UI states are unchanged; completion and
  reservation release may briefly be observed separately.
- `cargo test -j 2 -p tetonic-app --lib` after that test correction: **206 passed,
  zero failed, two ignored**. Combined with the other suites above and the 65
  orchestrator tests, the latest results cover **698 passing tests** without
  counting the focused reruns twice. The two existing ignored application tests
  require manually prepared live-model proof directories.
- `cargo clippy -j 2 -p tetonic-runtime -p tetonic-orchestrator --all-targets --
  -D warnings`: **passed**. The runtime test helper now names its callback type;
  the soak test explicitly drops its join handle, preserving its existing detach
  behavior. The changed soak test also passes its focused rerun.
- Package verification passes formatting, architecture, and static quality
  checks, but **still fails Clippy**: two large managed-runtime enum variants and
  a needless borrow in managed cancellation. These are existing findings; the
  package check stops before establishing a clean downstream baseline. Previously
  recorded server lint findings also remain unaddressed.

Local, ignored verification logs: `.lokai/current-contract-tests.txt`,
`.lokai/current-contract-tests-all.txt`, `.lokai/current-contract-app-lib-final.txt`,
`.lokai/current-contract-clippy.txt`, `.lokai/current-contract-soak-final.txt`, and
`.lokai/current-contract-package-final.txt`. `git diff --check` also passes.

## Delivery implications

This closes a fleet locking defect and repairs checks left behind by previous
architecture changes. It does not close the sprint's end-to-end product,
usefulness, recovery or release criteria. No dependency, database schema, HTTP
contract, gate suppression, or quality allowlist is changed. The running UI/server
has not been restarted.

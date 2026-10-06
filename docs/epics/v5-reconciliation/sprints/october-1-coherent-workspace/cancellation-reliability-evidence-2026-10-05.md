# Cancellation outcome reliability — October 5, 2026

This follow-up addresses the intermittent parent-revocation failure recorded during the October 5 commit checks. It changes the existing managed runtime, not the agent/tool permission envelope.

## Reproduction and correction

A deterministic regression forces run cancellation to reach the supervisor journal before the child finalizer attempts its terminal command. Before the fix, finalization returned `PersistenceFailed("run not accepting commands: Canceled")`. The asynchronous submission owner could publish that rejected transition as a failed worker, even though cancellation had already committed.

The finalizer now reconciles an error against the authoritative run snapshot. It returns cancellation only when the journal records run cancellation and this exact attempt is canceled. It still drains workers and records quiescence before releasing ownership or delivering completion. A local stop flag is insufficient: a negative regression preserves the error and ownership when cancellation has not been durably recorded. Completed attempts are not reclassified by this reconciliation.

The submission owner also avoids emitting a second failure after another terminal owner has removed the attempt binding and delivered completion. Blocking-effect and cooperative-verifier tests now require a canceled outcome after committed cancellation, while retaining their existing checks that running workers are joined and effects cannot continue after the stop boundary.

The original application-level parent/child test retains its cancellation and unknown-spend assertions, with diagnostic outcomes added. This does not refund uncertain inference usage or extend permissions, deadlines, or retry budgets.

## Validation

- The new forced-order regression fails before the fix and passes after it.
- Full `tetonic-run` test suite: **106 passed**, including **36 managed-service integration tests**.
- Application library tests: **206 passed, 2 opt-in tests ignored**. This includes the previously intermittent revocation test and delegated usage checks.
- `cargo fmt --all -- --check`: **passed** after repository-wide rustfmt cleanup. Formatting is recorded separately from the runtime correction.
- Full package engineering gate: **still failing**, independently of the fixed formatting check. It reports Clippy failures in `core/tetonic-domain/src/engine_config.rs` (four derivable defaults and one collapsible conditional), the 936-line `local_workspace.rs` size finding, and the model-literal finding in `local_workspace/tests.rs`. These remain follow-up work; no gate is disabled or marked passing.

Commands: `cargo test -p tetonic-run`, `cargo test -p tetonic-run --test managed_service_tests`, `cargo test -p tetonic-app --lib`, `cargo fmt --all -- --check`, and `cargo run -p tetonic-arch-gate -- verify package`, all from `engine/`.

## Scope

This improves cancellation reporting and preserves runtime cleanup guarantees. It does not implement external connectors, tool-capable team execution, durable parking, or distributed cancellation guarantees. The running local preview is not restarted by this change.

# Engineering gate cleanup — October 5, 2026

Follow-up to the cancellation reliability check. This is a bounded maintenance slice; it does not close the engineering gate or a product sprint.

## Completed

- Extracted local owner startup, default-agent registration, and managed host setup into `engine/litho/tetonic-app/src/local_workspace/bootstrap.rs`. The methods are unchanged. The parent workspace module now has 739 lines, below the existing 900-line limit.
- Moved the existing test-only workspace module to `local_workspace/tests/mod.rs`. Its contents and `#[cfg(test)]` parent declaration are unchanged. The gate recognizes the standard test directory, so fixture model names no longer look like production routing defaults.
- Derived four configuration defaults with the same default variants and simplified string unquoting without changing the length guard.
- Replaced two `Option::is_none_or` calls in memory with equivalent `map_or(true, ...)` calls. This removes those incompatibilities with the crate's declared Rust 1.80 minimum; it does not establish whole-workspace support on that compiler.
- Moved four inline test modules after production items and named two storage-row tuple types. No storage schema, authorization, budget, or cancellation semantics changed.

No lint suppression, allowlist expansion, or gate implementation change was added.

## Verification

`cargo test -p tetonic-domain -p tetonic-sandbox -p tetonic-memory -p tetonic-app -p tetonic-arch-gate --lib`:

| Crate | Passed | Ignored |
|---|---:|---:|
| tetonic-app | 206 | 2 |
| tetonic-arch-gate | 89 | 0 |
| tetonic-domain | 35 | 0 |
| tetonic-memory | 168 | 0 |
| tetonic-sandbox | 15 | 1 |
| Total | 513 | 3 |

The ignored cases are existing opt-in fixtures. The unchanged moved test bodies and bootstrap methods were also compared against the previous commit. `git diff --check` passed.

`cargo run -p tetonic-arch-gate -- verify package` passes formatting, architecture, and quality-static checks. **The overall command still fails** `QG-CLIPPY-001`. Current diagnostics are 20 `too_many_arguments` findings in memory and one `match_like_matches_macro` finding in inference (`engine/atmos/tetonic-inference/src/decoupled.rs:31`). Clippy stops failing crates early, so this is not a complete inventory of downstream warning debt.

## Remaining API cleanup

These memory entry points need focused request/parameter types, with their existing scoped authorization, idempotency, transaction, and lease checks preserved. Keep those changes separate from this mechanical maintenance slice. Do not bundle positional arguments into anonymous tuples just to satisfy the lint.

| Memory source | Methods currently reported |
|---|---|
| `context_publication.rs` | `publish_context_message` |
| `delegated_grants.rs` | `require_delegated_execution_binding` |
| `human_controls.rs` | `propose_effect_approval`, `resolve_effect_approval`, `record_team_effort` |
| `team_work.rs` | `create_team_work_item`, `create_team_work_item_with_input`, `create_team_work_item_for_purpose`, `activate_from_cursor`, `create_work_delegation` |
| `work_briefs.rs` | `save_work_brief` |
| `work_usage.rs` | `begin_work_inference`, `begin_work_inference_with_limit` |
| `huddle_plans.rs` | `save_huddle_plan` |
| `plan_human.rs` | `ask_work_human`, `answer_work_human`, `amend_plan_assignment` |
| `huddle_execution.rs` | `begin_huddle_execution` |
| `workstation_placement.rs` | `enroll_workstation`, `claim_worker_assignment` |

After those changes, rerun the memory and application regressions, then the unmodified package gate to discover any further findings. External connectors, tool-capable team execution, and durable park/resume remain separate product work. The local preview process was not restarted.

# BASE-003 — Consolidate host construction and configuration

Date: October 8, 2026. Status: complete.
Scope: step 3 of the architecture tidy-up, based on `611f0c51`.

## Delivered

- Added the [application host boundary](../../../../architecture/host-configuration.md)
  for the local UI and registered job launcher. Control, credentials, resources,
  runtime and compute now share one `SharedStore` writer and reader pool within
  the composed host instead of reopening that database at each layer.
- Moved application constructors into `host/composition.rs` and consolidated
  their service wiring. Production bootstrap and injected constructors retain
  the existing managed run service, supervisor, approvals and policy services.
- Moved runtime initialization into `host/initialization.rs`. Artifact storage
  is configurable; a failure to acquire the configured policy-store reader
  now returns an error. Individual policy-setting defaults remain unchanged.
- Replaced `product_submit::TurnBind` with `host::HostServices` and updated its
  callers. Existing inference/provider, broker, runtime and egress attachment
  behavior remains behind the same application operations.
- Added validated storage, sanitized logging and trace configuration. `tetonic
  ui --host-config` accepts JSON; `tetonic job --host-settings` accepts the same
  object under `host`. New directory fields resolve relative to that file.
- Command adapters own diagnostics installation and the buffered-log flush
  guard. Library construction does not install a global subscriber. Both
  configured sinks use the existing safe formatter and trace gate.
- Updated architecture ownership, construction/configuration documentation and
  the local UI contract. Updated source-location architecture checks to follow
  the moved composition and added mutants proving missing boundaries fail.

## Preserved boundaries

This changes construction, not work orchestration or execution authority. The
same resource admission, agent revisions, grants, managed lifecycle, approvals,
runtime tool mediation, inference broker and egress guard remain in use.
Existing UI/API payloads and existing CLI invocations are preserved. There are
no web changes, schema migrations, replacement controllers or new dependencies.

The legacy default artifact location remains compatible, including the temporary
directory fallback when the database is a bare filename. An operator changing
the artifact directory must preserve/migrate existing payloads separately.
Configured artifact/log directories do not grant file tools access to them.

## Validation

Lean offline builds used the existing cleanup-worktree target cache. The live
manual-testing engine and its database were not restarted or modified.

| Check | Result |
|---|---|
| `cargo run -p tetonic-arch-gate --offline --locked -- verify package` | Passed: formatting, workspace/all-target Clippy with warnings denied, architecture and static quality. |
| `cargo test -p tetonic-app --lib --offline --locked -- --test-threads=1` | 158 passed, 4 ignored. Includes six new host checks, parallel teams, provider/tool/MCP paths, exact approvals, human-wait reconstruction and killed-process recovery. |
| `cargo test -p tetonic-app --lib --offline --locked host::tests -- --test-threads=1` | Seven passed after final review added the bare-database-filename compatibility regression; the engineering gate passed again. |
| `cargo test -p tetonic-cli -p tetonic-telemetry --offline --locked -- --test-threads=1` | 14 CLI tests (including six separate-process journeys) and 29 telemetry tests passed. |
| `cargo test -p tetonic-app --offline --locked --test comp01_pins --test work02_execution --test work03_door --test workfin01_finalization --test workfin02_terminal -- --test-threads=1` | 91 boundary/contract tests passed. Many are source assertions, not substitutes for behavioral execution tests. |
| `cargo test -p tetonic-arch-gate --lib --offline --locked -- --test-threads=1` | 89 passed, including the five missing-composition-boundary mutants. |
| Documentation and diff checks | Relative Markdown targets/heading fragments resolve; `git diff --check` passes; no `web/` diff. |

New host coverage checks actual shared-writer queuing, configured artifact
placement, relative-path handling, rejected unsupported/authority settings,
validation before database creation, sanitized and flushed file diagnostics,
and explicit failure for an unusable log destination.

The four ignored application cases retain the previous baseline meaning: three
opt-in local-model/scenario fixtures and the child-process helper invoked by its
parent test. No paid inference, browser journey or full Rust workspace test run
was performed for this construction refactor.

## Limits and next boundary

Supported configuration is deliberately limited to implemented local storage and
diagnostics. This does not implement distributed storage, a Keeper service,
remote agent execution, OTLP export, live config reload or global process
coordination. Offline `control`/`estate` adapters retain their current defaults;
the older domain `EngineConfig` schema remains unwired to these product commands.

Step 4 is explicit application scope and work-service extraction, following the
[ownership map](../../../../architecture/ownership.md). It has not started here.

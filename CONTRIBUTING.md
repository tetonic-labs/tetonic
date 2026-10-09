# Contributing to Tetonic

Tetonic has one connected map workspace and one managed execution path. Start
with the [current architecture](docs/architecture/README.md), then find the
existing owner in the [ownership map](docs/architecture/ownership.md). The
[terminology guide](docs/architecture/terminology.md) explains why a saved agent,
work item, task, attempt, security team and work team are different records.

## Start a contribution

1. Follow the [local preview setup](README.md#try-the-local-preview). Use a
   separate database and workspace folder for development experiments. Do not
   point tests or a second execution host at someone's running workspace.
2. Trace the user action through the local API, application service and durable
   record before editing. Extend that owner instead of adding a parallel service.
3. Test the changed behavior at its boundary, including rejection or recovery
   where relevant. Most fixtures use temporary stores and fake providers; a paid
   model account is not required to run them.
4. Run the applicable checks below. Update current contracts and owner links
   with the code. Record what ran, what was skipped and any remaining limits.

The engine's Earth-themed folders group packages; they are not a strict
dependency hierarchy. Cargo manifests and the ownership map describe real edges.

## Where a change belongs

| Change | Start here | Useful behavior checks |
|---|---|---|
| Agent identity, saved configuration, grants, MCP or skills | `tetonic-app` resources and scoped workspace services | App library tests; web `agent-editing`, `agent-capabilities`, `mcp-connections`, `workspace-skills` |
| Shaping, plans, assignment or human intervention | `tetonic-app` work services and existing team-work controller | App library tests; web `team-work-shaping`, `team-plan`, `human-handoff` |
| Admission, cancellation, attempts or final results | `tetonic-app` execution boundary and `tetonic-run` managed services | App `work02_execution`, `work03_door`, `workfin01_finalization`, `workfin02_terminal`; managed-service tests |
| Durable data, migrations or budgets | `tetonic-memory` control/execution/context/usage/artifacts and their application callers | Memory tests plus affected app tests; web `usage` for presentation |
| Tool effects or provider transport | `tetonic-runtime`, `tetonic-tools`, `tetonic-sandbox`, `tetonic-egress`, `tetonic-inference` | Relevant package tests; app `cap01_capabilities`, `scope04_boundaries`, `approval_binding_regressions` |
| Map, inspector or interaction | Existing `web/src/components/team-work` and pure engine projections | Web `map-work-clarity`, `map-layout`, `team-work-engine`, `local-context`; visually inspect changed interactions |
| Local HTTP contract | CLI adapter, app request/response owner and `web/src/engine` | CLI tests; web `engine-client`; update [local UI v1](docs/implementation/contracts/local-ui-v1.md) |

The [frontend guide](docs/architecture/frontend-boundaries.md) separates wire
types, transport, projections, cached server reads and local interaction state.
Keep the existing map/inspector/composer experience unless the task calls for a
product change. Preview fixtures are not a disconnected-mode fallback.

## Boundaries to preserve

- **Authority:** verified resource services decide access. IDs, saved preferences
  and team membership are not substitutes for current grants and host limits.
- **Execution:** submit through registered, managed execution. Do not create a
  second supervisor, claim owner, completion journal or UI-owned run state.
- **Effects:** retain capability checks, controlled egress, process isolation
  and transactional paths where supported. Review the actual effect and platform;
  a passing static check is not a universal safety guarantee.
- **State:** stores own transactional integrity; application services own use
  cases. Unknown provider usage is not zero, and browser cache is not authority.
- **Errors and concurrency:** preserve actionable typed errors; avoid casual
  panics, blocking I/O on async workers, and new coarse store locks. Use existing
  cancellation and revision fences instead of inventing an independent lifecycle.

## Verify the change

From `web/`:

```sh
npm ci
npm test                 # boundary regression tests and UI/unit tests
npm run build           # type-check and production build with boundary checks
```

During an iteration, `npx vitest run tests/agent-editing.test.tsx` (substitute the
affected suite) or `npm run architecture:check` gives narrower feedback.

From `engine/`:

```sh
cargo test -p tetonic-app --lib  # substitute the affected package/suite
cargo run -p tetonic-arch-gate -- verify package
```

`verify package` checks formatting, workspace/all-target Clippy with warnings
denied, architecture and static quality. It does not run behavior tests.
Use `verify fast --crate tetonic-app` while iterating; `verify full` additionally
runs workspace tests when the scope warrants it. Platform/ignored/live-provider
tests need their stated environment and must be reported separately.

[Quality policy](docs/engineering/QUALITY-GATE.md) explains the checks and their
limits. When adding a guard, test a deliberate violation and a legitimate case.
Update required-owner checks when intentionally moving an owner; do not disable
them to make a missing path look clean. Exceptions need a reason, owner and
removal condition in [quality debt](docs/engineering/QUALITY-DEBT.md).

## Keep documentation navigable

- Update the [current architecture](docs/architecture/README.md) and relevant
  contract when changing behavior, responsibility or public entry points.
- Keep dated audits and delivery records historical. Label superseded proposals;
  do not present planned servers, harnesses or distributed storage as implemented.
- Keep tickets in `docs/epics/<epic>/sprints/<sprint>/`; no root `sprints/` folder.
- Use Tetonic for current packages, binaries and examples. Retained schema keys
  or legacy data directories are compatibility details, not new product names.

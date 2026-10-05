# Sprint 1 implementation baseline

Recorded October 4, 2026. Starting revision: `c1c64eb`, branch `main`. The checkout already contains substantial modified and untracked local-engine and product work from earlier development. It has been preserved. This record distinguishes newly run checks from historical local-preview exits.

## Proposed execution profile

Primary validation profile: Windows, one local owner, loopback UI API, existing SQLite resource/run storage, Tetonic general harness and the installed Ollama `qwen3.5:latest` model. Explicit file tools operate within a selected test workspace. This is a candidate profile until actual-model and boundary checks pass; it is not a packaged release or a claim of arbitrary process isolation.

Hosted agents retain prompt-only compatibility. Workspace-tool disclosure through hosted providers is not supported by the current execution contract. Shell, arbitrary MCPs, remote workers and shared-human sessions are outside this first profile. No paid provider request or model download is required for the baseline.

At startup the machine had approximately 55.5 GB free disk space. Ollama and the local UI ports were not listening. Ollama has since been started with its existing installed models for bounded validation. The running browser URL alone was not evidence of a connected engine.

## Fresh checks before implementation

| Check | Result |
|---|---|
| `npm run build` in `web` | Failed: missing GraphNode type import and unused imports/parameters in the experimental work views |
| `node node_modules/vitest/vitest.mjs run --no-cache` in `web` | Passed: 132 tests in 21 files |
| `cargo test -p tetonic-app --lib local_workspace -- --test-threads=1` in `engine` | Passed: six focused local-workspace tests |

These tests do not prove the complete browser journey or actual model usefulness. Fixture-based engine tests are recorded separately from real inference.

## Initial implementation findings

- Empty tool selection was sent as an omitted field and expanded to the host's default tools. Fix at both the UI and creation API; reject unavailable tools rather than silently changing a grant.
- Connected setup offered shell/recall and hosted file-tool choices that the runtime cannot honor. Project host capabilities into setup and preserve the existing hosted disclosure boundary.
- `tetonic ui` implicitly granted its current directory when no workspace was specified. Require an explicit folder argument for filesystem access.
- Approval UI showed success before acknowledgement and called both its own resolver and its parent's resolver. Give the mutation one owner and await the engine receipt.
- The approval API exposes a digest without inspectable effect details. Disable approval of that incomplete representation; an opaque digest is not a command or evidence of consent.
- Main UI lifecycle, example fallback, disconnected map activity and duplicate work surfaces still require consolidation. Passing the current component tests does not close OCT-103 through OCT-105.

## Outstanding baseline evidence

Actual file-based model execution, current builds, application/storage tests and architecture checks are now recorded in the [October 4 evidence](evidence-2026-10-04.md). The Windows symlink tests skipped for missing privileges; those cases are not verified. Clean-install packaging, browser journey consolidation and fresh-user usability evidence remain open. Pilot participants for sprint 3 have not been arranged; no outreach has been sent. The source launcher is not a release installer. OCT-101 stays in progress until its missing evidence and pilot arrangements are resolved; implementation can proceed independently.

The real run also exposed truncated task input: restored tasks and subsequent conversation turns used a shortened title instead of the full accepted request. Schema 52 now stores immutable input on the existing team-work row through ResourceService. Focused tests cover retry mismatch after the shortened prefix, reopening, and full history. Legacy rows without stored input retain their old title; the original missing text cannot be reconstructed by this migration.

## Product review rebaseline October 4

Source review at `7ed2a1d` plus the substantial modified/untracked tree. This section supersedes only the earlier UI/team scheduling assumptions; it does not erase earlier evidence. The current request changes planning documents, not application behavior. No new runtime checks were run for this planning revision.

| Area inspected | What exists in the current tree | Consequence for the revised sprint |
|---|---|---|
| [App](../../../../../web/src/App.tsx), [Workspace](../../../../../web/src/components/workspace/Workspace.tsx) | Ordinary route uses LocalEngineProvider and one map/composer/focus shell; starting work stays on the map | Keep this integration; do not build another shell |
| [workspaceRecords](../../../../../web/src/lib/workspaceRecords.ts) and [WorkspaceMap](../../../../../web/src/components/workspace/WorkspaceMap.tsx) | Work is grouped by task conversation `parent_id`; nodes use the latest single agent and run state | Useful for the solo slice, insufficient for goals, delegated children, dependencies and shared contributions |
| [WorkspacePanels](../../../../../web/src/components/workspace/WorkspacePanels.tsx) | Actual agent setup/inspection, teams readout, incomplete-effect approvals disabled | Reuse forms/readers; add permitted team assignment and complete human flag/effect projections, not browser-only team mutation |
| [team_work resources](../../../../../engine/litho/tetonic-app/src/resources/team_work.rs) and [storage](../../../../../engine/strata/tetonic-memory/src/team_work.rs) | Goals, work, huddle acceptance, park/resume, cursor activation and parent/child delegation records already exist | Extend these contracts/readers; preserve durable identity and avoid a parallel work database |
| [team_work_activation](../../../../../engine/litho/tetonic-app/src/resources/team_work_activation.rs) | Existing activation uses registered jobs, honors stops/placement, checks a parked parent and applies a child reported-token ceiling | Reuse this path; a per-child ceiling alone does not prove shared allocation/accounting across children |
| [managed admission](../../../../../engine/mantle/tetonic-run/src/managed/admission.rs) | Scoped parent attempts are explicitly rejected with `governed delegation is not configured` | COORD-A is real integration work. Do not remove the guard until inherited authority, allocation, stop and recovery contracts are enforced |
| [local workroom projection](../../../../../engine/litho/tetonic-app/src/local_workspace/workroom.rs) | Local notes can overlay status, lead and agent IDs; contributor metadata is not execution evidence | Replace authoritative-looking overrides with recorded assignment/lifecycle fields before using them for the team map |
| [team participation](../../../../../engine/strata/tetonic-memory/src/team_admin.rs) and [human controls](../../../../../engine/litho/tetonic-app/src/resources/human_controls.rs) | Existing scoped participation and control resources are available to integrate | Reuse privacy/control authorities; a durable bounded exchange and delivery contract is still required on the supported product path |

Earlier in this implementation session, the solo workspace passed a 153-test frontend suite before the last design change. After that change, the build and 14 focused workspace/context tests passed. Developer browser checks on the isolated local `qwen3.5:latest` profile observed a document comparison, a context-preserving follow-up, reload persistence and a stopped test task. The initial comparison work was `6fceb516-60ba-42c0-ada5-764da5148aa4`; the stopped task was `c94626f7-a403-4f32-bb48-879bd55d768c`. These are historical dirty-tree checks, not re-run results for this revision, human usability evidence, or a complete engine-restart/team proof. The last browser session did not finish all required viewport/zoom/first-use checks. No coordination gate is verified by these observations.

Keep: authoritative solo submission, retry identity, scoped drafts, last-known data handling, actual result inspection, camera/portraits and acknowledgement behavior. Rework: conversation-centered grouping, single-assignee-only activity and generic failure badges as a substitute for human flags. Add through existing owners: real assignment/dependency projection, governed collaboration, durable exchange receipts/limits and safe attention delivery. Defer: optional customization, new filters/shortcuts, visual polish and broad cleanup. The next implementation begins with the work/coordination contracts and COORD-A, not another copy or theme pass.

# Team-view cutover — October 5, 2026

Status: implemented UI cutover onto existing local engine capabilities. This
does not close the autonomous-team sprint, accepted-plan dispatch or COORD gates.

## One product surface

The user explicitly requested removing older views and wiring the engine into
the agreed team-work design. The production entry now mounts `TeamWorkspace`.
The existing `/dev/team-work/` URL loads that same entry, as does `/`. Engine
connection URLs, old `#work=` links and shaping links continue into this surface.

Removed 28 obsolete view components, the old preview app and interactive example
controller/data, and 11 retired stylesheets. This includes `Workspace`, its old
map/composer/focus/panels, `LocalWorkspace`, `MissionDeck`, `Workroom`,
`DirectorExperimentView`, `TeamRoom`, the old graph/activity/resource views,
and the sample-only agent/team/tool management views. Generic components,
integration adapters and reusable tested layout/motion code were retained.
No engine data was deleted, no repository was reset, and no changes were pushed.

Before removal, the uncommitted UI source and tests were archived locally at
`.lokai/ui-retirement-2026-10-05/before-retirement.zip`. The verified file inventory
and exact removal list are in that directory. They are recovery artifacts, not
an alternative UI. Thirteen old-view test files were retired with their removed
targets; brief-conflict/retry, lineage, credential and layout coverage was
preserved or migrated. Active integration coverage tests the sole current UI.

## Integration and reuse

| Surface | Engine source / existing boundary | Behavior |
|---|---|---|
| Work map | Workspace snapshot, work-items, registered agent profiles | Real requests grouped by recorded goal reference or connected team; conversation replies are one work item, not dependency edges |
| Floating composer / work inspector | Existing LocalEngineProvider and useWorkspaceDraft | Submit, follow up, reconcile unknown sends with stable IDs, cancel and inspect acknowledged state |
| Shape work | Existing Guide, exploration-purpose run, WorkingBrief | Saved discussion and revisioned brief; no execution from saving |
| Agents | Existing LocalAgentSetup and registered-agent API | Installed/provider model selection, consent/grants, actual agent registration and inspection |
| Teams | Authorized local team reader | Actual connected team and available agent roster; no pretend membership editor |
| Blackboard | Authorized task messages and accepted output | Returned original requests/responses/tool results grouped by discussion and turn; no fabricated global chronology |
| Find work | Same authorized snapshot projected into workContext | Literal source retrieval and links; no fake model answer |
| Tools & MCPs | Host catalog and actual agent tool profiles | Show actual availability/grants; no invented MCP connections or destination activity |
| Needs you | Existing approval/stop reader and resolution API | Inspect available request metadata, decline; block approval if action details are missing |

The existing map, camera, portraits, type, colors, inspector and dark floating
composer are reused. The misleading legacy work adapter that manufactured
milestones and claimed verified delivery was removed. Stopped work remains
stopped. Tool permission does not imply tool activity. Example fixtures used for
layout tests moved under tests; production bundle validation excludes them.

## Validation

- Automated cutover checks ensure both HTML entries mount the same application
  and named old shells no longer exist.
- Integration tests cover real API contracts for send/reply/deep-link restore,
  unknown-send identity through navigation, validation failures, acknowledged
  stopping, disconnection, approval denial, registration, tools and blackboard.
- Projection tests cover idle-agent separation, explicit goal grouping, terminal
  state, and the absence of invented dependencies/destinations.
- Working-brief conflict and uncertain-save recovery tests are retained on the
  current surface. Existing scope/tool permission and map layout tests remain.
- A browser request through the team-view composer ran against local
  `qwen3.5:latest`, completed, and returned an actual workshop comparison. The
  saved result was reopened at `/` with its work ID, proving it uses the same
  engine state and interface. No example playback or hard-coded answer was used.
- Final frontend verification: **92 tests passed across 19 files** with
  `node node_modules/vitest/vitest.mjs run --no-cache`. This includes terminal-state,
  map layout and credential-rotation isolation regressions.
- `npm run build` passed, including TypeScript and the production-import guard.
- Browser verification at 390px showed readable work details and no horizontal
  overflow. The viewport override was reset. The desktop map was checked after
  the avatar/title spacing adjustment and left open in the existing browser tab.
- Local visual evidence: `.lokai/ui-consolidation-check/team-work-live-2026-10-05.png`.
  This shows the real connected workspace, with four recorded work items and two
  registered agents; it is not evidence of multi-agent dispatch.

## Deliberate remaining boundaries

The current local API does not supply dependency graphs, structured active tool
destinations, arbitrary team/membership management, MCP connection management,
skills, a model-backed work-context tool or accepted-plan multi-agent dispatch.
Those remain engine integration work; no UI success is fabricated for them.
The transcript reader remains bounded to 100 records per run, and is not a
shared-room privacy boundary. Work that finished is not labeled independently
verified. This change introduces no new scheduler, orchestration service,
permissions or execution backend.

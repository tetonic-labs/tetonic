# Persistent work teams — 8 October 2026

## Product slice

An owner can create a named team from saved agents, explain its purpose, edit its roster, and choose **Work with this team**. This returns to the existing map composer with that team selected. The Guide shapes the request and proposes assignments from that roster. The owner still reviews and starts the plan. Small questions can receive direct answers without dispatch.

The team editor lives in the existing inspector. Agents are references to saved definitions: joining a team creates no agent copies, changes no model or harness, and grants no tools, MCP connections, skills, private context, or budget. Existing agent and workspace limits still apply.

## Integration

- `tetonic-memory::work_teams` adds schema 68: immutable roster versions and work-to-roster bindings within the existing organization/security-team scope. Saves use expected revisions and durable request receipts. Work creation and its roster binding commit together in the existing Store transaction.
- `ResourceService` authorizes reads and writes through existing `ReadTeam`/`ManageTeam` checks. The current local adapter permits the workspace owner to edit rosters. Security-team principal membership is unchanged.
- `LocalWorkspace` exposes saved rosters in the workspace snapshot; authenticated `GET/POST /api/local/work-teams` lists/saves them. A new Guide request can specify `work_team: {id, revision}`. Follow-ups inherit the accepted roster; conflicting or stale selections cannot silently retarget a request.
- Guide observations and explicit plan generation use the saved roster. Plan save/capture and execution admission reject assignments outside it. The existing HuddlePlan, agent revision pins, delegated grants, resource reservations, TeamWorkController, and managed executor remain the execution owners. No second dispatcher or fleet runtime was introduced.
- Continuation creation copies the original roster binding in the same transaction as the continuation. Editing the reusable team affects new discussions, not an existing plan, parked work, or its continuation.
- Task projections carry the accepted team snapshot. Separate efforts group under its team identity on the map, with stable functional area colors. Recorded assignments determine activity; current roster membership is not fabricated running work.
- Browser drafts retain both the request ID and selected roster revision through uncertain sends. The response must confirm that team selection before the UI accepts it.

## Verification

- Memory suite: 180 tests passed, including migration failure/crash recovery, roster persistence, stale edits, retry identity, scope denial, unchanged agent definitions, and out-of-roster plan rejection.
- Local workspace suite: 52 passed, 3 explicitly opt-in live-model demonstrations skipped. Includes Guide request/follow-up persistence across restart, parallel agents, a human-waiting worker alongside a completing sibling, roster edits during that wait, mixed-provider tools, and continuation behavior.
- Web: 194 tests passed; TypeScript and production build passed. New coverage creates/edits a team, selects it on the map, retries an uncertain send after reopening, and groups separate plans without presenting current members as historical contributors.
- Authenticated HTTP create/edit/list/snapshot and exact retries verified against a disposable server/database on port 3088. No inference was started in that HTTP proof; its disposable server was stopped after verification.
- Browser create/select/save/profile/composer flow checked using an explicitly labeled isolated design fixture. Preview image: `.lokai/manual-testing/saved-team-profile-2026-10-08.png` (ignored local evidence).
- Strict Clippy passed for memory, app and CLI, including all targets. Architecture and quality gates passed. Production bundle still exceeds Vite's advisory 500 kB chunk threshold.

## Boundaries

This slice does not add organization membership administration, team deletion/archive, a new team budget policy, native vendor harness integration, or autonomous approval. Rosters support 1–24 saved working agents; a proposed plan still supports up to 12 assignments. Adding a team does not promise that every member will be used for every request. Real dependencies, saved agent capacity, permissions and budgets still constrain concurrency.

After the owner explicitly requested a restart, the port-3004 manual-testing engine was backed up and upgraded successfully. The authenticated workspace exposed the new team API; all 27 task IDs and four saved agent definitions were retained. The existing port-5177 browser connection was refreshed and its project view restored.

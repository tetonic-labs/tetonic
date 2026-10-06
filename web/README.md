# Tetonic team workspace

There is one product UI: `src/components/team-work/TeamWorkspace.tsx`.
Both `/` and the compatibility address `/dev/team-work/` load `src/main.tsx`.
Connection links and saved `#work=` / `#shape=` links open this same interface.
The older workspace, workroom, mission deck, director experiment and preview
shells have been removed. Do not add a second UI or restore an example fallback.

## Run locally

```sh
npm ci
npm run dev -- --host 127.0.0.1
```

From `engine`:

```sh
cargo run -p tetonic-cli -- ui --database ../.lokai/ui/workspace.db --model <installed-ollama-model>
```

Open the connection URL printed by the engine. Its credential is removed from
the address and held in tab session storage. A server restart requires its new
connection link. `--ui-origin` can select another Vite origin. File access needs
an explicit host `--workspace-root`; opening the UI does not grant it.

## Connected experience

- The map projects authorized engine work, conversation lineage and registered
  agent identities. An existing `goal_id` groups work; replies are not dependency
  edges. Without a goal reference, work belongs to the connected team.
- The floating composer submits actual work to the chosen registered agent.
  The inspector exposes responses, recorded tool output, run identifiers,
  follow-ups and acknowledged cancellation. Unknown sends retry their original ID.
- Shape work opens the registered Guide and durable revisioned working briefs.
  Saving a brief does not authorize execution or dispatch assignments.
  Its Work plan tab generates a real proposal from the saved brief, shows tasks,
  proposed agents, dependencies and effort, and supports revision and agreement.
  Proposals use existing huddle records. Agreement does not start team execution.
- Agents supports actual model discovery, registration and profile inspection.
  Teams reads the connected team. Needs you reads approvals and failed/interrupted
  work. Requests without inspectable proposed effects cannot be approved.
- Blackboard renders the returned owner-workspace transcript in conversation/turn
  order. The current reader returns up to 100 records per run; this is neither an
  unlimited audit export nor a shared human room. Find work searches these same
  records with links back to the source; it is not a generated status answer.
- Tools & MCPs shows the host tool catalog and recorded per-agent tool profiles.
  No sample MCP connector or local setup draft is presented as a connected service.
- Disconnection preserves last-seen records, marks the loss, and disables sends.
  It never substitutes sample agents or activity.

## Engine gaps kept explicit

This connection does not yet expose team creation/membership editing, MCP
connection management, skills, or structured live tool destinations. Agreed plans
can dispatch bounded local contributors using supplied context and written results;
file tools, hosted models, and MCP tools are unavailable on this team path. The map does not invent dependency graphs, external-service
activity, or agent collaboration to fill those gaps. A completed run means the
execution finished, not that the result was independently verified.

See [the local UI contract](../docs/implementation/contracts/local-ui-v1.md) and
[cutover evidence](../docs/epics/v5-reconciliation/sprints/october-1-coherent-workspace/team-view-cutover-2026-10-05.md).

## Verify and maintain

```sh
npm test
npm run typecheck
npm run build
```

`team-work.css` preserves the agreed map/inspector/composer design and `brand.css`
provides common tokens. `teamWorkspace.ts` projects authorized engine state.
`LocalEngineContext`, `localEngine`, `workspaceRecords`, `WorkingBrief`,
`useWorkspaceDraft`, `LocalAgentSetup`, `ProjectMap`, `useMapCamera` and `Portrait`
reuse existing integration and interaction code.

The production build rejects imports from `dev`, tests, sample stores and retired
shell names. Authenticated `/api/local` remains the engine boundary. Do not use
the legacy unauthenticated fleet API as a shortcut.

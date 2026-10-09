# Tetonic team workspace

The connected product is the map workspace in
[`TeamWorkspace.tsx`](src/components/team-work/TeamWorkspace.tsx). Both `/` and
`/dev/team-work/` load the same `src/main.tsx` entry. Saved work, project and
shaping links open that interface. Do not restore retired shells or substitute
example data when the engine is unavailable.

## Run locally

From this directory:

```sh
npm ci
npm run dev -- --host 127.0.0.1 --strictPort
```

From `engine/`, using an installed Ollama model for the default agents:

```sh
cargo run -p tetonic-cli -- ui --database ../.lokai/ui/workspace.db --model YOUR_INSTALLED_MODEL
```

Open the connection URL printed by the engine. Its credential is removed from
the address and held in tab session storage. A server restart requires its new
connection link. `--ui-origin` selects another Vite origin. Host file access needs
an explicit `--workspace-root` and the relevant saved agent grants; opening the UI
does not grant it. Hosted provider configuration is available in agent setup.
See the [repository setup](../README.md#try-the-local-preview) and
[host configuration](../docs/architecture/host-configuration.md).

## What the workspace represents

- Agents have persisted identities, configuration revisions, model selections,
  limits and selected capabilities. Editors use engine catalogs and resource
  services; browser selections cannot override host policy.
- Teams reuse registered agents. Shape work collects intent in a conversation
  and revisioned brief, then supports proposing, revising and agreeing a plan.
  Agreement and starting execution are separate actions.
- Bounded team execution uses selected registered agents, their configured
  providers and granted tools. Independent assignments can run concurrently;
  dependencies and limits constrain dispatch. This is not an always-running
  autonomous team service.
- The map, work inspector, Needs you and usage views project actual engine
  records. A completed execution is not an independent correctness verdict.
- Tools & MCPs and workspace skills expose the current capability catalog,
  connection/skill management and agent grants. A listed capability is not
  automatically granted, connected or available under every host policy.
- Blackboard displays recorded conversations. It is a bounded reader, not an
  unlimited audit export or a shared multiplayer chat room.
- Connection loss retains last-seen records with a visible failure and disables
  work submission. Partial read failures remain distinguishable from fresh data.

The local host still bootstraps an owner and defaults. Multi-user organizations,
replicated deployment, native vendor harness management and automatic ongoing
responsibilities remain separate work. See the
[local API contract](../docs/implementation/contracts/local-ui-v1.md) for details.

## Code navigation

| Area | Owner |
|---|---|
| Wire types / HTTP / connection state / failures | `src/engine/{contracts,client,connection,failure}.ts` |
| Pure agent, record, task-state and map projections | `src/engine/projections/` |
| Cached reads, polling and freshness | `src/context/LocalEngineContext.tsx` |
| Map, inspector, composer and work interaction | `src/components/team-work/` |
| Existing agent, tool and workspace editors | `src/components/views/`, `src/components/work/`, `src/components/workspace/` |
| Presentation helpers and URL selection | `src/lib/` |
| Brand and map styling | `src/brand.css`, `src/components/team-work/team-work.css` |

Read [frontend boundaries](../docs/architecture/frontend-boundaries.md) before
changing integration or state ownership. `engine/contracts.ts` is handwritten
and must be kept aligned with Rust and contract tests.

## Verify

```sh
npm test
npm run build
```

`npm test` runs the architecture regression tests and Vitest suites. Build runs
TypeScript and Vite, including import-graph and bundle boundary checks. Use
`npm run architecture:check` or `npm run typecheck` for narrower feedback.
These checks cannot replace visual inspection of changed interactions.

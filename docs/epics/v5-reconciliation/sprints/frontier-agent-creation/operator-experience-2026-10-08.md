# Operator experience refinement — October 8, 2026

## Product problem

As parallel work grows, a flat record list asks the operator to reconstruct which agent is doing what, which effort it belongs to, and whether it needs intervention. Raw setup fields and disclosure triangles also made agent management harder to scan than the surrounding map.

## Implemented in the existing connected workspace

- Agent roster: portrait, durable role, model and current readiness/activity; search by name, role or model; filters for working, attention and ready. Historical failed work does not permanently label an agent blocked.
- Agent detail: current assignments first, parent effort names for otherwise ambiguous contributions, saved tools/connections/skills, model and per-run allowance, and a short recent-work list. Edit and assignment actions retain their existing engine paths.
- Agent editor: identity and lasting role precede setup; existing tool selection, provider consent, capability checks and save receipts remain intact. The embedded workspace library keeps the draft in place. Core tools, reusable skill instructions and operator-configured MCP servers are explained separately.
- Teams: a selectable team list, connected-workspace roster and direct links to agents/work. Other teams are not given invented members. Team name and membership editing remain unavailable because the current client has only a team-list API.
- Needs you: questions/permissions and failed or blocked work are separate categories. A pending permission is not counted again as a generic waiting-work problem. Question context appears with the answer choices, without repeating the question heading. Exact command, isolation limits, scoped approval and confirmed receipts are preserved.
- Your work: effort-level grouping with search, status filters, recorded contribution counts and agent names. Render the first 30 efforts, then explicitly load more. Finished coordination cannot conceal running, waiting or failed contributions. The map shelf links to the complete list.
- Shared visual signals: blue for active work, amber for human input, red for blocked work, green for completed work. Icons and text accompany every status; colors are adapted for both themes. This is recorded execution status, not an assertion that a human has accepted the result.
- Larger portfolio areas spread over up to four columns instead of one increasingly tall stack. Existing map navigation, project dependency layout and agent motion are preserved.
- Native disclosures use visible Show/Hide labels instead of arrow markers, retaining keyboard semantics. Workspace settings otherwise keep their existing organization.

## Architecture and boundaries

Presentation is split into focused components in `web/src/components/team-work`. `TeamPanels` keeps existing agent create/edit navigation. `workSignals` owns status aggregation and `attentionItems` owns inbox deduplication; obsolete competing journey-status helpers were removed. These consume the existing authorized snapshot, records, approvals and catalog. No Rust runtime, storage migration, external permission or agent grant behavior changed.

This pass does not implement team membership writes, authenticated remote MCP setup, native vendor harnesses or automatic access grants. The currently running local engine predates workspace skill support; its UI correctly disables import/create until an updated engine is running. It was not restarted against live data for this visual pass.

The work list limits initial rendered rows, not server-side history retrieval. Large-scale history pagination, map virtualization and cross-team membership administration remain separate product/engine work.

## Verification

- 183 tests across 28 files passed. Added coverage for completed coordinators with unfinished children, approval/problem deduplication, real planning status, 48-effort searching/filtering/load-more, team roster navigation, and non-overlapping busy portfolio areas.
- TypeScript check, production build, architecture and quality gates passed. Vite continues to report the existing bundle-size warning.
- Browser reviewed with a labeled, isolated fixture containing 12 efforts, 36 contributions and four agents; inspected agent roster/detail/editor, team detail, work filters, human questions and workspace settings. Checked a 430px viewport, dark appearance, and native keyboard expansion. Restored the viewport afterward.
- Fixture data and screenshots remain ignored under `.lokai`; no fake activity was inserted into the connected engine. No real approvals or provider/model requests were submitted.

## Next product gaps exposed

1. Add a governed team-membership read/write contract before offering a real team editor.
2. Distinguish delivered results from explicitly reviewed/accepted results in durable state; the present UI does not invent that distinction.
3. Add history pagination and map level-of-detail once operating volumes exceed what the local snapshot can reasonably return.

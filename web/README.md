# Tetonic workspace preview

A full-screen agent map with floating Teams, Agents, human decisions, and chat.
Agents have distinct portraits. Services are destinations; active interactions
capture agents into an orbit, then release them when an explicit end event arrives.

**This is a local, fixture-backed UI. No engine is connected.** Creating agents,
team membership, decisions, policies and messages change this tab's React state.
They reset on reload. Messages are addressed to a team or individual agent; the
preview never fabricates an agent reply or executes tools. Client-side filtering
is not an authorization or privacy boundary.

## Run and verify

```sh
npm ci
npm run dev -- --host 127.0.0.1
npm test
npm run typecheck
npm run format:check
npm run build
```

Open `http://127.0.0.1:5173/`. The existing `/api` and `/ws` proxy configuration is
retained but unused by this preview. Do not connect the UI directly to the legacy
unauthenticated, in-memory fleet API as a substitute for authenticated integration.

## Try the motion

Use Play at the lower left. The adjacent settings icon offers:

- **Sample trace:** existing fixture trace targets and outcomes, with compressed
  playback timing. Running steps have no invented completion and remain attached.
- **Dock, work & release:** a synthetic sequence through services and a peer handoff.
- **A shared orbit:** three arrivals, an explicit wait/resume, independent releases.
- **Failure, cancellation & redirect:** synthetic interruptions, including during travel.

Previous/Next show semantic states without requiring precise playback timing.
Reduce motion removes drift, travel and spring capture, preserving attachment and
outcome states. The OS preference is honored and cannot be overridden to force
animation. Opening an inspector or switching browser tabs pauses playback.

The bottom composer addresses the selected team or one of its agents. Enter sends,
Shift+Enter inserts a line break. The conversation expands above the composer;
Escape collapses it. Drafts are retained separately for each team/recipient.

## Implementation

- `lib/mapActivity.ts`: projects existing agents, graph destinations and trace targets.
- `lib/graphMotion.ts`: persistent bodies, fixed-step springs, stable orbit slots,
  bounded ambient drift, soft separation, tool satellites and explicit interaction events.
- `lib/motionPlayback.ts`: clearly labeled fixture/synthetic event schedules.
- `components/graph/useGraphMotion.ts`: playback, pause, visibility and reduced motion.
- `components/graph/useMapCamera.ts`: anchored wheel/pinch zoom, drag and keyboard camera.
- `components/graph/TeamActivityMap.tsx`: renders positions and relationships from that state.
- `components/graph/FloatingChat.tsx`: team/agent chat, scoped drafts and local history.
- `App.tsx`: reuses existing agents, teams, approval handlers and detailed inspectors.

`canvas.css` owns the map and floating controls. Existing `index.css`,
`workspace.css` and `brand.css` supply shared components and paper/terracotta/ink
branding. Fonts remain locally bundled Fontsource assets. No new dependency was
added for motion.

See [the motion review](../docs/epics/v5-reconciliation/sprints/mvp-6-product-cutover/ui-review/fluid-motion.md)
for rules, validation and current limitations. This work does not mark the engine
MVP or its authenticated product integration complete.

# Epic: The Village — Living World Experiment

> A 24/7 autonomous digital civilization in the browser, powered by the **Tetonic Standing Agent Engine**, an authoritative **Colyseus 2D World Server**, and a **Pixi.js Web Spectator Client**.

---

## 1. Architectural Mission & Core Axioms

1. **"A Civilization in Motion, Not a Chatbox in a Box"**
   * The world advances on an unpaused physical clock. Agents continuously perceive, reason, navigate, build, and converse without pausing time.
2. **"Physical Agency & The Modifiable Canvas"**
   * Moving beyond clunky, hardcoded state machines. Agents have real creative agency: clearing terrain, constructing buildings, laying desire paths, and managing supply lines.
3. **"Economic Realism via Dual-Process Cognition"**
   * High-frequency physical reflexes run via ultra-cheap, fast local/hosted models ($<200\text{ms}$) with sensory threshold filters (85%+ zero-cost ticks). Deep strategic deliberation (System 2) is invoked on-demand via frontier models.
4. **"The Looking Glass: Controlled Public Interaction"**
   * Site visitors interact through in-world physical mechanisms (citizen petitions, resource shipments at the docks, referendum ballots) rather than direct raw text prompts.
5. **"Clean Domain Decoupling"**
   * The Tetonic Engine remains 100% domain-neutral in its own repository. The game server (`world/`) and spectator client (`web/`) live independently in `the-village` repository, communicating via WebSockets.

---

## 2. Sprint Roadmap

```text
┌────────────────────────────────────────────────────────────────────────┐
│ SPRINT 1: Single-Agent Local Loop & Protocol Contract [COMPLETED]      │
│ - Formalize JSON wire protocol (Perception packets & WorldAction types)│
│ - Implement StreamWorldAdapter TWP binding in Tetonic                  │
│ - Run local single-agent sanity test proving >80% sensory suppression  │
├────────────────────────────────────────────────────────────────────────┤
│ SPRINT 2: Authoritative World Server (Colyseus in the-village/world)   │
│ - Scaffold self-hosted Colyseus server in the-village/world            │
│ - Ingest Tiled map grid & authoritative 2D coordinate system           │
│ - A* spatial pathfinding & Tetonic Gateway WebSocket bridge            │
├────────────────────────────────────────────────────────────────────────┤
│ SPRINT 3: Generative Mechanics & Modifiable Canvas                     │
│ - Tile/structure modification mechanics (place_tile, build, harvest)   │
│ - Resource supply graph (timber, iron, stone, grain)                   │
│ - Physical information propagation & Town Notice Board (Workpad)       │
├────────────────────────────────────────────────────────────────────────┤
│ SPRINT 4: Spectator View Integration (the-village/web Pixi.js Client)  │
│ - Connect Pixi.js client to authoritative Colyseus room state diffs    │
│ - Live Thought Stream Inspector HUD wired to Tetonic SSE endpoint      │
│ - Real-time rendering of dynamic construction, paths, and activity     │
├────────────────────────────────────────────────────────────────────────┤
│ SPRINT 5: The Looking Glass & Live 24/7 Experiment Launch             │
│ - Visitor ingress conduit (Petitions, Dock shipments, Referendums)    │
│ - Founding Trio fleet bootstrap (Mayor, Blacksmith, Scout)             │
│ - 24-hour soak test, safety interlock verification, and public launch  │
└────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Sprint Directory Structure

All sprint tickets strictly adhere to `QG-DOCS-001`:

```text
docs/epics/the-village/sprints/
├── sprint-1-single-agent-local-loop/
│   ├── VIL-101-protocol-wire-specification.md
│   ├── VIL-102-village-world-adapter.md
│   └── VIL-103-single-agent-local-sanity-test.md
├── sprint-2-authoritative-world-server/
│   ├── VIL-201-scaffold-colyseus-world-server.md
│   ├── VIL-202-authoritative-grid-pathfinding.md
│   └── VIL-203-tetonic-gateway-websocket-bridge.md
├── sprint-3-generative-world-mechanics/
│   ├── VIL-301-modifiable-canvas-construction.md
│   ├── VIL-302-resource-economy-supply-graph.md
│   └── VIL-303-spatial-information-notice-board.md
├── sprint-4-spectator-view-integration/
│   ├── VIL-401-pixi-colyseus-room-sync.md
│   ├── VIL-402-live-thought-inspector-hud.md
│   └── VIL-403-dynamic-construction-rendering.md
└── sprint-5-looking-glass-experiment-launch/
    ├── VIL-501-looking-glass-visitor-conduit.md
    ├── VIL-502-founding-trio-fleet-bootstrap.md
    └── VIL-503-24h-soak-test-safety-verification.md
```

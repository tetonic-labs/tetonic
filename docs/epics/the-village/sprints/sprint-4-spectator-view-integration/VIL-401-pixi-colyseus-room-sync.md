# VIL-401: Wire Pixi.js Client to Authoritative Colyseus Room

**Epic:** The Village — Living World Experiment  
**Sprint:** Sprint 4 — Spectator View Integration  
**Layer:** `the-village/web`  
**Status:** Ready

---

## 1. Context & Objective
Replace the mock client-side loop in `the-village/web/src/engine/simulation.ts` with a live Colyseus client connection that subscribes to authoritative room state diffs and renders sprite movements smoothly.

## 2. Requirements
1. Add `@colyseus/schema` and `colyseus.js` client dependencies to `the-village/web`.
2. Connect `web` to Colyseus server:
   * Establish WebSocket connection to `ws://localhost:3001` (or production host).
   * Join `VillageRoom` on load.
3. Bind state diffs to Pixi.js rendering:
   * Listen to entity coordinate changes (`agent.onChange`) and animate sprites using linear interpolation (lerp).
   * Support walk, idle, and work sprite animations based on authoritative action states.
4. Graceful handling of network disconnects and reconnects with visual spectator indicators.

## 3. Acceptance Criteria
- [ ] Mock simulation replaced with live Colyseus room subscription.
- [ ] Sprites move smoothly across the canvas matching server coordinates.
- [ ] 60 FPS rendering performance maintained with multiple moving entities.

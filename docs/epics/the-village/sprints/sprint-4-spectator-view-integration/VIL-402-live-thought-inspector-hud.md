# VIL-402: Live Thought Stream Inspector HUD

**Epic:** The Village — Living World Experiment  
**Sprint:** Sprint 4 — Spectator View Integration  
**Layer:** `the-village/web` & Tetonic Telemetry  
**Status:** Complete

---

## 1. Context & Objective
Connect the frontend Character Inspector drawer directly to Tetonic's `ThoughtStreamHub` zero-compute SSE endpoint (`GET /api/v1/agents/:agent_id/thoughts`), allowing spectators to read live internal monologues with $<100\text{ms}$ latency.

## 2. Requirements
1. Implement SSE connection in the web UI:
   * When a spectator clicks an agent sprite, open an `EventSource` connection to Tetonic's SSE thought stream.
   * Ingest demuxed thinking deltas (`ThoughtDelta`), perception signals, and proposed actions.
2. Render character thought stream:
   * Streaming typewriter effect in the Character Inspector side drawer.
   * Floating, lightweight speech/thought bubble above the agent's sprite in the Pixi.js canvas.
   * Ring buffer catch-up: immediately display the last 10 thoughts upon selecting an agent.
3. Automatically close SSE connection when the inspector is closed to conserve client and network bandwidth.

## 3. Acceptance Criteria
- [x] Clicking any villager opens live SSE stream and displays recent thought history instantly (`ThoughtStreamClient.ts`, `inspector.ts`, `VillageRoom.ts`).
- [x] Incoming token deltas render smoothly with low latency via typewriter effect.
- [x] Zero compute or inference overhead incurred on the server during spectator viewing.

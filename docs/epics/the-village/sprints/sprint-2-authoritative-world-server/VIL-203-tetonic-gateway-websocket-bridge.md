# VIL-203: Tetonic Gateway WebSocket Bridge

**Epic:** The Village — Living World Experiment  
**Sprint:** Sprint 2 — Authoritative World Server  
**Layer:** `the-village/world`  
**Status:** Ready

---

## 1. Context & Objective
Provide a dedicated WebSocket gateway on the Colyseus server that connects directly to Tetonic's `VillageWorldAdapter`.

## 2. Requirements
1. Expose WebSocket route: `/world/gateway` on the Colyseus server.
2. Inbound/Outbound protocol mapping:
   * Periodically compile physical room state into a `Perception` packet matching `VIL-101` schema and broadcast to Tetonic.
   * Ingest `WorldAction` JSON packets received from Tetonic (`move_to`, `place_tile`, `speak`, `post_notice`).
   * Apply validated actions to the authoritative `VillageRoom` state.
3. Support per-agent routing:
   * Allow multiple continuous agents (Mayor, Smith, Scout) to connect via the gateway concurrently or multiplexed through a single connection.

## 3. Acceptance Criteria
- [ ] Tetonic engine connects to `/world/gateway` successfully.
- [ ] World state ticks are emitted at regular cadence (e.g. 1Hz).
- [ ] Agent action packets mutate the physical server room state in real time.

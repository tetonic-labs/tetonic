# VIL-203: Tetonic Gateway WebSocket Bridge

**Epic:** The Village — Living World Experiment  
**Sprint:** Sprint 2 — Authoritative World Server  
**Layer:** `the-village/world`  
**Status:** Complete

---

## 1. Context & Objective
Provide a dedicated WebSocket gateway on the Colyseus server that connects directly to Tetonic's `StreamWorldAdapter` using the universal Tetonic World Protocol (TWP).

## 2. Requirements
1. Expose WebSocket route: `/world/gateway` on the Colyseus server.
2. Inbound/Outbound protocol mapping:
   * Periodically compile physical room state into a `Perception` packet matching `VIL-101` schema and broadcast to Tetonic.
   * Ingest `WorldAction` JSON packets received from Tetonic (`step_move`, `move_towards`, `place_tile`, `speak`, `harvest`).
   * Apply validated actions to the authoritative `VillageRoom` state.
3. Support per-agent routing:
   * Allow multiple continuous agents (Mayor, Smith, Scout) to connect via the gateway concurrently or multiplexed through a single connection.

## 3. Acceptance Criteria
- [x] Tetonic engine / client connects to `/world/gateway` successfully (`TetonicGateway.ts`).
- [x] World state ticks are emitted at regular cadence (1Hz) with proper `StreamMessage::Perception` framing.
- [x] Agent action packets mutate the physical server room state in real time and return `StreamMessage::ActionResult`.
- [x] Authoritative E-Stop halts all motor actions across the bridge.

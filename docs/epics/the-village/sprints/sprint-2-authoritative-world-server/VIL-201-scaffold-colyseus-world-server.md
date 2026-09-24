# VIL-201: Scaffold Self-Hosted Colyseus World Server

**Epic:** The Village — Living World Experiment  
**Sprint:** Sprint 2 — Authoritative World Server  
**Layer:** `the-village/world`  
**Status:** Complete

---

## 1. Context & Objective
Scaffold the 100% free, self-hosted Colyseus multiplayer game server in `the-village/world` (using the MIT open-source library), establishing the authoritative physical backend for the simulation.

## 2. Requirements
1. Initialize TypeScript Colyseus application under `the-village/world/`:
   * Set up Node.js / Express / Colyseus server with zero paid/commercial cloud dependencies.
   * Configure room structure: `VillageRoom`.
2. Ingest the 2D Village Tiled map (`.json` / tile matrices):
   * Ground layers (grass, water, sand, paths, elevation).
   * Static collision layers (mountains, coastlines, deep water).
   * Points of interest (Town Hall, Blacksmith Forge, Watchtower, Windmill).
3. Define Colyseus State Schema:
   * `TileGridState`: Synchronized grid state with dynamic terrain IDs.
   * `EntityMapState`: Synchronized entity records (villagers, props, structures).
   * `GlobalEconomyState`: Synchronized resources (`timber`, `iron`, `stone`, `grain`).

## 3. Acceptance Criteria
- [x] Colyseus server builds and starts cleanly on port 3001 with zero external cloud dependencies.
- [x] Ingests Tiled map data and initializes 2D coordinate space (`MapManager.ts`).
- [x] Schema state syncs reliably to test client connections (`VillageState`, `EntityState`, `TileState`, `EconomyState`).

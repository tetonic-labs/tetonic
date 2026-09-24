# VIL-202: Authoritative Grid Pathfinding & Movement

**Epic:** The Village — Living World Experiment  
**Sprint:** Sprint 2 — Authoritative World Server  
**Layer:** `the-village/world`  
**Status:** Ready

---

## 1. Context & Objective
Implement authoritative movement validation and 2D spatial pathfinding on the Colyseus server so agent `move_to` commands translate into smooth, collision-free coordinate navigation without telepathic teleportation.

## 2. Requirements
1. Implement A* grid pathfinding on the server:
   * Build a walkable cost matrix from the Tiled ground and collision layers.
   * Calculate shortest path routes around obstacles, water tiles, and placed buildings.
2. Server-side tick loop:
   * Advance entity positions incrementally along their computed paths (e.g. 1 tile per second or fractional interpolation).
   * Emit position updates to the room state schema.
3. Validate movement boundaries:
   * Prevent agents from crossing impassable terrain or walking off-grid.
   * Dynamically update pathfinding matrix when new buildings or obstacles are placed.

## 3. Acceptance Criteria
- [ ] Agents accurately navigate between points of interest (e.g. Manor to Forge) using A*.
- [ ] Obstacle collisions are authoritatively prevented.
- [ ] Positions advance smoothly and synchronize to connected room subscribers.

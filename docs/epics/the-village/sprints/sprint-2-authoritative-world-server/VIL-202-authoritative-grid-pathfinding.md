# VIL-202: Authoritative Grid Pathfinding & Movement

**Epic:** The Village — Living World Experiment  
**Sprint:** Sprint 2 — Authoritative World Server  
**Layer:** `the-village/world`  
**Status:** Complete

---

## 1. Context & Objective
Implement authoritative movement validation and 2D spatial locomotion on the Colyseus server so agent motor commands translate into smooth, collision-free coordinate navigation without telepathic teleportation or black-box server-side auto-piloting.

## 2. Requirements
1. Implement authoritative locomotion & collision on the server:
   * Build a walkable cost matrix from the Tiled ground and collision layers.
   * Enforce System 1 reflexive step locomotion (`step_move { dx, dy }`) and System 2 intent waypoint tracking (`move_towards { targetX, targetY }`).
2. Server-side tick loop:
   * Advance entity positions incrementally along validated steps.
   * Emit position and facing direction updates to the room state schema.
3. Validate movement boundaries:
   * Prevent agents from crossing impassable terrain, cutting diagonal walls, or walking off-grid.
   * Dynamically update walkability matrix when new buildings or obstacles are placed.

## 3. Acceptance Criteria
- [x] Agents navigate between points of interest using System 1 steps and System 2 intent waypoints.
- [x] Obstacle and water collisions are authoritatively prevented (`MovementController.ts`).
- [x] Positions advance smoothly, update facing direction, and synchronize to connected room subscribers.

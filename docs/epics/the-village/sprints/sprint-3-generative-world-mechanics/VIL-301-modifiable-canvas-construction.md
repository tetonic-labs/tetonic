# VIL-301: Modifiable Canvas & Construction Mechanics

**Epic:** The Village — Living World Experiment  
**Sprint:** Sprint 3 — Generative World Mechanics  
**Layer:** `the-village/world` & Physics  
**Status:** Ready

---

## 1. Context & Objective
To make the simulation truly generative and move beyond clunky static loops, give agents the physical agency to modify the terrain, pave roads, and construct buildings on the 2D grid.

## 2. Requirements
1. Implement grid mutation operations in `VillageRoom`:
   * `place_tile(x, y, tile_type)`: Lay cobblestone path, clear scrub, till soil.
   * `build_structure(x, y, structure_type)`: Construct sheds, storage chests, drying racks, fences, footbridges, and signposts.
   * `demolish_structure(structure_id)`: Reclaim space or materials from damaged structures.
2. Construction lifecycle states:
   * Structures begin in a `under_construction` state with required work cycles and materials.
   * Agents dedicate labor ticks to advance construction progress to `completed`.
3. Dynamic collision updating:
   * When an agent places a fence or wall, the server's A* pathfinding matrix immediately flags the tile as impassable.
   * When an agent paves a road, walking speed across that tile increases by $25\%$.

## 3. Acceptance Criteria
- [ ] Agents can successfully pave paths and place functional structures.
- [ ] Newly placed obstacles block pathfinding immediately.
- [ ] State diffs broadcast dynamic tile changes to all watching clients.

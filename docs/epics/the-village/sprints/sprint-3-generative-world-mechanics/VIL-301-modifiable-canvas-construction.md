# VIL-301: Modifiable Canvas, Physical Chemistry & Fire Mechanics

**Epic:** The Village — Living World Experiment  
**Sprint:** Sprint 3 — Generative World Mechanics  
**Layer:** `the-village/world` & Physics  
**Status:** Complete

---

## 1. Context & Objective
True emergence requires moving beyond static, hardcoded interaction trees. Give the world a composable physical property system (temperature, flammability, fuel, moisture, structural durability) where physical phenomena—such as fire ignition, spreading across wooden structures, and water extinguishing—occur dynamically. Agents possess the physical agency to modify terrain, construct buildings, and respond to environmental hazards.

## 2. Requirements
1. **Property-Based Physical Chemistry Engine:**
   * Grid cells and structures maintain physical properties: `temperature`, `flammability` (0..1), `fuel` (remaining combustible mass), `moisture` (extinguishing factor), and `durability`.
   * **Ignition & Thermal Spread:** When temperature exceeds ignition threshold on a flammable tile, it catches fire. Burning tiles emit heat to neighboring cardinal tiles each tick.
   * **Combustion & Collapse:** Sustained burning consumes fuel and reduces structural durability. When durability hits 0, the structure collapses into ash/debris, freeing or blocking walkability.
   * **Quenching:** High moisture or water contact lowers temperature and extinguishes active fire.
2. **Modifiable Canvas & Construction Operations:**
   * `place_tile(x, y, tile_type)`: Lay paths, clear brush, till soil.
   * `build_structure(x, y, structure_type)`: Erect wooden walls, fences, storage chests, and bridges with defined material properties.
   * `ignite_tile(x, y)` / `extinguish_tile(x, y)`: Direct chemical interventions by agents or environmental sparks.
   * `demolish_structure(structure_id)`: Reclaim space or salvage materials.
3. **Dynamic Walkability & Hazard Perception:**
   * Newly placed obstacles or collapsed debris immediately update `MapManager` collision matrices.
   * Fire on a tile flags it as a high-urgency environmental hazard in the TWP `Perception` packet, allowing reflexive retreat and deliberative response.

## 3. Acceptance Criteria
- [x] Physical chemistry simulation ticks thermal spread and flammability across adjacent combustible tiles (`ChemistryEngine.ts`).
- [x] Structural integrity degrades under combustion, collapsing into debris when consumed.
- [x] Agents can pave paths, erect structures, ignite campfires/heat sources, and extinguish fires.
- [x] Fire events generate high-urgency perception signals and trigger state change broadcasts.

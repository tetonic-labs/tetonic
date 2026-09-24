# VIL-302: Resource Economy & Supply Graph

**Epic:** The Village — Living World Experiment  
**Sprint:** Sprint 3 — Generative World Mechanics  
**Layer:** `the-village/world` & Economy  
**Status:** Ready

---

## 1. Context & Objective
Emergent social and operational dynamics require resource tension and supply chain interdependencies. An agent cannot simply build without materials; materials must be harvested, refined, and allocated.

## 2. Requirements
1. Implement the material supply chain:
   * **Harvesting:** Forest trees $\rightarrow$ Raw Timber; Quarry/Coast $\rightarrow$ Stone; Iron Vein $\rightarrow$ Raw Ore.
   * **Refining:** Blacksmith Forge turns Raw Ore + Timber $\rightarrow$ Iron Nails & Tools; Windmill turns Grain $\rightarrow$ Flour.
   * **Consuming:** Building projects require combinations of processed materials (e.g., Bridge = 30 Timber + 15 Iron Nails).
2. Physical inventory tracking:
   * Agents have limited personal carrying capacity (e.g. 5 items).
   * Storage chests and granaries hold village reserves at specific map coordinates.
   * Agents must travel to storage locations to deposit or withdraw supplies.
3. Scarcity signals:
   * Emit resource reserve signals (`timber_reserve`, `iron_reserve`) in the perception packet to trigger agent deliberation when supplies run dangerously low.

## 3. Acceptance Criteria
- [ ] Material harvesting and refining flow works end-to-end according to recipe costs.
- [ ] Agents must physically pick up resources from storage nodes to construct buildings.
- [ ] Low-supply thresholds trigger urgent perception events.

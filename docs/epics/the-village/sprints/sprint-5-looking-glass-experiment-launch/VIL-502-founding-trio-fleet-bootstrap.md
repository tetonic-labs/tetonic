# VIL-502: Founding Trio Fleet Bootstrap & Multi-Tier Inference Setup

**Epic:** The Village — Living World Experiment  
**Sprint:** Sprint 5 — The Looking Glass & Live 24/7 Experiment Launch  
**Layer:** `engine/litho/tetonic-app` & Launch Configuration  
**Status:** Complete

---

## 1. Context & Objective
Bootstrap the complete Founding Trio (**The Mayor**, **The Blacksmith**, and **The Scout**) into the `"village-council"` squad with their autonomous routines and distinct role behaviors.

## 2. Requirements
1. **Agent Charters & Roles:**
   * **The Mayor (Barnaby):** Strategic intent on civic harmony, petition evaluation, and notice posting. Walks civic route, deliberates on citizen petitions.
   * **The Blacksmith (Silas):** Strategic intent on forge operation, tool production, and material economy. Crafts at the anvil, responds to harbor dock supply drops.
   * **The Scout (Maeve):** Strategic intent on perimeter security, resource foraging, and weather alerts. Patrols boundary waypoints, river crossings, and trails.
2. **Autonomous Living World Simulation:**
   * Autonomous routines embedded directly within `VillageRoom.ts` enabling full self-contained operation without external dependencies.
   * Continuous generation of rich, contextual thoughts broadcast to the spectator SSE inspector hub.

## 3. Acceptance Criteria
- [x] All 3 founding agents bootstrap cleanly and execute their distinct behavioral loops (`VillageRoom.ts`).
- [x] Silas actively perceives and hauls harbor dock supply shipments to storehouses.
- [x] Barnaby deliberates on high-voted visitor petitions and updates the Town Notice Board.
- [x] Maeve patrols perimeter waypoints and river crossings with live thought broadcasting.

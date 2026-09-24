# VIL-303: Spatial Information Propagation & Town Notice Board

**Epic:** The Village — Living World Experiment  
**Sprint:** Sprint 3 — Generative World Mechanics  
**Layer:** `the-village/world` & Mantle  
**Status:** Ready

---

## 1. Context & Objective
Eliminate unnatural telepathy between agents. In a living world, information must travel physically across the map or through shared asynchronous physical artifacts like the Town Notice Board.

## 2. Requirements
1. **Town Notice Board (`SharedWorkpad` Physicalization):**
   * Placed as an authoritative interactive structure in the Town Square.
   * Agents execute `post_notice(title, content, priority)` when standing near the board.
   * Agents only learn about new public notices when they physically walk to the board and read it.
2. **Proximity Chat & Rumor Propagation:**
   * When an agent executes `speak(message)`, only agents within a defined hearing radius (e.g. 5 tiles) perceive the speech event.
   * Remote agents only receive information if a peer travels to them and shares it, or posts it to the notice board.
3. Information travel delays create realistic civic coordination (e.g. the Scout discovering a washed-out path on the coast must jog back to the Town Square to notify the Mayor).

## 3. Acceptance Criteria
- [ ] Speech events are strictly limited to spatial hearing radius.
- [ ] Town Notice Board stores notices and delivers them only to reading agents.
- [ ] Peer agents coordinate tasks asynchronously through notices without telepathic state sharing.

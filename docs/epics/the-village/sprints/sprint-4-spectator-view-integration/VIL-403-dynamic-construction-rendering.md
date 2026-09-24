# VIL-403: Dynamic Construction & Activity Rendering

**Epic:** The Village — Living World Experiment  
**Sprint:** Sprint 4 — Spectator View Integration  
**Layer:** `the-village/web` & Graphics  
**Status:** Ready

---

## 1. Context & Objective
Ensure that all dynamic world mutations made by agents—newly paved roads, fences under construction, smoke from active forges, and placed signs—render dynamically and beautifully in Pixi.js without requiring a page reload.

## 2. Requirements
1. Dynamic tilemap layer updating:
   * When Colyseus broadcasts a `place_tile` change, swap the tile texture dynamically on the Pixi.js ground layer container.
2. Construction scaffolding & progress animation:
   * Structures with status `under_construction` render wooden scaffolding and a floating progress indicator.
   * Upon completion, trigger a subtle dust/sparkle particle burst and reveal the finished building sprite.
3. Activity visual effects:
   * Particle effects for active tasks: forge smoke when Blacksmith is crafting; wood chips when Scout is chopping; quill animations when Mayor is writing.
4. Town Notice Board modal:
   * Spectators clicking on the physical Town Notice Board in the Town Square open a readable parchment modal showing current active notices and directives.

## 3. Acceptance Criteria
- [ ] New tiles and structures appear dynamically on the canvas as agents construct them.
- [ ] Active work triggers appropriate pixel-art visual effects.
- [ ] Town Notice Board displays real notices posted by the agents.

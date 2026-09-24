# VIL-304: Visitor Object Creation, Star Ranking & Discovery Lifecycle

**Epic:** The Village — Living World Experiment  
**Sprint:** Sprint 3 — Generative World Mechanics  
**Layer:** `the-village/world` & Community Pipeline  
**Status:** Ready

---

## 1. Context & Objective
Because the static pixel tileset cannot contain every emergent phenomenon (such as animated fire, custom tools, or unique town structures), empower site visitors to draw multi-frame pixel objects/animations and vote on them with stars. Top-ranked creations cross "The Looking Glass" into the agent simulation via an authoritative Discovery Lifecycle, giving agents new physical blueprints and phenomena to interact with.

## 2. Requirements
1. **Authoritative Community Asset Registry:**
   * Store dynamic community creations in `VillageRoom` with synchronized schema:
     * `id`, `name`, `category` (`vfx`, `structure`, `tool`, `decoration`, `crop`).
     * `author`: site visitor handle.
     * `frames`: array of 16x16 or 32x32 color hex arrays (1 to 4 frames of animation).
     * `properties`: physical tags (`flammable`, `heat_source`, `light_radius`, `walkable`, `durability`).
     * `stars`: integer vote counter.
     * `status`: `pending` | `ranked` | `discovered`.
2. **Star Ranking & Curation:**
   * REST endpoints (`POST /api/v1/assets`, `POST /api/v1/assets/:id/star`, `GET /api/v1/assets/top`) and Colyseus message handlers (`submit_asset`, `star_asset`).
   * Community voting elevates high-quality assets to the top of the queue for the next Discovery cycle.
3. **The In-World Discovery Lifecycle:**
   * At configured intervals (e.g. Epoch dawn or discovery ticks), top-ranked assets are officially discovered:
     * **Environmental VFX (Fire/Smoke):** If an asset is a fire/smoke animation, the chemistry engine dynamically adopts the community sprite for burning tiles in the world.
     * **Physical Blueprints:** Items and structures generate a `blueprint_discovered` event in the TWP `Perception` stream with their physical properties.
     * Agents perceive the newly discovered object and can incorporate it into deliberate construction, crafting, or ceremonial behavior.

## 3. Acceptance Criteria
- [ ] Visitors can submit multi-frame pixel creations with physical properties via server API.
- [ ] Star voting increments rankings and maintains an authoritative top-assets leaderboard.
- [ ] Discovery lifecycle successfully transitions top-voted assets into active world state and emits TWP perception events to agents.
- [ ] Dynamic animations (e.g. community-drawn fire) link directly to the physical chemistry engine.

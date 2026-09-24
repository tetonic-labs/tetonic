# VIL-501: The Looking Glass Visitor Conduit

**Epic:** The Village — Living World Experiment  
**Sprint:** Sprint 5 — The Looking Glass & Live 24/7 Experiment Launch  
**Layer:** `the-village/web` & `world`  
**Status:** Complete

---

## 1. Context & Objective
Implement "The Looking Glass" public interaction channels, enabling web visitors to actively influence the village through structured, physicalized world mechanisms rather than direct raw text prompts.

## 2. Requirements
1. **The Petition Box (Town Hall):**
   * Visitors submit civic petitions (e.g. *"Rebuild the southern pier"*, *"Build a beacon on watchtower island"*).
   * Petitions enter the Colyseus room queue and are emitted as `WorldEvent::VisitorPetition` to The Mayor.
   * Rate limiting & content moderation: restrict submissions per IP/session to prevent flooding.
2. **Harbor Dock Resource Drops:**
   * Visitors can drop resource shipments (timber, stone, exotic goods) at the physical dock coordinates.
   * A wooden crate entity spawns on the dock; agents perceive the crate and can inspect/gather its contents.
3. **Democratic Referendum Ballots:**
   * When the Mayor puts a policy or major project to a town vote, the web client renders the voting drawer.
   * Spectators cast votes; tallies update live on the HUD and are read by the Mayor at the close of the ballot.
4. **Visitor Creation Studio & Star Voting:**
   * Upgrade `AssetForgeModal` with multi-frame animation studio (1 to 4 frames with live looping preview).
   * Physical property tagging (`flammable`, `heat_source`, `walkable`, `durability`).
   * ⭐ Star upvote button on community cards calling authoritative `starAsset(id)`.

## 3. Acceptance Criteria
- [x] Visitors can submit petitions, drop dock resources, and vote on town ballots (`PetitionModal.ts`, `DockDropModal.ts`).
- [x] Visitors can create multi-frame pixel animations, tag physical properties, and vote on community blueprints with stars (`AssetForgeModal.ts`).
- [x] Submissions cleanly translate into in-world events without prompt injection vulnerabilities (`VillageRoom.ts`, `simulation.ts`).
- [x] Rate limits and content sanitization prevent denial-of-service or queue poisoning.

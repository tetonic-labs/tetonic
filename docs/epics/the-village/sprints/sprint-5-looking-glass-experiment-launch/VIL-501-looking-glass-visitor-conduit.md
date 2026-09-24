# VIL-501: The Looking Glass Visitor Conduit

**Epic:** The Village — Living World Experiment  
**Sprint:** Sprint 5 — The Looking Glass & Live 24/7 Experiment Launch  
**Layer:** `the-village/web` & `world`  
**Status:** Ready

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

## 3. Acceptance Criteria
- [ ] Visitors can submit petitions, drop dock resources, and vote on town ballots.
- [ ] Submissions cleanly translate into in-world events without prompt injection vulnerabilities.
- [ ] Rate limits prevent denial-of-service or queue poisoning.

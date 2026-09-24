# VIL-603: 24-Hour Autonomous Production Soak & Cost Audit

**Epic:** The Village — Living World Experiment  
**Sprint:** Sprint 6 — Production Deployment & 24/7 Operations  
**Layer:** Operations & Telemetry  
**Status:** Ready

---

## 1. Context & Objective
Execute the full 24-hour continuous autonomous soak test in the live production environment, validating physical stability, zero-compute spectator streaming, and economic viability.

## 2. Requirements
1. **24-Hour Continuous Operation:**
   * Run the production deployment continuously for 24 hours without human intervention.
   * Verify zero unhandled panics, zero memory leaks, and zero state corruption across repeated epochs.
2. **Public Spectator Load Test:**
   * Validate that concurrent browser spectators can view the Pixi.js map, stream SSE thought monologues, and submit petitions/star blueprints simultaneously without degrading agent tick cadence ($<200\text{ms}$).
3. **Economic & Cost Audit:**
   * Measure total 24-hour token consumption and monetary cost across System 1 reflex and System 2 deliberation models.
   * Prove daily operational cost adheres strictly to the budget goal ($< \$5\text{--}\$10/\text{day}$).

## 3. Acceptance Criteria
- [ ] 24-hour continuous run completed with 100% uptime and zero manual interventions.
- [ ] Colyseus room state and agent episodic memory persist across ticks without drift.
- [ ] Total 24-hour operating expenditure documented and within target budget.
- [ ] Experiment officially open to the public.

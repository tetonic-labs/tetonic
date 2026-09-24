# VIL-503: 24-Hour Autonomous Soak Test & Safety Verification

**Epic:** The Village — Living World Experiment  
**Sprint:** Sprint 5 — The Looking Glass & Live 24/7 Experiment Launch  
**Layer:** Operations & Verification  
**Status:** Ready

---

## 1. Context & Objective
Execute the full 24-hour continuous autonomous soak test of the live Village simulation, validating physical stability, zero-compute inspection, economic sustainability, and authoritative operator safety controls.

## 2. Requirements
1. **24-Hour Continuous Operation:**
   * Run the full simulation with the 3 founding agents live for 24 continuous hours.
   * Verify zero unhandled panics, memory leaks, runaway token loops, or state corruptions.
2. **Operator Safety Interlocks:**
   * Test in-flight steering injection (`POST /api/v1/agents/:id/steer`) during live operation to verify real-time goal adjustment.
   * Trigger 1-click E-Stop (`POST /api/v1/agents/:id/estop` and `/fleet/estop`) to confirm instantaneous physical actuator freeze and mutation abort.
   * Verify safe resume clearance (`POST /api/v1/agents/:id/resume`).
3. **Public Spectator Experience:**
   * Confirm multiple concurrent browser viewers can watch the Pixi.js map and stream live thought monologues via SSE simultaneously with zero performance degradation on the agent loops.
4. **Economic & Performance Audit:**
   * Document total 24-hour token consumption and monetary cost, proving economic viability.

## 3. Acceptance Criteria
- [ ] 24-hour soak test passes with zero crashes or disconnections.
- [ ] E-Stop immediately freezes target agents and resumes cleanly upon operator command.
- [ ] Measured 24-hour operating cost matches budget targets ($< \$5\text{--}\$10/\text{day}$).
- [ ] Spectator experience is smooth, responsive, and engaging.

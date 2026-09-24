# VIL-503: Local Operator Safety Interlocks & Simulation Sanity Verification

**Epic:** The Village — Living World Experiment  
**Sprint:** Sprint 5 — The Looking Glass & Fleet Integration  
**Layer:** Operations & Verification  
**Status:** Complete

---

## 1. Context & Objective
Validate physical stability, zero-compute inspection, and authoritative operator safety controls locally across the three founding agents before production deployment.

## 2. Requirements
1. **Operator Safety Interlocks:**
   * Test in-flight steering injection (`POST /api/v1/control/steer`) during live operation to verify real-time goal adjustment without state corruption.
   * Trigger 1-click E-Stop (`POST /api/v1/control/estop`) to confirm instantaneous physical actuator freeze and mutation abort.
   * Verify safe resume clearance (`POST /api/v1/control/resume`).
2. **Local Multi-Agent Simulation Sanity Run:**
   * Run accelerated simulated epochs with all 3 founding agents active.
   * Verify zero unhandled panics, deadlocks, or runaway token loops in local execution.
3. **Public Spectator Experience Sanity:**
   * Confirm concurrent spectator connections can watch the Pixi.js map and stream live thought monologues via SSE simultaneously with zero performance degradation on the agent loops.

## 3. Acceptance Criteria
- [x] E-Stop immediately freezes target agents and resumes cleanly upon operator command (`VillageRoom.ts`, `simulation.ts`).
- [x] In-flight steering injection redirects agent intent without restarting agent processes (`VillageRoom.steerAgent`).
- [x] Accelerated multi-agent sanity run passes with zero crashes or desyncs (`petitions_and_drops.test.ts`).
- [x] Spectator experience is smooth, responsive, and engaging (`visitor_conduit.test.ts`).

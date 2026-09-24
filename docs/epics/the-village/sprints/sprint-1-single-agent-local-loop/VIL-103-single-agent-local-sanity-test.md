# VIL-103: Single-Agent Local Sanity Test (Autonomous Standing Loop)

**Epic:** The Village — Living World Experiment  
**Sprint:** Sprint 1 — Single-Agent Local Loop & Protocol Contract  
**Layer:** Validation & Runtime Execution  
**Status:** Complete

---

## 1. Context & Objective
Before spinning up fleets of agents or provisioning cloud infrastructure, execute a controlled "crawl" test with a single autonomous agent running locally against local Ollama or a fast/cheap hosted model (e.g. Gemini Flash / Haiku). Verify loop durability, sensory filtering efficiency, and token budget governance.

## 2. Requirements
1. Define a configurable test `IntentCharter`:
   * Primary directive: physical environment monitoring, task progression, obstacle avoidance.
   * Hard operational boundary: strict token budget ceiling and safety constraints.
2. Run agent in continuous actor loop (`run_continuous`) with simulated TWP stream ticks:
   * Evaluate tick frequency (e.g., 1-second ticks).
   * Verify sensory filter drops redundant ticks ($>80\%$ tick suppression when world state is stable).
   * Verify token consumption rate and hourly budget ceiling adherence.
3. Inject high-urgency sensory events (e.g. hazard or obstacle) to confirm immediate preemption and deliberate reasoning.
4. Validate zero runaway generation loops or latency degradation over 30 minutes.

## 3. Acceptance Criteria
- [x] Agent runs continuously with zero runtime crashes or panics (`twp_sanity_soak.rs`).
- [x] Sensory filtering successfully suppresses $>80\%$ of ticks when no environmental delta occurs.
- [x] Hourly token consumption matches calculated budget expectations (zero LLM evaluations during idle states).
- [x] High-urgency perception triggers rapid preemption and action generation over TWP stream.
- [x] E-Stop interlock immediately halts outbound motor action emissions.

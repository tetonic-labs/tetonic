# VIL-502: Founding Trio Fleet Bootstrap & Multi-Tier Inference Setup

**Epic:** The Village — Living World Experiment  
**Sprint:** Sprint 5 — The Looking Glass & Live 24/7 Experiment Launch  
**Layer:** `engine/litho/tetonic-app` & Launch Configuration  
**Status:** Ready

---

## 1. Context & Objective
Bootstrap the complete Founding Trio (**The Mayor**, **The Blacksmith**, and **The Scout**) into the `"village-council"` squad with their full `IntentCharter`s and configure the cost-effective multi-tier inference routing.

## 2. Requirements
1. **Agent Charters & Roles:**
   * **The Mayor:** Strategic intent on civic harmony, petition evaluation, and notice posting. System 2 heavy.
   * **The Blacksmith:** Strategic intent on forge operation, tool production, and material economy. Balanced System 1/2.
   * **The Scout:** Strategic intent on perimeter security, resource foraging, and weather alerts. System 1 reflex heavy.
2. **Multi-Tier Inference Configuration (`tetonic.toml`):**
   * **System 1 (Reflexes / Routine Ticks):** Point to fast, ultra-cheap hosted provider (e.g. Groq, Cerebras, or Gemini 1.5 Flash / Claude Haiku) for $<200\text{ms}$ decisions costing pennies per day.
   * **System 2 (Deliberation / Petitions / Strategy):** Point to frontier provider (Claude 3.5 Sonnet / Gemini 1.5 Pro) invoked only on high-urgency escalations.
3. Automated bootstrap script to launch the organization, squad, and all 3 agents via `tetonic-app`'s Fleet API.

## 3. Acceptance Criteria
- [ ] All 3 agents bootstrap cleanly and register with `FleetSupervisor`.
- [ ] System 1 handles rapid physical reactions and pathing smoothly.
- [ ] System 2 deliberates asynchronously on petitions without freezing physical simulation ticks.
- [ ] Total hourly token consumption adheres strictly to budget quotas.

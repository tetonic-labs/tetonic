# SAE-103: Pluggable Brain Interface & Continuous Cognition

**Epic:** Standing Agent Engine  
**Sprint:** Sprint 1 — The Continuous Agent & Pluggable Brain  
**Layer:** `engine/core/tetonic-runtime`  
**Status:** Ready

---

## 1. Context & Objective
The cognitive architecture of an agent must be flexible and pluggable, not rigidly bound to a single paradigm:
* **The 95% Default (`SingleModelBrain`):** A developer supplies a single model (e.g. Claude, Gemini, or a local model via Ollama). One model handles both turn completions and continuous perceptions at its natural pace, requiring zero cognitive jargon.
* **The High-Tempo Optimization (`DualProcessBrain`):** An advanced brain composing a fast local reflex provider (System 1) with an asynchronous deliberative provider (System 2) and preemption triggers.
* **The Deterministic Alternative (`ScriptedBrain`):** Pure state machine / heuristic logic without an LLM (for simple background NPCs, deterministic workers, or testing).

All cognitive structures implement the exact same `tetonic_domain::Brain` trait so `Agent` remains completely agnostic to internal brain complexity.

## 2. Requirements
1. Extend `Brain` trait in `tetonic-domain`:
   * Support `perceive(&self, perception: Perception) -> Result<Option<WorldAction>, BrainError>`.
   * Provide a default implementation that bridges to `complete()` for single-model backward compatibility.
2. Update `SingleModelBrain` in `tetonic-runtime`:
   * Implement `perceive()` so a single model can process incoming continuous perceptions out-of-the-box.
   * Support cadence throttling (so a single model isn't overwhelmed by high-frequency ticks).
3. Implement `DualProcessBrain` in `tetonic-runtime`:
   * Two providers: `reflexive_provider` (System 1) and `deliberative_provider` (System 2).
   * Fast reflex evaluation on signals; asynchronous dispatch to deliberative provider on urgency thresholds.
   * Cognitive preemption: cancel in-flight deliberative tasks via `CancellationToken` if sensory urgency changes.

## 3. Acceptance Criteria
- [ ] An agent using `SingleModelBrain` successfully processes a continuous perception stream.
- [ ] An agent using `DualProcessBrain` successfully runs fast reflexes and preempts slow deliberations when an urgent tick arrives.
- [ ] Unit tests prove zero code changes are required in `Agent` when swapping between `SingleModelBrain` and `DualProcessBrain`.

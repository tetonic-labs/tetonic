# SAE-102: Continuous Agent Loop (`run_continuous`)

**Epic:** Standing Agent Engine  
**Sprint:** Sprint 1 — Continuous Cognition  
**Layer:** `engine/core/tetonic-core`  
**Status:** Complete

---

## 1. Context & Objective
Currently, `Agent` only supports turn-based execution via `turn(&self, convo, invocation, on_step)`. In a continuous environment, the world clock does not pause. The agent must run an ongoing background actor loop that consumes a stream of `Perception` inputs, queries its brain, and emits `WorldAction` outputs.

## 2. Requirements
1. Migrate `Agent` struct to hold `Arc<dyn Brain>` rather than `Arc<dyn InferenceProvider>` directly.
2. Implement `pub async fn run_continuous(...)` on `Agent`:
   * Ingest `Perception` from `tokio::sync::mpsc::Receiver<Perception>`.
   * Evaluate perception via `Brain::perceive(&self, perception)`.
   * Emit resulting `WorldAction` to `tokio::sync::mpsc::Sender<WorldAction>`.
   * Support graceful cancellation via `tokio_util::sync::CancellationToken` or `WorkScope`.
3. Support latest-value drop semantics (if the brain is busy, stale intermediate ticks can be dropped so the agent always acts on the freshest world state).

## 3. Acceptance Criteria
- [x] `Agent` is fully decoupled from direct inference dependencies.
- [x] `run_continuous` passes unit tests simulating a 50Hz tick stream with mock actions.
- [x] Cancellation signal cleanly terminates the loop without panicking.

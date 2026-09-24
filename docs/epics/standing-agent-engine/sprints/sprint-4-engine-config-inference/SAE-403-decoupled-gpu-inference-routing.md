# SAE-403: Decoupled GPU Inference Fabric Routing

**Epic:** Standing Agent Engine  
**Sprint:** Sprint 4 — Engine Configuration & Decoupled Inference  
**Layer:** `engine/atmos/tetonic-inference` & `engine/core/tetonic-runtime`  
**Status:** Complete

---

## 1. Context & Objective
Stateful agents should not run on expensive GPU nodes, and GPU nodes should not hold stateful actor loops. Inference must be completely decoupled from execution. Runners dispatch token requests across a pool of stateless GPU endpoints with circuit breaking and fallback.

## 2. Requirements
1. Implement `DecoupledInferenceRouter`:
   * Manages a dynamic pool of stateless inference endpoints (`vLLM`, `Ollama`, `TensorRT-LLM`, or cloud APIs).
   * Health checks GPU endpoints and tracks current in-flight queue depth.
   * Routes reflexive (fast) requests to high-throughput endpoints and deliberative (deep) requests to high-capacity reasoning endpoints.
2. Implement circuit breaking and resilient fallback:
   * Trips circuit on consecutive timeouts and redirects requests to backup endpoints or local fallback models.

## 3. Acceptance Criteria
- [x] Agent loops route inference requests to decoupled GPU endpoints without holding GPU memory.
- [x] Failed endpoints trip circuit breaker and route to healthy backups without crashing the agent loop.

# SAE-502: Zero-Compute Telemetry & Live Thought Inspection Stream

**Epic:** Standing Agent Engine  
**Sprint:** Sprint 5 — Fleet Portal & Human Experience  
**Layer:** `engine/core/tetonic-telemetry` & `engine/litho/tetonic-app`  
**Status:** Ready

---

## 1. Context & Objective
Operators need real-time visibility into what standing agents are doing, thinking, and perceiving without burdening the agent actor loop or polluting conversational context. Demuxed thinking tags and sensory signals must be broadcast over SSE/WebSockets at zero inference cost.

## 2. Requirements
1. Implement `ThoughtStreamHub`:
   * Ingests real-time streaming deltas from `TokenDemuxer` (thoughts vs output prose) and `Perception` events.
   * Broadcasts Server-Sent Events (SSE) or WebSocket events on `GET /api/v1/agents/:agent_id/thoughts`.
   * Ring buffer preserves last $N$ seconds of thoughts for immediate UI catch-up upon connection.
2. Emits structured telemetry events:
   * `PerceptionReceived` (signal values, urgency)
   * `ThoughtDelta` (raw inner monologue chunk)
   * `ActionProposed` & `ActionExecuted` (verbs, parameters, outcome)
   * `BoundaryViolation` (intercepted actions)

## 3. Acceptance Criteria
- [ ] SSE thought stream broadcasts token deltas with low latency (<10ms).
- [ ] Multiple UI clients can connect concurrently without degrading agent loop throughput.

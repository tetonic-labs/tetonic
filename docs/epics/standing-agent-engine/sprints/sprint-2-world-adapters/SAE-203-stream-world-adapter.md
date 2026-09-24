# SAE-203: WebSocket & Stream WorldAdapter

**Epic:** Standing Agent Engine  
**Sprint:** Sprint 2 — World Adapters & Actuator Safety Gates  
**Layer:** `engine/core/tetonic-runtime`  
**Status:** Complete

---

## 1. Context & Objective
To enable agents to be aimed at external real-time environments (live simulations, cloud control planes, game engines), we need a concrete implementation of `WorldAdapter` that communicates over bidirectional WebSocket / TCP streaming sockets.

## 2. Requirements
1. Implement `StreamWorldAdapter` in `tetonic-runtime`:
   * Connects to a target WebSocket URI (e.g. `wss://target-env/stream`).
   * Handles inbound JSON framing: deserializes incoming ticks into `Perception` instances and forwards to the agent's channel.
   * Handles outbound action dispatch: serializes `WorldAction` instances and sends over the wire.
   * Manages reconnection, keep-alives, and connection backoff automatically.
2. Provide integration test fixture simulating an external stream server.

## 3. Acceptance Criteria
- [x] Connects, completes handshake, and streams perceptions to a running agent.
- [x] Successfully delivers emitted actions back to the stream server.
- [x] Recovers cleanly from network drops without crashing the agent actor loop.

# VIL-102: Generic Stream World Adapter & Protocol Binding

**Epic:** The Village — Living World Experiment  
**Sprint:** Sprint 1 — Single-Agent Local Loop & Protocol Contract  
**Layer:** `engine/core/tetonic-runtime` / Adapter Runner  
**Status:** Complete

---

## 1. Context & Objective
Ensure Tetonic's `StreamWorldAdapter` acts as the universal, domain-neutral bridge connecting continuous agents to external simulations over TCP, Unix domain sockets, or duplex streams. No custom Rust adapters are needed for any specific game or domain.

## 2. Requirements
1. Verify and harden `StreamWorldAdapter` in `tetonic-runtime`:
   * Robust NDJSON stream framing for `StreamMessage` envelope.
   * Auto-reconnection with exponential backoff on connection drops.
   * Concurrent bidirectional channel dispatch (inbound perceptions vs outbound actions).
2. Wire `EstopSwitch` physical interlock:
   * Instantly trips and halts outbound action serialization when E-Stop is triggered.
   * Propagates inbound remote E-Stop signals into the agent lifecycle state.
3. Affordance negotiation:
   * Validates dispatched actions against the advertised `WorldManifest` capability set.
4. Clean re-export in `tetonic-core` alongside `WorldAdapter` and `EstopSwitch`.

## 3. Acceptance Criteria
- [x] `StreamWorldAdapter` executes clean roundtrips with mock duplex streams and TCP sockets.
- [x] Ingests `Perception` ticks and transmits `WorldAction` decisions without domain-specific code.
- [x] E-Stop interlock immediately halts outbound transmission.
- [x] Fully verified by automated unit and integration tests.

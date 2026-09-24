# VIL-101: Tetonic World Protocol (TWP) Wire Specification

**Epic:** The Village — Living World Experiment  
**Sprint:** Sprint 1 — Single-Agent Local Loop & Protocol Contract  
**Layer:** `engine/core/tetonic-domain` & Stream Protocol  
**Status:** Complete

---

## 1. Context & Objective
In line with the core axiom of domain neutrality, the Tetonic Engine must NOT contain custom, bespoke adapters for every external ecosystem. Instead, the engine provides the **Tetonic World Protocol (TWP)**: a universal, stream-framed JSON wire standard that any game engine, simulation server, robotics controller, or cloud environment can speak over WebSockets, TCP, or Unix domain sockets.

## 2. Requirements
1. **TWP Message Envelope (`StreamMessage`):**
   * `perception`: Inbound world tick from simulation to agent.
   * `action`: Outbound motor decision from agent to simulation.
   * `action_result`: Actuator execution confirmation / feedback.
   * `estop`: Authoritative emergency stop command.
   * `resume`: Resumption signal clearing an active interlock.
   * `heartbeat`: Ping/pong message detecting socket liveness.
2. **Standard Perception Schema (`Perception`):**
   * Monotonic sequence number and ISO-8601 timestamp.
   * Urgency classification (`background`, `low`, `medium`, `high`, `critical`).
   * Named pre-computed physical signals with trends (`rising`, `falling`, `stable`) and change flags.
   * Discrete occurrences as `WorldEvent`s (`kind`, `source`, `payload`, `urgency`).
   * Arbitrary structured world state snapshot (`Value`).
3. **Standard Action Schema (`WorldAction`):**
   * Agent ID, action kind (verb), typed parameters JSON, and issuance timestamp.
4. Document the complete TWP specification in `docs/architecture/tetonic-world-protocol.md` with concrete JSON wire examples and provide verification tests.

## 3. Acceptance Criteria
- [x] Comprehensive TWP wire specification document created (`docs/architecture/tetonic-world-protocol.md`).
- [x] Framing and envelope schemas validate against `StreamMessage` serialization (`test_twp_wire_protocol_serialization_compliance`).
- [x] Wire contract is completely domain-neutral, with zero game-specific keywords in core domain crates.

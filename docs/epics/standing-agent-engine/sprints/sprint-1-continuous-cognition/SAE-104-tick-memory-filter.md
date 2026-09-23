# SAE-104: Continuous Sensory Filtering & Episodic Memory Boundary

**Epic:** Standing Agent Engine  
**Sprint:** Sprint 1 — Continuous Cognition  
**Layer:** `engine/core/tetonic-core` & `engine/strata/tetonic-memory`  
**Status:** Ready

---

## 1. Context & Objective
In turn-based agents, every interaction is pushed into `Conversation.messages`. In a continuous environment ticking multiple times per second, pushing every sensory tick into an append-only message array would exhaust the LLM's context window within minutes. Sensory ticks must be decoupled from the episodic memory record.

## 2. Requirements
1. Separate continuous sensory state from conversation / episodic history:
   * Sensory ticks are transient and evaluated in-flight by System 1.
   * Only **discrete milestone events** (e.g. external proposals, dialog with other agents, completed actions, environmental shifts) are committed to episodic memory.
2. Provide an `EventFilter` that discards repetitive or zero-delta ticks from durable logging.
3. Ensure context compilation for System 2 injects:
   * Current working state snapshot (inventory, location, immediate neighbors).
   * Recent milestone episodic memory items (last N significant events).
   * The overarching agent charter.

## 3. Acceptance Criteria
- [ ] Running 10,000 sensory ticks does not increase conversation message length if no milestone events occurred.
- [ ] Milestone events are reliably committed and recalled when System 2 is engaged.

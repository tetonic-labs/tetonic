# SAE-101: Re-export Universal Perception & World Adapter Types

**Epic:** Standing Agent Engine  
**Sprint:** Sprint 1 — Continuous Cognition  
**Layer:** `engine/core/tetonic-domain`  
**Status:** Complete

---

## 1. Context & Objective
The fundamental types for continuous agent execution (`Perception`, `Signal`, `WorldEvent`, `WorldState`, `WorldAction`, `ActionResult`, `WorldError`, and `WorldAdapter`) were drafted in `perception.rs` and `world_adapter.rs`. They must now be cleanly wired into `lib.rs` and exposed as public API of `tetonic-domain` without introducing any foreign inference dependencies.

## 2. Requirements
1. Re-export all perception and world adapter types from `tetonic-domain::lib.rs`.
2. Ensure strict `Send + Sync` guarantees on `WorldAdapter` and `Perception`.
3. Add comprehensive round-trip JSON serialization unit tests for `Perception` and `WorldAction` in `tetonic-domain`.
4. Ensure zero dependencies on `tetonic-inference`.

## 3. Acceptance Criteria
- [x] `cargo check -p tetonic-domain` passes cleanly.
- [x] Round-trip JSON tests pass for all perception types.

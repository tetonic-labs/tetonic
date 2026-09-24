# SAE-304: FleetSupervisor & Persistent Actor Lifecycles

**Epic:** Standing Agent Engine  
**Sprint:** Sprint 3 — Standing Fleet, Multi-Dimensional Intent & Steering  
**Layer:** `engine/mantle/tetonic-orchestrator`  
**Status:** Complete

---

## 1. Context & Objective
A standing fleet requires a supervisory runtime layer that continuously monitors agent lifecycles, health heartbeats, and cluster node status. If an individual agent actor loop panics or stalls, the `FleetSupervisor` automatically restarts it from durable state while keeping the rest of the fleet and organization operational.

## 2. Requirements
1. Implement `FleetSupervisor`:
   * Registry of running agents, their active squad bindings, and current lifecycle state (`Idle`, `Running`, `Paused`, `Estopped`, `Failed`).
   * Heartbeat monitor: tracks periodic health signals from running continuous loops.
   * Supervised actor spawning: spawns and monitors continuous agent loops.
2. Fleet-wide steering and safety controls:
   * `broadcast_steering(squad_id, vector)`: injects steering vectors across squad members.
   * `emergency_stop_fleet(reason)`: authoritatively trips E-Stop across all managed agents and world adapters.
   * `resume_fleet()`: clears E-Stop and resumes execution.
3. Real-time telemetry snapshot:
   * `fleet_snapshot()`: produces an aggregated status report of all organizations, squads, and agents for the Fleet Portal UI.

## 3. Acceptance Criteria
- [x] Stalled or panicked actor loops are detected by the supervisor.
- [x] Fleet-wide E-Stop halts all agents within 100ms.
- [x] Fleet snapshot returns an accurate real-time inventory of all standing agents.

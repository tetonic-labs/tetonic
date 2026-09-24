# SAE-501: Fleet Creation & Management API

**Epic:** Standing Agent Engine  
**Sprint:** Sprint 5 — Fleet Portal & Human Experience  
**Layer:** `engine/litho/tetonic-app`  
**Status:** Ready

---

## 1. Context & Objective
In line with the core axiom "Expert-level engine design, brain-dead easy UI", operators must be able to spin up organizations, squads, and continuous agents with minimal friction via a clean HTTP/REST and CLI surface.

## 2. Requirements
1. Implement RESTful endpoints in `tetonic-app`:
   * `POST /api/v1/orgs`: Create organization with budget quotas and default boundary profiles.
   * `GET /api/v1/orgs/:org_id`: Retrieve organization hierarchy, squads, and quota utilization.
   * `POST /api/v1/orgs/:org_id/squads`: Provision a squad aimed at a shared domain context.
   * `POST /api/v1/orgs/:org_id/squads/:squad_id/agents`: Launch standing continuous agent bound to designated world adapters.
2. Provide CLI bindings (`tetonic fleet up`, `tetonic fleet list`, `tetonic agent create`).
3. Validate request parameters, world adapter bindings, and budget allocations.

## 3. Acceptance Criteria
- [ ] Endpoints create organizations, squads, and agents with valid configuration.
- [ ] Over-budget creation requests are refused with descriptive error diagnostics.
- [ ] Spawned agents automatically register with the supervisor and start continuous actor loops.

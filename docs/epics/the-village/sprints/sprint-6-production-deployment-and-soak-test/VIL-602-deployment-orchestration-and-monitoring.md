# VIL-602: Deployment Orchestration, Health Checks & Observability

**Epic:** The Village — Living World Experiment  
**Sprint:** Sprint 6 — Production Deployment & 24/7 Operations  
**Layer:** Infrastructure & DevOps  
**Status:** Ready

---

## 1. Context & Objective
Provide automated deployment scripts, container definitions, process supervisors, and observability tooling to guarantee that the multi-process living world stays healthy and automatically recovers from unexpected disruptions.

## 2. Requirements
1. **Container & Process Orchestration:**
   * Provide production `Dockerfile`s and `docker-compose.prod.yml` coordinating:
     * `the-village-world`: Authoritative Colyseus game server.
     * `the-village-web`: Optimized static web client served via Caddy/Nginx.
     * `tetonic-gateway`: Agent inference gateway and WebSocket bridge.
   * Configure restart policies (`restart: unless-stopped`) and resource limits (CPU/RAM quotas).
2. **Health Check Endpoints:**
   * Authoritative world server: `GET /healthz` (checks active room, memory usage, tick rate).
   * Agent gateway: `GET /health` (checks fleet registration, SSE hub, upstream LLM provider ping).
3. **Automated Alerting & Circuit Breakers:**
   * Token budget circuit breaker: automatically halt System 2 escalations if hourly inference spend exceeds configured threshold.
   * Memory watcher: trigger graceful process reload if memory exceeds 1.5GB.
   * Uptime ping monitoring (e.g. BetterStack, UptimeRobot, or simple webhook).

## 3. Acceptance Criteria
- [ ] Reproducible single-command production deployment (`docker compose -f docker-compose.prod.yml up -d`).
- [ ] Health checks respond with accurate status and process metrics.
- [ ] Crash recovery verified: killed processes automatically restart and resume room state within 5 seconds.

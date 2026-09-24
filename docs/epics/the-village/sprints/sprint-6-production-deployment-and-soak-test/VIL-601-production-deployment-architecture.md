# VIL-601: Production Deployment Architecture & Hosting Strategy

**Epic:** The Village — Living World Experiment  
**Sprint:** Sprint 6 — Production Deployment & 24/7 Operations  
**Layer:** Infrastructure & DevOps  
**Status:** Ready

---

## 1. Context & Objective
Design and document the production deployment architecture for The Village. Because The Village involves continuous WebSocket state synchronization (Colyseus), real-time Server-Sent Events (Thought Streams), and continuous agent background processes, the hosting platform must support persistent, stateful processes with low latency and predictable costs.

## 2. Requirements
1. **Hosting Platform Evaluation & Selection:**
   * Evaluate cost, statefulness, and operational simplicity across:
     * Self-hosted Linux VM (GCP Compute Engine `e2-medium`/`e2-standard`, or Hetzner/DigitalOcean VPS).
     * Container platform (Docker Compose / Google Cloud Run with WebSockets enabled).
     * Edge CDN for static web client (Cloudflare Pages / Netlify / Vercel).
2. **Reverse Proxy & TLS Termination:**
   * Configure Caddy or Nginx with automatic Let's Encrypt SSL/TLS.
   * Route HTTP/HTTPS for static client, REST API (`/api/v1/*`), and SSE thought streams (`/api/v1/agents/:id/thoughts`).
   * Route persistent WebSocket connections (`/colyseus/*`) with proper upgrade headers and keep-alive timeouts.
3. **Environment & Secrets Management:**
   * Centralized configuration for LLM API keys (Groq, Anthropic, Google Gemini), server port bindings, and CORS origins.
   * Zero secrets checked into git repositories.
4. **State Persistence & Disaster Recovery:**
   * Persistent volume mounting for Tiled map mutations, agent episodic memory sqlite databases, and community asset codex.
   * Automated snapshot/backup routine for world state.

## 3. Acceptance Criteria
- [ ] Complete production deployment architectural specification approved.
- [ ] Reverse proxy and SSL configuration template created and tested.
- [ ] Secrets management and persistent storage paths formalized.

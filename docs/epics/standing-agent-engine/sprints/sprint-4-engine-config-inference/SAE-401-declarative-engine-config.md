# SAE-401: Declarative Engine Configuration (`tetonic.toml`)

**Epic:** Standing Agent Engine  
**Sprint:** Sprint 4 — Engine Configuration & Decoupled Inference  
**Layer:** `engine/core/tetonic-domain` & `engine/core/tetonic-core`  
**Status:** Ready

---

## 1. Context & Objective
To uphold the "1-to-1,000 Scale Invariant", the engine must be configured declaratively via `tetonic.toml`. The exact same binary must run identically for a junior developer on a laptop (zero-config standalone default) or in a 1,000-node Kubernetes cluster.

## 2. Requirements
1. Implement `EngineConfig` parser in `tetonic-core` (or `tetonic-domain`):
   * `[node]`: `id`, `mode` (`Standalone`, `Coordinator`, `Runner`), `bind_addr`, `coordinator_url`.
   * `[storage]`: `storage_mode` (`LocalSqlite`, `DistributedDb`, `MoveableVolume`), `volume_mount_path`, `checkpoint_interval_secs`.
   * `[inference]`: `provider` (`Local`, `RemoteFabric`, `Cloud`), `endpoint_url`, `default_model`, `reflex_model`, `timeout_ms`.
2. Support loading from:
   * Explicit path (`--config tetonic.toml`).
   * Working directory (`./tetonic.toml`).
   * Environment variable overrides (`TETONIC_NODE_MODE`, `TETONIC_INFERENCE_ENDPOINT`).
   * Sensible zero-config desktop defaults when no file is present.

## 3. Acceptance Criteria
- [ ] Successfully parses complete multi-section `tetonic.toml`.
- [ ] Returns valid desktop standalone defaults when no config file exists.
- [ ] Environment variable overrides take precedence over file configuration.

# Capacity profile — v1

**Status:** ES5-0 (schema + types; daemon integration ES5-1+)  
**Owner:** `engine/crates/lokai-capacity`  
**Relates to:** [`inference-fabric-v1.md`](./inference-fabric-v1.md), [`estate-v1.md`](./estate-v1.md), [`performance-and-scale.md`](../performance-and-scale.md)

## Purpose

A **RuntimeProfile** binds hardware identity, an inference recipe, **observed placement** at bench time, and gate evidence. Profiles are **immutable**; activation changes a pointer in `capacity_bindings`.

## Schema version

All documents include `"schema_version": 1`. Breaking changes bump the version; readers reject unknown versions.

## RuntimeProfile

```json
{
  "schema_version": 1,
  "id": "profile_20260628_p40_coder",
  "label": "P40 floor — coder",
  "created_at": "2026-06-28T12:00:00Z",
  "node_id": "local",
  "role": "coder",
  "source": "import",
  "hardware": {
    "fingerprint": "sha256:…",
    "gpus": [
      {
        "name": "Tesla P40",
        "uuid": "GPU-…",
        "bus_id": "00000000:03:00.0",
        "vram_total_mb": 24576,
        "role": "compute"
      }
    ],
    "cpu_cores": 8,
    "ram_mb": 32768,
    "os": "windows-x86_64",
    "ollama_version": "0.6.0"
  },
  "recipe": {
    "base_model": "qwen3.6:latest",
    "estate_model": "qwen3.6-estate",
    "num_ctx": 4096,
    "num_gpu": 999,
    "keep_alive": "30m",
    "modelfile_path": null,
    "env_hints": [
      { "name": "CUDA_VISIBLE_DEVICES", "value": "0", "apply": false, "rationale": "Target P40 only" }
    ]
  },
  "observed": {
    "processor_split": "85%/15%",
    "vram_used_mb": 18432,
    "resident_model": "qwen3.6-estate",
    "load_wall_s": 12.5,
    "measured_at": "2026-06-28T12:05:00Z"
  },
  "metrics": {
    "raw_short_wall_s": 8.2,
    "raw_short_tps": 42.0,
    "tool_payload_prefill_tps": 0.0,
    "raw_long_prefill_tps": 0.0
  },
  "gates_passed": true
}
```

### Fields

| Field | Required | Notes |
|-------|----------|-------|
| `observed` | yes | Ground truth from `/api/ps` + NVML at bench — doctor compares live vs this |
| `recipe.env_hints[].apply` | — | v1: always `false` (document only) |
| `role` | yes | `coder` \| `fast` \| `hard` \| `embed` — floor tier often maps coder=hard |

## BenchReport (`bench-report-v1`)

Output of microbench / `infer_gate.py`:

```json
{
  "schema_version": 1,
  "model": "qwen3.6-estate",
  "ollama_base": "http://127.0.0.1:11434",
  "suites": {
    "raw_short": {
      "wall_s": 8.2,
      "prompt_tokens": 12,
      "eval_tokens": 8,
      "prefill_tps": 180.0,
      "decode_tps": 42.0
    }
  },
  "observed": { "...": "same shape as profile.observed" },
  "gates": {
    "passed": true,
    "failures": []
  }
}
```

## GatePolicy (floor tier defaults)

| Gate | Warn | Fail |
|------|------|------|
| `raw_short.wall_s` | > 15 | > 30 |
| GPU processor share (from `observed.processor_split`) | < 50% for models > 10GB | < 30% |
| `raw_long` prefill | — | wall > 120s (when run) |

Thresholds scale with total compute VRAM in ES5-2+.

## CapacityStatus (RPC summary, ES5-1+)

```json
{
  "completed": true,
  "stale": false,
  "doctor": "healthy",
  "active_profile_id": "profile_…",
  "active_profile_label": "P40 coder",
  "hardware_summary": "Tesla P40 24GB",
  "gates_ok": true,
  "last_setup_at": "2026-06-28T12:05:00Z"
}
```

`doctor`: `healthy` \| `degraded` \| `unknown` \| `no_profile`

`profile_model` / `profile_base_model` (optional): the saved recipe tags. Doctor and `/capacity` describe this **profile default**, not necessarily the live session.

## Turn admission (H2-1)

Interactive turns go through `lokai_app::admit_chat_turn`. CLI (`chat.rs`) and daemon (`chat/send`) both snapshot `CapacityService::snapshot_admission_status` (saved profile rows only — no per-turn Ollama or NVML) and render the same kernel outcome.

| Capacity state | Kernel outcome |
|----------------|----------------|
| No saved profile | Admit, no warning |
| Healthy profile matching this chat | Admit, no warning |
| Degraded / `gates_ok=false`, or this chat's `--model` is a different family from the profile | Admit + `CapacityGateWarning` (warn and proceed) |
| Optimize in progress | Reject `TurnAdmitError::CapacityBusy` (daemon flag; not a doctor gate) |

Inference still fail-closes on actual VRAM spill. `TurnAdmitError::CapacityGate` is unused while policy is warn-and-proceed.

## Session `--model` vs profile default

`--model` (and the TUI header) is what the current chat runs. Optimize writes a default for sessions that omit `--model`.

- Interactive chat **must not abort** a turn because the saved profile failed gates, including when the session model is a different family (`qwen3.5:latest` vs `qwen3.6-estate`).
- Inference still fail-closes on VRAM spill for the model actually loaded.
- `/doctor` and `/capacity` show both lines: **This chat** and **Profile default**.
- Estate tags are `{stem}-estate` once. Re-optimize must not stack (`qwen3.6-estate:latest` is not a new base model).

## Storage (ES5-1+)

See [`memory-store-v2.md`](./memory-store-v2.md) — tables `runtime_profiles`, `capacity_bindings`, `capacity_jobs`.

**Coordinator:** `lokai.db` — node id `local`.  
**Worker:** `worker.db` (schema v3) — same table shapes; profiles keyed with `node_id = "local"` on the worker box. Fabric identifies workers by enrollment id.

## Worker fabric (ES5-4)

`GET /v1/capacity/status` on the worker fabric returns `WorkerCapacityWire` (same fields as `CapacityStatus` summary). The coordinator remote probe maps this into `NodeInfo.capacity` for pooled placement; hard tier prefers workers with `gates_ok`.

CLI (coordinator): `lokai estate capacity status --worker <enrollment_id|label>` and `doctor --worker …` (read-only via mTLS fabric).

## RPC methods (ES5-1 … ES5-3)

| Method | Sprint |
|--------|--------|
| `estate/capacity/status` | ES5-1 |
| `estate/capacity/doctor` | ES5-1 |
| `estate/capacity/optimize` | ES5-2 |
| `estate/capacity/profiles/*` | ES5-3 |
| `estate/capacity/jobs/get` | ES5-3 |
| `estate/capacity/jobs/cancel` | ES5-3 (alias: `estate/capacity/cancel`) |

CLI alias: `lokai estate setup *` → same handlers.

**Last updated:** 2026-08-17

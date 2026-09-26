# Sprint 2 — One runtime, real effects and useful first work

Status: **exited (local MVP preview).** Registered general jobs submit through the application run service and existing managed executor. Durable retries, host deadlines, identity concurrency and org/principal/team admission ceilings share the run journal; a full ceiling rejects preparation (no silent queue). A default effectful admission records one attempt. Local `tetonic job run` (optional `--view`) is the thin setup activation: it launches a real agent and shows terminal outcome plus journal event names. A noncoding recall job runs with no repository. Managed submissions can run a world adapter with deny and cancel. Cancel stops an owned process. Running begins only when execution is claimed. Registered jobs reject `run_shell` before inference; recall and workspace-jailed file tools are the supported profiles. Coding sessions keep optional deadlines and the legacy coding identity until later cutover; general work uses `submit_registered_job` and does not require a repo.

Deferred (later stages): remote multi-user setup UI (MVP-601); cumulative team token/money ledgers and waiting/fair queues (MVP-302/402); OS filesystem jail on Windows; deleting the standalone server world loop (D07/D08) — Village stays outside this repo and that cutover is not required for this exit. Depends on sprint 1. See [implementation progress](../../progress.md).

## MVP-201 — Connect activation to managed local harness execution

Inject through the existing LocalAgentAttemptExecutor boundary; preserve claim, identity, policy, thread-affinity and quiescence semantics. Create durable activations with bounded concurrency, time, tokens and queues. Local worker reports observations; manager validates outcomes. Enforce supported process, credential and egress isolation before running model-requested tools.

Acceptance: the setup UI can activate a real agent and inspect actual events; failed preparation never reports Running; stop reaches owned processes; no uncontrolled direct effect or inference bypass. Exactly one active attempt by default for effectful work.

**Exit evidence (thin local setup):** `tetonic job run` + receipt/`--view`; Running only at claim; `managed_cancel_stops_the_owned_process`; admission ceilings reject when full; registered shell refused before inference.

## MVP-202 — Make coding optional and move the world path onto the runtime

Extract coding roles, prompts, critic/router and toolsets behind a selected harness/capability composition. Route world effects through authorization on the managed path. Prove a noncoding external-tool workload with no repository requirement.

Acceptance: one lifecycle works for both workloads; no mandatory coding identity/repo/tools; idle cancellation works; event receipts survive supported retries; denied effects never reach destinations. Preserve no-omniscience observations. Village game code stays outside this repo.

**Exit evidence:** `noncoding_recall_job_runs_without_a_repository`; registered general harness; managed world adapter allow/deny/cancel; durable receipts on retry. Standalone `tetonic-server` world loop remains a deferred compatibility path (D07/D08), not the product activation door.

Reuse: REC-201/202. Remove D07/D08 only after a later parity decision. Retain useful adapters and test fixtures.

Commit discipline: characterization/contract, replacement behavior, caller cutover, then gated removal.

# EXP-001–003: first implementation slice

Date: October 8, 2026, America/New_York. Planning baseline: `208e848`.
Engine implementation: `9b93c69`; UI implementation: `82816fd`. Product acceptance remains open for all three
tickets. This is progress toward the additional release gate, not a scope reduction.

## What changed

### EXP-001 — Begin with a conversation

- The empty map has one starting prompt and defaults to the existing Guide.
  Choosing a worker/team remains available. Saved agent identities are retained.
- Orientation has authoritative product/navigation context. New worker defaults
  no longer assume every request is a coding task. The Guide is explicitly told
  that worker tools are not its own tools. Actual execution still uses the same
  managed run and planning capabilities.
- A proposal form is disclosed by a deliberate action instead of dominating
  an unfinished discussion. Existing saved proposals remain reviewable inline.
- During review, the owner rejected both a side drawer and a page-like discussion
  surface. The implemented direction grows the conversation upward from the map
  composer, slightly wider, on one subtly translucent ink surface. Compact avatar,
  name and message rows replace the separated human-message card treatment.
  Minimize/resume retains the same discussion and unsent draft. The input grows
  vertically within a viewport bound. This also advances part of EXP-005; it does
  not close that ticket's full reader, settings, and budget-presentation scope.
- Restored the existing palette picker to the live workspace header with presets,
  custom colors and existing preference keys. It was present but unmounted, with
  its presentation styles absent from the current production entry. Classic
  Tetonic remains the fallback when no preference is saved.

### EXP-002 — Relevant folders and finite working limits

- Added startup-configured `agent_folders` and an agent-editor selector. The owner
  chooses an exact canonical folder from the host-approved catalog. It is stored
  in the existing agent definition revision using the existing compare-and-swap
  edit path. No second permission store was introduced.
- Future admission revalidates the selected folder. Removing it from the host
  does not silently substitute the default. Previous agent revisions and prepared
  settings retain their scope. Additional folder candidates exclude whole-drive,
  recognized credential/control, database-containing and configured artifact/log
  paths. The legacy explicitly supplied base folder keeps its existing behavior.
- Folder selection and hosted disclosure consent are separate. Changing the
  selection invalidates the UI consent; the engine checks the exact consented root.
- Ordinary file discovery/read operations exclude `.lokai` and `.tetonic` runtime
  control directories through the existing tool protections. This is not an
  OS-wide shell sandbox or a migration of all existing runtime storage.
- Default host ceilings are now 32 steps/600 seconds; newly created UI agents
  start at 16 steps/300 seconds, capped by operator settings. Saved per-agent
  limits remain unchanged. Token and higher-level allowance enforcement remain.
  These settings are provisional until a useful reference profile is qualified.
- Setup and agent details expose the working folder and explain host versus
  per-agent limits. Adding a *new host-approved folder* still requires a host
  configuration edit/restart; this slice provides selection, not an in-app host
  configuration manager. See [host configuration](../../../../architecture/host-configuration.md).

### EXP-003 — Connect, discover, select

- Provider-key setup precedes the model control. Saving a key refreshes discovery
  without discarding the draft. Searchable display names lead; exact IDs remain
  available. Manual IDs and external catalog links are advanced/fallback options.
- Missing credentials, authentication rejection, account access denial, rate
  limiting, discovery failure and empty catalogs retain distinct recovery paths.
  Typed HTTP status propagates through existing egress/inference adapters;
  provider error bodies and secrets are not exposed by discovery errors.
- Agent tool selection and new provider-disclosure requirements are preserved.
  Existing vendor capability/harness limitations have not changed.

## Verification

| Check | Result |
|---|---|
| `tetonic-app --lib` | 176 passed, 4 existing ignored |
| `tetonic-tools --lib` | 60 passed, 1 existing ignored |
| `tetonic-egress --lib` | 27 passed, 1 existing ignored |
| `tetonic-inference --lib` | 151 passed, 2 existing ignored |
| Web suite after palette integration | 215 passed, 31 files |
| Web architecture tests / static boundaries | 8 passed / passed |
| TypeScript | Passed |
| Rust `tetonic-arch-gate verify package` | Passed formatting, workspace/all-target Clippy, architecture and static quality |
| Production UI build | Passed including palette integration; existing large-chunk warning |
| Browser | Desktop first-use, discussion surface/message layout, palette control and custom builder inspected; owner supplied live feedback |

The folder integration test exercises actual managed file-tool execution with a
controlled inference transport: the selected folder's evidence is received and a
different folder's canary is absent. It also checks stale edits, pinned previous
configuration, changed hosted consent, restart persistence and removed-root
rejection. This proves the exercised authority path, not real-model usefulness.

## Real-model trials and limitations

The original review database at `.lokai/first-use-review-20261008` was retained.
The initial repair preview uses `.lokai/first-use-repair-20261008`, engine 3006,
UI 5180. Once the owner began using that preview, final model checks moved to a
separate `model-checks/workspace.db` on engine 3007 with no visible UI. That
validation-only engine was stopped afterward; the preview remains available.

Windows; Ryzen 5 5600X; GTX 970 and Tesla P40 reported by the host (not proof of
which GPU served each inference). Model `qwen3.5:latest`, existing local Ollama
route. Two final requests each had a 6,144-token work envelope, under the host's
12,288-token ceiling. No frontier credential or paid provider was used.

| Trial | Recorded run duration¹ | Reported input/output tokens | Result |
|---|---|---|---|
| Unseen product-orientation phrasing | 44.05 s | 2,042 / 461 | Explained teams, delegation and the map; no plan or worker dispatch. Still incorrectly claimed personal file-tool access. **Partial/fail for accurate orientation.** |
| Seed swap versus book exchange, discussion only | 41.94 s | 2,052 / 386 | Compared noncoding alternatives and asked about priorities; no execution plan or assignments. Generic assumptions remain model output, not verified facts. |

¹ Work creation to final run-projection timestamp, not time to first token.
Two final trials: **4,941 reported tokens**, one inference call each, zero unknown
or pending usage calls. An earlier orientation trial consumed 2,381 tokens and
incorrectly introduced the workspace mainly as a file assistant. Total these
three automated model trials: **7,322 tokens**. Owner preview interactions are
separate; a capacity-rejected automated submission on 3006 remained not started
and was not retried there. A temporary test allowance on that preview was restored.

The local model's tool-ownership claim is misleading even though the runtime
does not give the Guide file tools. Forty-second short replies are also not a
qualified first-use latency target. Do not mark EXP-001 accepted from these runs.

## Still required

### October 9 follow-up: conversations are not dispatched work

- The workspace projection now separates durable `purpose: explore` conversations
  from assignments. Guide discussions and unstarted proposals stay out of the map,
  work counts, work shelf, and work attention list. Failed replies remain visible
  in the conversation and its picker. Existing records are preserved.
- Conversations are available beside the floating composer and in the Guide
  header. The selected conversation and its draft survive navigation and reload
  in the same tab; the transcript remains stored by the engine.
- Proposals follow the discussion inline without collapsing it. Starting a reviewed
  plan uses the existing agreement/admission/dispatch path; only its execution
  records populate the map. A direct action opens that team's map.
- Private Guide transcripts do not become shared blackboard entries after launch.
  The execution's agreed direction, contributions and tool output remain visible.
- No new conversation store, scheduler, planner, or dispatch authority was added.
  This does not change the existing one-execution-per-plan-source restriction.
- Validation: 219 web tests, 8 architecture tests, frontend boundary check,
  production build, and 5 engine Director tests passed. UI integration covers
  proposal → explicit start → coordination/assignment map → return to the Guide.
  Browser review used existing conversations without submitting new inference.
- UI changes are live on 5180. The engine's updated navigation description is a
  source change for the next engine restart; the running preview was kept alive
  while the owner was configuring and testing it. No new real-model qualification
  or frontier inference trial was performed for this UI follow-up.

### Remaining acceptance work

#### October 9: Guide planning density and frontier tool continuation

- Keep the Guide conversation as the main surface. A saved proposal or execution
  appears as one summary with its title, assignments, actual status and next action.
  Full review, budget details, history and recovery remain available inline on
  request; review does not start work. Returning to discussion retains the draft.
  Direction shared for planning is editable immediately instead of behind another
  disclosure. Work failures and human requests remain visible in the summary.
- The live toy plan stopped before dispatching its assignments. Recorded tool
  output reported a checkpoint/dispatch-receipt failure, then hosted continuation
  failed a secret scan. Responses carries encrypted reasoning and opaque IDs;
  the generic scanners treated this provider protocol data as local secrets.
- Hosted disclosure scanning now distinguishes exact private Responses protocol
  fields from all conversation text, tool arguments/results and schemas. Protected
  local checkpoints have a distinct artifact kind, require Secret classification,
  reject remote-worker declarations, enforce a 2 MiB bound and run a complete,
  validated content scan before sealing. Ordinary artifacts retain their existing
  scanning path. Invalid checkpoint content and scanner failures fail closed.
  Existing checkpoint references remain readable; no failed work is auto-replayed.
- Coverage includes provider ciphertext through parallel hosted coordination,
  selected tools and human handoff; actual content still blocks disclosure;
  malformed/checkpoint-size/UTF-8 boundaries; saved review state and chat drafts.
  These are deterministic integration checks, not a claim that a live provider
  completed the user's original toy plan.

#### October 9: stuck Guide diagnosis and recovery

- A Guide reply admitted before a preview-engine restart was canceled afterward,
  without a surviving executor to acknowledge quiescence. Its durable capacity
  hold therefore blocked later Guide messages. After verifying the old process
  had exited and the job had only `work_plan`, an offline, backed-up repair used
  the existing supervisor's quiescence command. No projection flag was edited.
  Automatic recovery of arbitrary interrupted effects remains out of scope.
- Retries of rejected admission previously reused a grant for an older workspace
  observation/model, causing `registered job access denied`. An explicit retry
  now refreshes preparation only if no durable activation exists. Existing jobs
  cannot be replayed or replaced; revoked base grants remain denied.
- Saved, unstarted discussions now say that the reply never started and offer
  an idempotent retry. Draft follow-up text is retained. Canceled Guide replies
  with unacknowledged executor exit show canceling/recovery rather than stopped.
- Live frontier checks reached the configured `o3-mini` provider, which returned
  `credit_balance_exhausted`. The streaming adapter had masked this as an invalid
  stream. Safe error-code handling and actionable billing/limit messages now
  preserve the cause without exposing provider messages or submitted context.
  No successful frontier answer was observed; no further inference was attempted
  after identifying the credit error. Provider/model settings were preserved.
- Validation: 43 work tests passed (3 live-scenario tests intentionally ignored),
  14 Guide UI tests passed, 3 Responses-stream tests passed, and TypeScript passed.
  Regression coverage includes immutable grants, revoked grants, replay prevention,
  saved-message retries, draft retention, and safe provider-error presentation.

1. Accurate, timely orientation and a follow-up through a qualified reference
   model/profile; preserve discussion without invented capabilities or plans.
2. Real-model missing-folder recovery and useful selected-folder work. Finish the
   owner-facing host-folder setup route if configuration/restart is too burdensome.
3. A real authenticated provider connection/catalog/selection trial. Mocked
   discovery and tool parity cannot establish account availability or compatibility.
4. Narrow viewport, both themes, reduced-motion and full long-discussion/recovery
   acceptance. Desktop review is only part of this evidence.
5. EXP-004–009, including planning allocation, visible team participation,
   comprehension, archive/restore and the complete fresh-user journey, remain open.

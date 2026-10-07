# The Guide can create and revise real proposals

Date: October 7, 2026. Baseline: `ba413c0c`.

## Product change

The map's Guide can now turn a planning conversation into a saved team proposal and revise it in a follow-up. The proposal appears in the existing conversation, without pressing Prepare a plan or navigating elsewhere. Short answers still do not require a plan. The existing Prepare control remains available as a fallback.

The Guide can also inspect the current conversation's plan, readiness, execution state and bounded result excerpts. Starting the team remains the owner's explicit **Start this plan** action. Planning does not launch workers or modify a running team's assignments.

The Guide retains conversational completion: a plain-text answer can finish a small question or a planning reply. Its bound control tool remains available in that mode. Dispatchers and effectful workers retain their existing completion rules. Requiring a completion tool for every Guide answer would regress the quick-answer journey.

This is an MVP product slice. It does not add release automation, another UI, a replacement scheduler, another agent registry or another budget ledger.

## Integration

`work_plan` is an internal managed tool with two operations: `inspect` and `propose`. It uses the existing asynchronous control-tool boundary in the general harness. The local host binds it to one active Guide turn, its conversation root and the plan/brief revisions observed at admission. The model cannot choose a different conversation, supply authority or invoke start/agree/grant operations.

The advertised outer shape always requires operation, direction and plan, using null placeholders for inspection. The proposal schema enumerates the available saved agent keys. At the boundary, an exact, unique display name can also resolve to a canonical saved key; unknown or ambiguous names are rejected. Resolution stays inside the authorized working-agent roster and cannot select the Guide or coordinator as a worker. Saved proposals always retain canonical identities.

Proposals reuse the existing versioned WorkBrief and HuddlePlan resource services, PlanContent validation, saved agent roster and readiness checks. They are saved directly from the Guide's tool call; there is no second planning model run. A distinct provenance identifier keeps the actual Guide conversation visible instead of projecting it as a hidden generation job.

Existing agreement, pinned agent definitions, permission checks, budget reservations, parallel dispatch, human questions and result projections still handle execution. The managed Guide remains local to the existing service/model configuration; this does not add a frontier-provider selector to the Guide.

## Boundaries

- One accepted proposal per reply, with a deterministic operation identity. Identical calls reuse the receipt; changed repeated calls are rejected.
- A saved owner edit made after turn admission prevents the Guide from overwriting that version. Unsaved UI edits remain visible and cannot be applied over a newer revision without explicit reconciliation.
- Brief save and plan save remain separate resource operations, not a new atomic transaction. A failure between them can leave a saved brief without a new plan. A successful plan save returns a compact write receipt, independently of subsequent snapshot availability.
- Inspection reads this conversation's saved plan and execution projection and bounds result excerpts. It does not repeat the unrelated-work observation already supplied at turn admission. It is not access to other private discussions or proof of healthy external connections. Oversized inspection snapshots fail explicitly.
- Turn context includes a bounded summary of this conversation's saved proposal. Before execution, it explicitly reports `plan_started: false`, zero execution records and no unrelated task rows. Workspace totals remain separately labeled. This prevents completed work elsewhere from becoming apparent progress on an unstarted proposal.
- The proposal's shared direction is selected by the model from the conversation. Instructions tell it to omit unrelated private material; this is not a guaranteed semantic redaction mechanism. The owner can inspect the saved direction and proposal before execution.
- Late operations after the Guide turn stops are rejected. Already admitted writes are not claimed to be atomically cancelable.

## Allowances

Live testing exposed the old 4,096-token host ceiling as insufficient for a planning call plus a reply with repeated context. The local host ceiling is now 12,288 reported tokens. An unconfigured engine-managed Guide defaults to that allowance; unconfigured workers and the coordinator retain 4,096. Creating a worker in the UI still defaults to 4,096, while explicit larger settings can fit within the host ceiling. Coordination readiness retains its 4,096-token ceiling.

Saved agent preferences and lower workspace allowances still apply. Neither the planning tool nor retries increase an allowance. Already funded work retains its original amount. Reported tokens include input and output and remain visible in Usage; these allowances are not monetary billing caps.

## Verification

- Final application library: **232 passed**, 3 existing opt-in live tests ignored. Clippy with warnings denied and the final CLI build passed.
- Web: 157 passed across 26 files; web and CLI builds passed. Existing Vite chunk-size advisory remains.
- Managed inference fixture exercises real tool advertisement, receipt delivery, proposal creation, follow-up revision, persistence across reopen and request replay without another model call.
- Negative cases cover unknown fields/operations, incomplete arguments, nonexistent agents, owner-edit conflicts, repeated changed proposals and completed-turn operations.
- Agent-name resolution is tested for unique names, ambiguous names and exact-key selection. Final focused director tests and Clippy passed after the live-trial corrections.
- Tests verify that a lower workspace cap still funds the Guide at that cap, workers keep their old defaults, and no worker or separate planning job is launched by proposal creation.
- Managed fixtures cover a natural answer without any proposal and a natural answer after a proposal tool call, as well as explicit `finish` completion. Proposal-context tests inject unrelated completed work and verify that it is excluded from the unstarted plan's progress.
- UI tests verify automatic proposal refresh at Guide completion, no automatic start, and preservation of unsaved edits when a newer proposal arrives.

## Local model trial

The first trial with `qwen3.5:latest` omitted the required shared direction and used a display name in place of an agent key. The invalid call saved no plan and dispatched no work. Its retry exhausted the inherited 4,096-token allowance. This led to precise missing-field feedback, clearer exact-key instructions and the bounded Guide allowance change above. Historical failed test work remains recorded.

The first successfully saved proposal used source conversation `65abdf38-9c0a-495f-a89d-35fbd9f4e50e`. A follow-up changed six volunteers to four, included setup time in both independent assignments, and saved revision 2 with 1,500 tokens per worker plus 3,000 for coordination. Both versions persisted across a service restart and had no execution receipt. These two replies spent time correcting display-name/key mistakes and reached the existing 120-second deadline after saving. The saved proposal remained visible and usable; a timed-out reply did not erase an accepted write. That prompted unique-name resolution rather than increasing the deadline.

A subsequent malformed proposal reached the deadline without saving; revision 2 remained authoritative. A status question also exposed missing draft-plan focus in the observation and an incorrect completion mode in the new Guide wiring. Both were corrected: saved proposals now have explicit scoped context, and plain-text Guide answers complete normally. These are engine integration defects, separate from model argument errors and local inference latency.

The final live status question completed successfully on the rebuilt engine. It correctly identified Local assistant and Reviewer, four volunteers in saved revision 2, independent assignments, and that execution had not started. No plan or agent was changed by that question. Compact evidence and the UI screenshot are retained locally in `.lokai/manual-testing/guide-planning-evidence.json` and `guide-planning.jpg`.

The tool path is implemented and covered by managed execution tests, and live proposal creation/revision persisted correctly. The current local model can still omit required fields and consume the 120-second reply deadline while correcting them; this trial does not establish reliable success for arbitrary planning requests. The final build retains explicit failures and the last valid proposal. A stronger, configurable Guide model remains a product follow-up, alongside broader MVP capabilities outside this slice.

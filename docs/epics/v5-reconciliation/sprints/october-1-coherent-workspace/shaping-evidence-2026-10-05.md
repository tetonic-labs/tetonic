# First shaping implementation — October 5, 2026

Historical first-slice evidence. The UI entry and sample-map limitations below
were superseded by the [team-view cutover](team-view-cutover-2026-10-05.md): both
URLs now use the same production team view and real engine records.

Status: verified first slice of OCT-103/104, not parent or sprint completion.
Base HEAD `a030db8` with extensive pre-existing tracked/untracked work. This slice
is also uncommitted. No unrelated work was discarded or swept into a commit.

## Implemented

- The accepted UI target is **`/dev/team-work/`**, not the older root workspace.
  Its floating composer opens **Shape work** in the existing map inspector, with
  saved discussions, real Guide replies and an editable working brief. It reuses
  the local engine provider, draft handling and brief component. The registered
  Guide uses the existing general harness, execution grant, managed admission,
  broker, run inspector and durable conversation lineage. The surrounding map
  remains explicitly sample data; no example agent, project, tool or assignment
  enters the Guide's requests. The initial implementation was incorrectly placed
  in the older workspace; the user corrected that target and integration now
  continues in team-work.
- Exploration purpose is persisted on existing team work. It cannot silently
  switch to execution on a reply or an idempotent retry. The local host requires
  the configured no-tools Guide and removes filesystem/tool authority for these
  runs. `finish` remains available for producing an answer. Inference uses the
  same bounded run and cancellation machinery; it is not free preprocessing.
- The human can edit and save a working brief on the conversation's root work.
  Schema v53 adds append-only revisions to the existing database. ResourceService
  checks ReadTeam/ManageTeam authority; storage rechecks participation. Revision
  comparison prevents silent overwrite. Exact retries return the original receipt.
  Save does not invoke a model, accept a huddle, dispatch work or grant access.
- Follow-up inference receives the latest saved brief from the authorized server
  reader, explicitly labeled draft rather than execution authority. The original
  request and earlier revisions remain available. Model text enters the editor
  only when the human explicitly adopts it.
- Unsaved edits and uncertain save identities remain scoped to the local browser
  session. A lost response locks that edit until retry resolves it; stale edits
  require explicit review. A retry can recover its receipt even if another editor
  saved a newer revision. Exploration has distinct map status and counts.

## Validation

- All 131 memory unit tests pass, including upgrade/crash/replay coverage.
  Broad validation caught and fixed duplicate-column handling during migration
  replay; no test was disabled. The new tests cover scope, retries, conflicting
  revisions, legacy work, restart and saving without run creation.
- All nine local workspace integration tests pass. The shaping test drives a
  protocol fixture through the real broker/runtime while file tools are granted
  elsewhere, proves only `finish` is advertised, rejects a different agent/purpose,
  preserves the brief after reopen and replays an admitted request without a
  second inference. Fixtures test contracts, not model usefulness.
- Four local HTTP adapter tests pass, including explicit purpose decoding,
  rejection of authority fields, and token/Host/Origin enforcement.
- Architecture and static quality gates pass. Quality initially misclassified
  model literals in the separate local workspace test module; its test functions
  now explicitly carry `cfg(test)` in addition to the parent module's existing
  guard. Production model routing and the checker are unchanged.
- All 176 frontend tests in 28 files pass. After the final copy/status refinements,
  the 31 affected workspace/client tests pass again. Production TypeScript/Vite
  build and CLI build pass. No full Rust workspace or release gate claim is made.
- Actual browser test uses local `qwen3.5:latest`: an unclear customer-feedback
  problem produced an actual comparison; an owner-authored brief saved without
  additional inference/dispatch. Page reload and engine restart retained both
  the conversation and revision 1. The first response was too verbose; the Guide
  instructions now ask for a concise useful distinction and at most two questions.
- After restart, a second actual model turn correctly identified the pilot name,
  the chosen ticket-review approach and the two-hour constraint, which were only
  present in the saved brief. An additional human decision saved as revision 2;
  earlier revisions remained available. No specialist was dispatched.
- Browser review at the normal 1280x720 viewport and 390x844 verified the brief
  editor, save acknowledgement and retained conversation. Narrow brief inspection
  now gives the editor more vertical room. Document width stayed 390px. The
  original brand/map shell remains. Screenshots are local review artifacts under
  `.lokai/ui-consolidation-check/shaping-{desktop,mobile}-2026-10-05.png`.
  The final workspace interaction tests pass (18); the final production build
  passes. These are developer checks, not fresh-user usability evidence.
- Following the UI-target correction, all 12 team-work tests pass (eight existing
  example tests plus four live-adapter tests). These cover configured Guide routing,
  fixture isolation, saved discussion/brief editing without dispatch, uncertain
  send identity across panel closure and disconnected drafting. The dedicated
  team-work TypeScript check passes.
- In the actual team-work browser, the existing discussion and saved brief were
  resumed, and a new human-authored decision saved through the real engine as
  revision 3. The discussion contains the prior actual local-model responses.
  The 390x844 layout exposes the editor, save and composer without horizontal
  overflow (document width 390px). The normal viewport was restored and the
  correct page left open. Current UI evidence:
  `.lokai/ui-consolidation-check/team-work-shaping-2026-10-05.png`.

## Still open

Accepted plan revisions are not yet bound to dispatched assignments. Governed
child admission, actual multi-agent collaboration and its inherited controls
remain COORD-A/B work. No fixed Research/Analysis decomposition was connected.

Skill authoring/import/download/export, version/source/scope management and exact
runtime binding remain OCT-102 work. There is no working skills catalog yet.
Durable capability requests, connection validation and selective resumption
remain OCT-105 work; the Guide can describe missing context and alternatives,
but cannot connect a service or resolve permissions. It currently has no external
research tools. This slice is the local owner profile, not shared human rooms.

Conversation context retains the existing 64-ancestor/12,000-byte bound, including
the brief and next prompt; a long history/brief is rejected explicitly rather than
silently truncated. Briefs can be saved up to 12,000 bytes, so a large saved brief
can prevent a follow-up until shortened. Better bounded context assembly and
continuation from the latest brief are needed before claiming long-lived shaping.
The UI reader shows the latest 50 brief revisions. Fresh-user validation, release
installation and the October 6 capacity/scope review remain outstanding.

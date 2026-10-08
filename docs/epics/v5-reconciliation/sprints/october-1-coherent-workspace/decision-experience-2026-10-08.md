# Human decisions beside their work

Date: October 8, 2026. Parent: OCT-105. Product tasks: 5.1, 5.2 and the receipt/observed-state portion of 5.4. Baseline: `368f9b7`. This is a local product implementation, not full sprint or MVP acceptance.

## User problem and delivered behavior

Needs you previously hid questions and exact command proposals inside expandable rows. Command approvals were available in the inbox but not directly beside the work that needed them. Polling could remove a request immediately after resolution, losing the acknowledgment. A saved answer with execution still marked waiting_human could also produce a second generic Needs your input warning.

- Questions now show the actual question, its supplied context, choices and reply controls directly. Drafts survive filtering and retain the existing session-scoped recovery behavior.
- Command cards show the requesting agent and related work when available, the exact command, working directory, shell, known isolation limits and one-attempt scope before the decision controls. Internal IDs remain in secondary details. Missing proposal data still prevents approval.
- The same approval requests are rendered in Needs you, WorkDetails and PlanExecution, scoped to the work or that plan's recorded root/assignments. A decision continues through the existing resolveApproval API and refresh path.
- Confirmed answers and decisions leave receipts in the mounted view. An approval receipt does not claim command execution. An answer receipt says it is waiting for an update until recorded task state shows running or another outcome.
- Shared projections distinguish a waiting task whose questions are all answered from a task that still needs input. Map/work labels, agent context and inbox counts use this distinction; an independent permission or unanswered question remains actionable.
- Keyboard focus moves from a submitted control to its receipt only if the user has not moved away. Requests keep their display order as they resolve.

## Integration and authority

Decision and HumanQuestion remain the request components. ApprovalRequests adds shared scoping and temporary presentation receipts; it is not an approval service or execution store. Existing LocalEngineProvider polling remains authoritative. The waitingAfterAnswer helper interprets recorded task/question state; it does not modify state or restart work.

No Rust runtime, inference, grants, orchestration, persistence schema or API changes. The map layout, navigation and brand styling are preserved. No permanent fixture route or alternate workspace is introduced.

## Validation

- Full web suite: 31 files, 210 tests passed.
- Production build and TypeScript check passed. Vite still reports the large output chunk warning (about 601 kB uncompressed).
- Focused tests cover work/plan approval scope, exact approval identity/digest, mismatched receipts, expiry while open, stale decision reads, removal of externally resolved requests, draft preservation across filters, duplicate answer suppression, retained receipts and keyboard focus. Existing handoff tests retain uncertain-response retries and deadline handling.
- Projection tests distinguish saved answers from resumed work and preserve independent pending questions/permissions.
- Browser review used the real TeamWorkspace and LocalEngineProvider with an isolated in-memory client. Verified direct work approval, disappearance of that permission from Needs you, answer selection/submission, the receipt, and the absence of a duplicate human warning while waiting. Reviewed desktop and narrow layouts at the browser's normal sizes. Test code and its temporary tab were removed afterward.
- Local screenshots: `.lokai/manual-testing/decision-experience-2026-10-08.png` and `.lokai/manual-testing/decision-receipt-2026-10-08.png`. These show labeled fixture data, not live agent execution.

## Limits and remaining work

This does not prove live model or command execution/resumption; no live permission, tool invocation or inference was performed. Approval payloads do not supply a trustworthy human-language purpose or consequence summary, so the UI uses actual work context and exact command data instead of inventing one. Temporary approval receipts survive polling in the current view, not a reload; durable history is unchanged. Capacity-specific waiting explanations require engine evidence. Grouping related blockers (5.3), contextual capability setup (4.4), continuous effort identity and setup-return improvements remain separate work.

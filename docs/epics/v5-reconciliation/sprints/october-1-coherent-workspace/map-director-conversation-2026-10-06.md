# Map conversation, proposal and team execution

Date: October 6, 2026. Source baseline: `95620125`.
This completes a connected local conversation-to-team slice, not the entire October demo or an autonomous director.

## User journey

The map composer defaults to the existing Guide. A short question can end with an answer; a larger goal can become a reviewed team proposal in the same conversation. Selecting a saved worker directly remains available.

Discussion, Brief and Plan are no longer separate shaping tabs. The conversation stays visible, with an editable direction disclosure, a proposal, team progress, contributions, result and follow-up composer in one place. Saved brief and plan history remain available in disclosures. The map, agent portraits, spatial layout, colors and tools/MCP management are preserved.

The normal handoff is now **Prepare a plan → review → Start this plan**. Preparing saves the selected direction and requests a structured proposal. A completed planning reply is validated and captured automatically. Starting records agreement and invokes the existing execution endpoint. Neither reading the screen nor capturing a proposal launches work.

The full conversation is not sent into planning. The editable suggested direction contains the opening goal, latest user guidance and latest Guide answer. The user can inspect or replace it before preparing the plan. Subsequent discussion does not silently change an agreed or running assignment; proposal refinement and the existing upcoming-work controls remain explicit.

## Existing systems reused

| Capability | Implementation |
|---|---|
| Conversation and replies | Existing `LocalWorkspace` Explore work, parent lineage and managed inference |
| Workspace awareness | A bounded projection of the same authorized snapshot used by the UI |
| Shared direction | Existing versioned work brief and scoped information context |
| Planning | Existing Guide, structured schema, persisted generation receipt and readiness checks |
| Agent selection | Existing saved roster, pinned definitions, tool grants and limits |
| Start | Existing agreement receipt and managed plan execution |
| Parallel execution | Existing grouped dispatcher and managed child admission |
| Results, usage and intervention | Existing execution projection, usage ledger and human questions |

There is no replacement scheduler, agent registry, runtime, permission system or budget ledger. The new `prepare` command composes the existing save-brief and generate operations. They share a retry identity; this is not a new atomic database transaction. A generation rejection can leave a saved direction, which remains inspectable and editable.

The engine-managed Guide's selected definition is updated through the existing resource edit service. Publishing a new definition alone did not move an existing installation to the new instructions. User-created agents and previously pinned run definitions are preserved.

## Context and correctness

Each ordinary Guide turn receives a fresh, owner-authorized observation: saved agent capabilities, selected work states, reported usage and configured allowances. It excludes other Explore discussion bodies and does not include work result bodies. Agent descriptions and titles are shortened; detailed lists are capped at 24 entries and the serialized observation at 16,000 bytes. Larger observations fail explicitly rather than silently fabricating an answer.

Follow-ups in a launched plan use recorded conversation lineage to scope work and token totals to that plan. Workspace totals are separately labeled. Worker assignments and coordinator records have distinct roles and explicit completion counts. Configured tools are not presented as evidence of a healthy connection, and token allowances are not described as billing caps.

UI requests retain their identities before dispatch. A lost response can be retried without creating a duplicate brief, planning run or team. Start validates agreement and execution receipts. Stale revisions, outstanding edits, readiness failures and uncertain operations still block unsafe transitions.

## Verification

- Engine application library: **228 passed, 3 existing opt-in live tests ignored**.
- Expanded director regression after live testing: passed; covers plan-specific counts, usage, coordinator role, unrelated-work exclusion and private-discussion exclusion.
- Web suite: **155 passed across 26 files**; focused post-polish tests: **16 passed**.
- Production web build and CLI build passed. Vite still reports its existing large-chunk advisory.
- Package engineering gate passed, including formatting, Clippy, architecture and static checks.
- New UI cases cover map-to-Guide routing, inline preparation/capture/start, uncertain prepare recovery, uncertain start recovery and refusing dispatch after a mismatched agreement receipt.
- New engine preparation test covers persistence across reopen, replay without another inference call, changed-payload rejection and exclusion of private conversation text from planning.

## Real local trial

Through the actual UI on port 5177, the installed `qwen3.5:latest` model proposed two independent reviews of a supplied workshop scenario. The saved Local assistant and Reviewer were selected; both worker requests were observed running concurrently. Both completed and the existing coordinator produced a combined result.

| Record | Reported tokens | Allowance |
|---|---:|---:|
| Local assistant: session format comparison | 1,099 | 1,500 |
| Reviewer: scheduling risk assessment | 1,118 | 1,500 |
| Coordination and synthesis | 2,552 | 3,000 |
| Plan execution total | **4,769** | **6,000** |

Planning and discussion usage is separate. Source conversation: `c2109e1f-c8ff-4aac-9050-dc47cdbece5d`; root execution: `e33b77d2-cf1c-4a8e-8c92-d2da7a5b65d3`. Compact local evidence is in `.lokai/manual-testing/director-live-evidence.json`. Completed work survived a service restart.

Live follow-up testing exposed workspace totals being described as this plan's totals. The context projection now scopes through the conversation root and tests that boundary. A second answer correctly reported 4,769 tokens but counted the coordinator as a worker; explicit role and completed-worker fields were added to avoid that ambiguity. The canonical UI already showed two contributions separately from coordination.

The final live follow-up, after rebuilding and restarting, correctly reported **two completed worker assignments, zero active work and 4,769 tokens for this plan**. The response kept workspace usage separate. Historical test replies remain recorded; the fix does not rewrite conversation history or guarantee that a model cannot misinterpret future state.

This trial uses real inference and managed parallel execution, with no external research or tool writes. It does not prove calendar access, a live MCP connection, arbitrary task success, fresh installation or active-run recovery.

## Remaining boundaries

- Awareness is a per-turn snapshot, not a model-callable inspection tool or continuous subscription in the Guide's context. It cannot answer questions requiring unseen result contents.
- Preparing and starting a plan still require the inline controls. Natural-language permission does not yet trigger autonomous proposal editing or dispatch.
- The Guide's model configuration follows the existing local service setup; no new frontier-provider selector or harness integration was introduced here.
- This does not implement installation packaging, empty-state onboarding, workspace organization, new skills/MCP grants or general active-run recovery.

The live service uses the existing database and workspace. No user agent, tool grant or earlier work record was removed.

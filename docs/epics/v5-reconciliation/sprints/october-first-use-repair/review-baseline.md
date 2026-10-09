# First-use review baseline

Date: October 8, 2026. Source revision: 113a6a3e.
Evidence: the owner's pasted walkthrough and read-only inspection of that session's authenticated workspace and proposal, plus the relevant source. No live requests were replayed during the review.

The review used a new database, fresh browser origin, no copied provider credentials, no work history, no saved teams, no skills/MCP connections and a separately granted empty working folder. The current first-run bootstrap exposed Local assistant and The Guide. This was the current product, not an intentionally seeded demo.

## Observed journey

| Observation | Confirmed boundary | Sprint response |
|---|---|---|
| Two unexplained starter agents and dense details made the starting action unclear | These are actual bootstrap defaults, not leftover sample agents | EXP-001/005 |
| An orientation question led to filesystem calls and timeout | Local assistant request a61fc25b-4329-4592-b570-3fe7c064505a failed at the time limit; tool exploration was reported in the owner's trace | EXP-001/002/007 |
| Planning activity changing the card to running/completed was helpful | Existing activity feedback has value and should be preserved | EXP-006/007 |
| Proposal asked the owner to fix coordination tokens before a goal was clear | Source 3fe40cc1-7c8e-487b-a08e-21cd6438df93 has draft proposal generation 898aa512-6597-470a-8912-3bd519a70fef: four dependent assignments to Local assistant, 2,000 tokens each, plan total 8,000; zero coordination headroom | EXP-001/004 |
| Model selection and verbose usage crowded the proposal | Work details mix decisions, routine setup, technical constraints and conversation | EXP-005 |
| Discuss-with-Guide inserted long text into a cramped composer | LiveShaping uses two initial rows; its composer CSS limits height to 120px | EXP-005 |
| Selected saved team seemed to disappear into Personal | Two saved rosters exist; selection primarily changes composer recipient while map project context remains separate | EXP-006 |
| Frontier setup seemed to require a website and manually finding an ID | Account discovery exists; all three provider key_saved flags were false. The empty-state model UI exposes external catalogs and manual entry | EXP-003 |
| Agent steps/time could not exceed 8/120 | Current host defaults and advertised catalog ceilings are 8 steps with a folder and 120 seconds. The engine supports configurable larger ceilings; this session did not select them | EXP-002 |
| Repository-location request produced another timeout | Tommy request 4a27e24d-cbc7-479b-b023-5d47b8cc5cdc failed. Only the empty review folder was granted; the user's repositories were outside it | EXP-002/007 |
| Unwanted work could not be cleared | The inspected local UI/client exposes no archive/delete work operation | EXP-008 |
| Agent/team creation was understandable; team orchestration was not | Preserve those forms' useful structure; make their handoff to work visible | EXP-006/009 |
| Status color was too subtle; user would abandon the product | Qualitative owner observation, not a measured accessibility or conversion study | EXP-007/009 |
| Skills/MCP store would be useful | Explicitly described as nice to have; retain existing library, defer storefront | Outside required sprint |

## Interpretation and limits

The user did click Prepare a proposal. The failure is offering/presenting an inappropriate plan before understanding the request, not evidence that the engine dispatched without consent. That proposal never started. Its four single-worker assignments do not demonstrate multi-agent collaboration.

Both direct attempts used qwen3.5:latest and the default 120-second limit. Model behavior, local performance and restrictive defaults can contribute to failure; their relative contribution has not been measured. Do not claim a stronger model alone fixes the experience, or that every inference adapter failed.

The isolated folder was selected when preparing the review. That explains why the repository request could not be fulfilled with current access; it does not excuse failing to explain scope or request the relevant folder. Do not solve the problem by granting the entire machine automatically.

No account-aware discovery was exercised with a saved frontier credential in this session. The owner accurately reports that the presented setup path was confusing; that is separate from whether the underlying discovery endpoint works.

The owner liked observable running/completed transitions and found creation forms relatively straightforward. Preserve those strengths. One owner walkthrough is strong evidence of these failures, not a statistically representative market test.

## Baseline for comparison

Repeat the same intents and additional unseen coding/noncoding requests. Record useful outcomes, unnecessary plans/tool calls, navigation detours, missing-context explanations, time to the first useful response, human repair work and actual resource usage. Keep model, hardware and permission differences visible when comparing trials.

The review's work titles, plan content and agent names are real user/model output. Do not replace them with polished samples and call the product improved.

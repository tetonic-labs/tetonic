# One current proposal in the Guide conversation

October 9, 2026. Product repair under EXP-001/004/005. This supersedes the earlier
requirement to hide a pre-execution proposal behind a compact review card. It does
not close the real-model or fresh-owner acceptance gates.

The owner reported losing their place between discussion, review, errors, manual
editing, duplicate plan copies and starting work. The interface now presents one
current proposal inline with the discussion. It contains the outcome, participating
agents, assignments, setup issues, consent and an explicit start action. Requesting
changes focuses the existing composer without hiding the current proposal. Manual
editing remains a secondary option; saving replaces the current proposal's contents.

Earlier versions are closed read-only history, excluding the current revision.
They are retained in the existing engine history, not deleted. The start control
identifies its exact revision. An unconfirmed start retains its original request
and clearly identifies that older revision if a newer proposal has appeared.
No approval, scope, budget, agreement or idempotency checks are removed.

Starting work changes the inline proposal into the existing execution summary,
with result/issue inspection and the map action. It does not navigate away from
the conversation. Changes to a proposal alone no longer force the conversation
scroll to the bottom. A new Guide turn refreshes the proposal even when the reply
finishes too quickly for the UI to observe a running state.

The existing engine brief/plan and execution APIs remain the only authorities.
This adds no plan store, deletion endpoint, planner or automatic launch behavior.

Behavioral coverage includes conversational revision in the full workspace,
same-composer issue resolution and draft retention, manual revision replacing the
visible proposal, read-only previous versions, exact-revision dispatch, uncertain
start replay, hosted consent and existing execution/map navigation. Live-model
quality and the owner's assessment of the revised experience remain separate.

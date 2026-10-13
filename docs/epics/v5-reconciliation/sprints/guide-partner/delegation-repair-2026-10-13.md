# Guide delegation repair

October 13, 2026. Follow-up to the [Guide sprint qualification](evidence-2026-10-13.md).

## Report and causes

In an existing conversation, the owner requested a text-based battle-bot game,
then accepted making the files without executable playtesting. The Guide instead
returned Python in chat. It did not save or run those files.

Two problems contributed: the Guide instructions did not clearly distinguish a
requested implementation from a copyable example, and proposal mutations were
always anchored to the original conversation root. That root already owned a
started library-event plan, which correctly rejected replacement. A conversation
was effectively consumed by its first dispatched effort.

## Changes

- Guide instructions preserve implementation intent through scope reductions,
  delegate feasible work through the existing `work_plan` capability, and distinguish
  missing optional validation from missing access required to do the work.
- The nearest proposal-owning ancestor is the current effort. An unstarted plan
  is revised in place. After start, the next proposal can attach to the actual new
  conversation turn, leaving the previous plan and execution receipt intact.
- The Guide's history and subject scope retain the original conversation identity.
  The UI reads engine-supplied proposal ownership and displays the latest proposal
  in the same conversation, including links to its individual turns.
- Available resource observations can be used directly instead of calling the same
  inventory tool again. Proposal inspection links to prior results without dumping
  the old effort's full record into the new turn.
- Worker allocation guidance accounts for repeated input, tool results, review
  and the final completion call, as well as generated output.

There is no new scheduler, intent keyword router, automatic permission expansion,
or chat-to-file fallback. Models still choose when and what to propose. Saving a
proposal is not launch authority; the existing review/start boundary remains.

## Verification

- Engine regression: started effort remains unchanged, new effort stays unstarted,
  subsequent corrections revise the new plan, exact retries are idempotent, the
  selected roster is preserved, and ownership survives database reopen.
- Web regressions: conversation-root and turn links display the current proposal
  without splitting the chat or submitting/starting work.
- App library suite: 219 passed, 5 ignored. Web suite: 323 Vitest tests and 13 Node
  boundary tests passed. Production build passed with its existing chunk-size warning.
  The package engineering gate passed (formatting, Clippy, architecture, static quality).
  After the final allocation wording and UI focus fix, all 25 Director tests,
  20 shaping UI tests, production build, formatting and the package gate passed again.

## Live qualification and limits

The owner's actual conversation and artifacts were copied to separate local test
profiles; worker folders were redirected to empty temporary directories only in
those copies. The owner profile was not reset or seeded with replay turns. Tests
used the saved OpenAI connection and `gpt-6.1-sol`, the general harness, and the
agents' existing `list_dir`, `read_file`, `write_file` grants. No shell was added.

Replaying “thats fine, just make the game” with the full history produced a real
file-creation assignment via `work_plan propose` and a concise review/start reply.
It did not paste a replacement game script or mutate the earlier library plan.

Execution testing exposed two additional limits: one hosted stream interrupted
after staging both files; another worker reached its 10,000-token allocation after
staging them. Both remained failed rather than reporting staged output as delivered.
The worker-budget schema now explicitly describes the cumulative tool-loop cost.

A natural-language follow-up described the allowance failure and requested a
smaller complete attempt within saved limits. The Guide itself proposed the new
scope and allocation: 16,000 worker tokens plus 12,000 for coordination. No plan or
game code was hand-authored by the test driver. Starting through the UI completed
both the worker and coordinator, reporting 12,682 tokens out of 28,000. The browser
showed “Result ready” and one of one contributions ready. Independent filesystem
inspection confirmed committed `battle_bots.py` (3,326 bytes) and `README.md`
(995 bytes); Python's AST parser accepted the game source without executing it.

Local evidence is retained under `.lokai/guide-delegation-84743635/`:
`retry-executed-plan.json`, `file-proof.json` and `completed-game.png`. The generated
files remain in `C:/Users/Mark Miller/AppData/Local/Temp/tetonic-game-proof-2kabo8_8`.
These temporary files, copied private state and connection credentials are not
committed. The successful effort ID is `c0855e8e-7e31-425c-8f32-3caaad930bd8`.

The original replay's underallocation and the hosted transport interruption are
retained as failures; the successful bounded retry does not relabel those runs.
The owner's engine on port 18741 is updated with the repair and retains its
original conversations, agent definitions, folders and completed work.

This is a bounded behavioral qualification, not proof that every model will always
delegate correctly. Instructions guide model judgment; the engine enforces proposal
identity, authority, grants and execution state. Executable game validation and
agent-versus-agent playtesting require an appropriately granted execution capability
and are outside this file-only test.

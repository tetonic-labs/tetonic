# Guide qualification and remaining coordination gap

Part 1 (Guide to useful team result) is **not yet accepted end to end**. This
change closes the durable structural-repair gap and reduces conversation
overhead. Real-provider tests confirmed parallel workers, but exposed a remaining
coordination budgeting problem. Do not count the isolated tests as successful
final-result journeys.

## Implemented

- Invalid proposal arguments reach the existing host-bound Guide control instead
  of being rejected before durable accounting. Structural, roster and allocation
  failures share the existing two-attempt receipt in SQLite, including after
  interleaved successful reads and database reopen. No schema migration or new
  planner was introduced.
- Repairs preserve direction, total allowance and promised work. They may repair
  representation, assignment IDs, dependency references and allocations. A quoted
  plan is decoded only to compare intent; invalid content is never published.
  Ambiguous original intent fails closed. Oversized rejected arguments retain a
  digest rather than another full payload.
- The Guide tool advertises a nullable object directly and pins available agent
  keys. Repeated product instructions were reduced. Context reports the existing
  agent-editor folder setup and each agent's actual scope.
- Worker input puts its agreed assignment before the shared brief and explains
  that the brief is background, not a request to perform every teammate's work.
  Coordination guidance accounts for dispatch, input context, reasoning and
  synthesis, without silently increasing an approved allowance.

## Actual-model evidence

All trials used isolated databases and synthetic workshop details. The hosted
trials reused only the owner's authorized OS-vault reference; the key itself was
not exported, logged or committed. The account's live model catalog was checked.
No live conversations or files were copied. Trial engines were stopped.

| Trial | Observed result |
| --- | --- |
| `journey-acceptance-06a4c12d`, local `qwen3.5:latest` | Discussion completed in 53.4 seconds without work. A proposal was eventually saved, but the Guide reply did not finish within the 180-second qualification window. The trial canceled the reply; no worker execution was qualified. This binary preceded the final early-boundary repair fix. |
| `hosted-acceptance-624bfee3`, OpenAI `o3-mini` | Discussion 5.1 seconds, proposal 16.1 seconds. Model created two independent assignments and a dependent synthesis task. The harness initially rejected the third assignment; corrected assertions accepted this legitimate graph and resumed its saved proposal. Explicit review/start and duplicate-start request launched both independent workers concurrently. All three assignments completed; the coordinator's 1,000-token allocation failed before final output. |
| `hosted-acceptance-89bf6895`, OpenAI `o3-mini` | Discussion completed; proposal left 6,500 tokens for a coordinator capped at 4,096. Repair changed instructions and summary as well as allocations. The existing scope-preservation rule rejected it; the reply exhausted its allowance. No work dispatched. |
| `hosted-acceptance-d40d42f7`, OpenAI `gpt-5.4-mini` | Discussion 2.1 seconds, proposal 4.1 seconds. Two independent workers ran concurrently and completed with distinct contributions. The coordinator used 1,972 input + 50 output tokens against a 2,000-token own share; the budget stopped further spend before the dependent synthesis assignment. No holds or unknown calls remained. |

Evidence files are private ignored `.lokai/<trial>/` artifacts. The model generated
the briefs, plans, dependencies and contributions. No hand-authored successful plan
was substituted. Setup qualification separately passed with a real hosted file
tool; see `working-setup-2026-10-09.md`.

## Remaining acceptance work — coordination cost

The engine accounts for reported input and output tokens. Planning still allocates
as if short visible output implied cheap coordination. Prompt guidance alone did
not correct this reliably. Increasing a provider's output cap would not fix a
coordinator with an insufficient approved own share.

1. Make orchestration overhead part of proposal feasibility using the actual
   coordinator context and expected dispatch/synthesis exchanges. Distinguish
   allowances from exact cost predictions; do not claim a provider-side hard cap.
2. Keep budget-only correction from requiring the model to reproduce every text
   field. Prefer a scoped allocation patch over weakening intent preservation.
3. Reduce unnecessary coordinator exchanges using the existing reviewed dependency
   graph and controller. Do not create another scheduling authority or duplicate
   worker effects, and preserve stop, dependency, budget and human-wait controls.
4. Repeat the unchanged workshop journey to a useful final result, with no manual
   budget repair. Then qualify concurrent independent efforts and recovery.

The 256-token structural minimum and 4,096-token default coordination ceiling are
not an adequate proof of feasibility for arbitrary model-authored plans. Part 1
remains open until the real journey passes; automated orchestration tests are not
a substitute for this outcome.

## Regression evidence

Application, memory and CLI suites: **623 passed, 5 existing opt-in tests ignored**.
Frontend: **13 Node checks and 301 Vitest tests passed**. Production build passed
with the existing large-chunk advisory. Package verification passed formatting,
workspace Clippy with warnings denied, architecture and quality-static gates.
The final small settings style correction was followed by focused UI checks and
another production build. There is no claim of new release qualification.

# Guide to a useful team result — October 11

**Outcome:** the bounded Guide → reviewed proposal → parallel workers → combined
result journey passed with OpenAI `gpt-6.1-sol`. No person rewrote the plan,
reallocated its budget or messaged workers separately. This is evidence for the
first readiness item, not closure of the whole first-use sprint or MVP. Model
reliability remains a qualification constraint; failed trials are retained below.

## Implementation

- Estimate coordination overhead from the same instructions, shared brief, plan
  and tool advertisements used at execution. Check it before saving and starting.
  The estimate covers two exchanges, concise contributions and output headroom;
  verbose results, extra exchanges and provider tokenization can cost more. Actual
  reported usage still belongs to the existing managed budget ledger. No owner
  allowance or configured ceiling is increased automatically.
- Add `work_plan`'s allocation-only `rebalance` operation. It reads the first
  parsed proposal from the existing durable receipt and changes only assignment
  allowances. Direction, total, membership, dependencies, tools and deliverables
  remain intact. Existing scope, revision, two-attempt and idempotency checks
  still authorize publication; resource reads cannot reset the correction limit.
- Advertise full input/output cost directly beside the model's budget fields.
  Inject the host's coordination ceiling into the Guide's proposal schema.
- Use the existing controller to dispatch the reviewed dependency graph together.
  A grouped response contains each worker contribution once. Persisted receipts
  still precede advancement of the coordinator's completion guard. Single-key
  dispatch, cancellation, human waits and capacity controls remain supported.

These changes reuse WorkService, ResourceService, the managed run ledger and the
existing SQLite proposal receipt. There is no new scheduler, storage schema,
permission authority, provider adapter or UI redesign. Repair activity uses the
existing Guide activity presentation.

## Real-model qualification

Each trial used a fresh isolated database and synthetic workshop constraints.
Only the owner's authorized OS credential reference was reused. No live work,
private conversation or credential value was copied into the trial. Account model
discovery preceded inference. Failed trials were not converted into successes.

| Trial | Result |
| --- | --- |
| `hosted-acceptance-21c0ff00` | Saved OpenAI connection rejected at model discovery. No inference started. The owner refreshed the connection after the local browser was reconnected. |
| `hosted-acceptance-3cfc5db7`, `gpt-5.4-mini` | Proposed a 3,200-token total below the estimated coordination minimum. Allocation correction remained invalid; the Guide exhausted its own reply allowance. No workers launched. This preceded the budget-field clarification. |
| `hosted-acceptance-b4c5d977`, `gpt-5.4-mini` | Proposed a 3,600-token total with only 400 for coordination. Correction left 600 rather than the required 3,584. The Guide ended with a blocker; it incorrectly presented its self-chosen 3,600 as the user's constraint despite the user's 10,000 ceiling. No workers launched. |
| `hosted-acceptance-5c1354c3`, `o3-mini` | Invented an unavailable agent key. Its correction changed roster and promised work instead of preserving intent; rejected. The reply exhausted its allowance. No workers launched. |
| `hosted-acceptance-dd95207e`, `gpt-6.1-sol` | **Passed.** Brainstorming 4.0 s; model-authored proposal 15.1 s; reviewed team execution and synthesis 23.12 s. No proposal repair or manual changes. |

The successful trial followed the same request as the failures: compare one
two-hour workshop with two one-hour sessions, evaluate Tuesday versus Saturday
using supplied attendance constraints, and return a recommendation with one open
question in fewer than 250 words. It used two saved agents and a selected team.

Observed assertions and results:

1. Initial brainstorming returned a conversational answer and saved no plan.
2. The Guide read resources and saved one proposal using the selected two agents.
   Neither worker started before explicit review/start through the normal API.
3. Repeating Start returned the same accepted execution receipt.
4. Both independent workers were observed running together, from 2.50 s until
   their completion was observed at 14.61 s. Two assignments produced two results.
5. The coordinator completed at 23.09 s. Its **206-word memo** recommended Tuesday
   conditionally, distinguished weekday employment from proven evening absence,
   acknowledged missing attendance information, asked one unresolved question and
   linked both actual contributions. It did not invent research or external actions.
6. The plan total was 10,000 tokens: 2,952 for each worker and 4,096 for coordination.
   Reported execution usage was **5,020 tokens**: coordinator 2,673; workers 1,152
   and 1,195. No over-limit flag, held allowance or unknown/pending call remained.
   Brainstorming used 2,203 tokens and proposal shaping used 9,980 separately;
   those conversation costs are not included in the 5,020 execution figure.

Private evidence lives in ignored `.lokai/<trial>/`: proposal, start/retry
receipts, execution samples, final result and usage snapshots. The API harness
does not author the plan or worker answers. This run qualifies the engine journey;
it is not a claim that a complete new-user browser walkthrough was repeated.

## Verification and limits

- Application, memory and CLI suites: **627 passed, 5 existing opt-in tests ignored**.
- After the final budget-field clarification: **60 work tests passed, 3 opt-in
  tests ignored**.
- Existing Guide activity, shaping, team-plan and team-engine UI suites:
  **82 tests passed**. No frontend behavior was changed.
- Normal CLI build passed. Final package verification passed formatting, workspace
  Clippy with warnings denied, architecture and quality-static checks.
- Original local workspace retained its 48 records. Its browser connection was
  restored using the fresh connection link, rather than weakening authentication.

One passing frontier-model trial does not establish reliability across providers,
models or arbitrary work. In particular, the two smaller-model configurations
above are **not qualified for reliable autonomous planning** by these results.
The estimator is an early feasibility check, not a provider billing cap or a
guarantee that any model can finish inside a chosen allowance.

The wider EXP-009 browser/owner walkthrough, simultaneous independent efforts,
recovery, recurrence and packaged launch gates remain separate. Tool-backed hosted
agent setup has its own evidence in [working setup](working-setup-2026-10-09.md).
Earlier coordination failures remain recorded in
[the October 10 qualification](guide-qualification-2026-10-10.md).

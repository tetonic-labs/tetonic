# Real-model width trial — October 13, 2026

**Acceptance: failed for the full scenario; narrower file execution passed after an engine fix.** This supports the existing first-use and coordinated-work gates. It does not close them or change the October 25 scope.

The standard is useful completed work across independent efforts, with evidence and understandable human intervention. Starting several agents, generating a plan, or showing a green completed conversation is not sufficient.

## Scenario and conditions

Run through the existing built product UI, served by the local engine on one loopback listener:

1. Ask the Guide to prepare an October launch evidence review for a saved two-agent team.
2. Have Release analyst read recorded product evidence and identify priorities. Have Runtime analyst independently read execution configuration and coordination evidence. Both use their granted `read_file` tool.
3. Run a separate Setup reviewer that reads launch evidence and writes a real Windows launch guide. Require owner approval for its writes.
4. Leave that approval pending while the team progresses. Approve the file, verify it exists, and inspect the team's combined decision brief.

The agents used the saved OpenAI connection and its discovered `gpt-6.1-sol` model. This was not a local-model capacity test. The engine profile and approved working folder were isolated from the owner's normal workspace. Input files were bounded copies of actual repository documents and source, with provenance headers; no successful results were seeded.

Provisioning the agents, folder and permission policy used the authenticated API. Requests, Guide proposal review/editing, dispatch and approvals used the real browser UI. Therefore this is **not** acceptance of first-user agent creation through the UI. The Guide authored the assignments and dependency structure; the operator adjusted the proposed token allocations through the existing plan editor before starting them.

## Observed results

| Requirement | Evidence | Result |
|---|---|---|
| Guide creates a real team proposal | A saved proposal assigned two independent source-reading tasks to the selected agents; no tasks started before review. | Passed |
| Independent team execution | Both analyst work records were running together. Both obtained actual file-tool output. | Passed at width two; not a throughput or load benchmark |
| A human wait does not freeze unrelated work | 51 polling samples showed both analysts running while Setup reviewer had a pending file-write approval, spanning at least **28.115 seconds**. | Passed for separate efforts |
| Complete the combined review | Release analyst exceeded its 8,192-token allowance at 8,263 reported tokens. The team stopped, the Runtime analyst's in-flight call became unconfirmed, and no combined brief was delivered. | Failed |
| Deliver the original Windows guide | First attempt could not read its own staged file. A later attempt made two approved staged revisions but exhausted its allowance before delivery. Neither produced a committed `WINDOWS-FIRST-RUN.md`. | Failed |
| Verify the repaired transaction path | A narrower 77-word launch note was read from source, written after approval, read back through `read_file`, and committed to disk by managed completion. | Passed; does not substitute for the full scenario |
| Report a blocked deliverable truthfully | An intervening attempt was incorrectly classified as read-only because its request said “Read only lines 7–18.” Its write was denied, but the conversation became `completed` and the UI said “Result ready”/“Done.” No file existed. | Failed |

The final launch note is useful as a launch snippet for someone with an already-built executable and UI. It is not an installation guide, a clean-machine test, or proof of a completed team project. Its SHA-256 is `495aec46aa1f5c2984e1227e59360587019fb86e68ea6bc9383dc4ce69ed6eeb`.

## Engine fix delivered

Previously, `write_file` staged a new file in the owning workspace transaction, but `read_file` checked only the live filesystem. A worker trying to verify its output received “does not exist.” Overwrites similarly returned the old live content. This could consume additional inference and prevent completion.

`read_file` now consults the existing transaction service for its own staged contents before falling back to the live file. It does not publish the file, bypass capability checks, create a separate workspace owner, or materialize a complete verification overlay for each read. Staged content is read without following links and checked against its recorded digest. Paths removed in the transaction do not fall back to their old live contents.

The existing path and credential-store restrictions remain before the read. A separate agent's transaction cannot see the pending file. Abort restores the live view; reading pending contents does not rebase a conflicting change by another writer. Line-range reads work on the pending content.

This fix is specifically for `read_file`. It does not claim that directory/search tools or the existing edit implementation provide a complete transaction-aware filesystem view. Repeated staged edits and discovery of pending files need separate coverage before making that broader promise.

Validation on Windows:

- Two new tool regressions reproduced the original missing/stale-read behavior before the fix, then passed.
- `cargo test -p tetonic-tools --lib`: 62 passed, one existing ignored test.
- `cargo test -p tetonic-transaction`: 48 integration tests passed, including three new tests for conflict preservation, deleted/renamed paths and changed staging contents.
- `cargo build --locked -p tetonic-cli`: passed; the bounded live file trial used the rebuilt executable.
- `cargo run --locked -p tetonic-arch-gate -- verify package`: passed formatting, workspace Clippy, architecture and static quality checks.
- The product design and frontend implementation were unchanged.

## Failures that now determine the next work

### 1. Make the budget boundary usable before it destroys momentum

The trial started with a host total of 16,384 tokens. The Guide respected that ceiling and proposed 4,096 per worker plus 8,192 for coordination. The operator raised the isolated host ceiling and edited the plan to 24,576 total: 8,192 per worker and 8,192 for coordination. **Those trial allowances were still too tight for the actual source-reading exchanges.** This is not evidence that parallel execution is impossible or that the frontier model is incapable.

The metered provider currently caps output by remaining allowance, while the allowance is charged for both input and output. It checks the reported total after the call. This is a reported-usage stop, not a strict provider billing cap. Repeated instructions and tool results consume input again on later calls. The planner's preflight estimates coordinator overhead; it does not establish that a worker can afford to read its sources and finish.

The existing budget ledger intentionally closes further inference in the whole allocated tree after a branch overrun (`require_bounded_work_reports`). Do not remove this protection just to obtain a passing demo. Build an actionable budget pause/recovery path, account for input and completion headroom before subsequent calls, and preserve completed contributions and pending output for review. Distinguish a local allocation problem from a shared stop or uncertain provider charge. Budget changes must remain explicit and bounded.

Acceptance: with suitably configured allowances, the original two-agent source-reading plan finishes and combines real contributions. In a separate deliberately constrained case, the operator sees the reason, remaining/held usage and a concrete recovery action, without silently increasing limits, hiding usage, or rerunning already accepted work.

### 2. Fix inferred read-only mode and outcome presentation together

The registered harness's `is_explain_turn` treats the substring `read only` as a whole-turn restriction before checking action words. The coding definition has similar logic. “Create a file; read only lines 7–18 of the source” therefore loses its write capability for that turn despite the saved agent having the tool and a review-required policy.

The narrow successful repeat used `start_line:7` and `end_line:18` instead. That wording is a **test workaround, not a fix** for intent handling. The explicit permissions were not relaxed. Do not require users to discover this wording trick.

A second problem compounds it: the engine accepted an explanatory final answer after the denial and the UI labeled the conversation as a ready result. The agent's answer honestly disclosed the missing file, but the visual state still suggested successful work. Preserve the distinction between an inference run finishing and an intended outcome being delivered. A denied tool call is not always terminal—an agent may recover—but an unresolved required action needs an intelligible blocked state or explicit unmet-result indication.

Acceptance: the original natural-language file request reaches its normal approval path; explicit read-only instructions and policy restrictions still hold. If a required write remains blocked, the map/inspector must not imply that the file was delivered.

### 3. Repeat the original scenario; do not replace it with the smaller pass

Repeat from the saved product flow with adequate, explicit worker allocations, without inserting a plan/result through a test fixture. Require:

- Both source-reading contributions and their coordinator's useful combined brief.
- A separately completed file deliverable.
- One pending human decision while unrelated work progresses.
- Inspection of actual artifacts, grants, usage and final states.
- Clear evidence of what was manually configured, what the Guide decided, and what the agents actually executed.

This is the next acceptance gate. More map concepts, marketplace integrations or experimental agent types do not address these failures.

## Local evidence and limits

The ignored profile `.lokai/mvp-width-d9a7e596/` retains the input manifest, first and final workspace snapshots, plan/task records, approval records, engine log, sampled execution states and `evidence-summary.json`. `.lokai/mvp-width-current.txt` locates the profile. The working-folder location and launched process are in its `session.json`; connection credentials are private and are not part of this document. The original failed attempts remain visible.

Work receipts:

| Work | ID |
|---|---|
| Guide-authored proposal | `5ed8ddec-77b1-4a18-9f67-914af5dab54d` |
| Team execution | `b8c2a3c3-bc5e-4ecc-bc91-45eb8c7d3605` |
| Release analyst | `9e5edca8-cc82-4c31-a26f-a978fcae04ef` |
| Runtime analyst | `7b56ca06-4a31-4b62-8039-593c4412677a` |
| Initial staged-read failure | `1b937f75-0253-4a3a-b827-2f33363e626a` |
| Review-required Windows guide | `63f89c15-c615-46b2-a97c-4422d176194d` |
| Incorrect read-only classification | `6c64c360-35fe-4705-b113-038d31613df1` |
| Successful launch-note file | `98e0ff62-1ce2-4e64-b227-ca9d4308f1cf` |

The runs recorded **50,130 input/output tokens** and **one unconfirmed call with 7,122 tokens of allowance held**. Held allowance is not confirmed actual usage or a billing guarantee. The successful narrow file task used 4,317 reported tokens. No more paid runs were started after this qualification, and all work had reached terminal states when evidence was captured.

No large-scale load, multi-user isolation, external MCP mutation, clean installation, restart recovery of an active team, recurrence or durable human-wait restoration was proved by this trial. Those existing gates remain open.

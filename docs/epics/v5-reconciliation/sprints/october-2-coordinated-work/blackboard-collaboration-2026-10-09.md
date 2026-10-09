# Blackboard collaboration — October 9

Scope: the single-owner local preview's assigned agents can exchange deliberate,
durable messages within one agreed effort. The existing Blackboard inspector
shows these conversations as compact topics and inline reply threads. Execution
records remain a separate view; they are not presented as agent conversations.

## Operator experience

- Select **Blackboard** in an agent's tools at creation or editing. Availability
  in the catalog is not an automatic grant, and this tool needs no working folder.
- Workspace, team and agent permissions can restrict communication to permitted
  agents on the same effort, selected stable agent identities, or nobody. Both
  sides must permit the exchange; a lower scope cannot override a higher ceiling.
  The read-only preset defaults to blocked communication. Explicit communication
  exceptions remain visible separately from file/connection approval settings.
- Open **Blackboard → Conversations** to see findings, questions and handoffs,
  their authors, audiences and reply counts. Expand replies in place. Long text
  is disclosed on demand; rendered message height is measured again when the
  available width changes, including formatted lists and code. Project filtering
  happens on the server before paging.
- Messages lead with the author and timestamp. Emoji pills show actual agent
  reactions and counts; select one to inspect who reacted inline. Long replies
  collapse independently. The map and existing color palette are preserved.
- The inspector is read-only. Agents use the tool; the operator observes it.
  Engine errors are not disguised as an empty conversation history.

## Existing systems used

`LoopDiscipline.host_tools` routes the tool through the existing managed async
host call path. `RegisteredToolHost` advertises it only when granted. The
application's `resources/plan_dispatch/blackboard.rs` binds the host-authorized
principal, workspace, work, attempt, call and current lease. The model supplies
only a command and its message fields; it cannot choose its author or authority.

`tetonic-memory/control/blackboard.rs` owns durable topics and idempotent write
receipts (schema 74). One transaction checks the live work/run/attempt, current
lease and stop state, pinned agent revision, current tool grant, pinned work
roster, and current workspace/team/agent communication policies before writing.
The existing registered-agent revision reader now has an internal transaction-
owned form, avoiding nested transactions without moving permission checks out
of the message commit.

An audience is fixed at topic creation. Membership follows stable agent identity
within the effort; attribution retains the originating assignment. A subsequent
assignment must still pass its own current policy checks. Grant/policy revocation
fences reads, writes and idempotent retries. Other efforts and other owners are
not reachable. Owner inspection retains already-recorded history after revocation.

The existing local authenticated API exposes `POST /api/local/blackboard/query`
as a read-only inspection query with optional thread/work scope and an offset.
Unknown authority fields are rejected. Authorization and work scope are applied
before the 20-topic page limit. A topic list returns the initial message and reply
count; opening a topic returns its messages. Polls do not overlap and are aborted
when the inspector, thread or scope changes.

## Nonblocking behavior and limits

The tool supports `peers`, `read`, `post`, `reply`, `react`, and author-owned `resolve`.
Messages never create a task dependency, suspend an attempt, dispatch another
agent, wake an inactive agent or interrupt a running one. Agents are instructed
to check at useful work boundaries and continue independent work. Existing work
dependencies and human handoffs retain their separate meaning.

Messages are limited to 4,000 UTF-8 bytes; titles to 140 bytes; recipient lists to
12 identities; a thread to 32 messages; writes to 32 per assignment. Exact retries
reuse their receipt without consuming another contribution. These limits bound
chatter; they do not prove that an arbitrary model will always use its time well.
Normal run budgets, deadlines and step ceilings still apply.

`react` requires a thread, a message ID, a supported emoji (👍 ❤️ 👀 🎉 💡 🙏 🤔 ✅),
and explicit `present: true` to add or `present: false` to remove. Author identity
is taken from the active execution, never the model's arguments. Each identity
has at most one reaction per emoji per message, even across different calls or
assignments. Removing a reaction cannot remove another agent's reaction. Existing
messages deserialize with an empty reaction list; schema 74 needs no additional
migration. Reactions share the message permission checks, write quota, and durable
retry receipts. They may acknowledge a resolved thread but do not approve actions,
resolve work, create dependencies, interrupt agents, or move a topic above newer
conversations. Only agents with current execution authority can mutate them.

Private chat history is not copied into the board. Only deliberate messages are
shared. This is not a claim that a model can never disclose information already
present in its authorized context; broader information-flow enforcement remains
a separate concern.

## Verification

Automated integration scenarios use real local workspace admission, the managed
team runtime, persisted agents/rosters/policies, and a scripted inference endpoint.
They exercise:

- Two independent agents exchanging a question and reply; the recipient's model
  response is held until the sender has posted and proceeded to finish.
- A stable agent reading and replying from a later assignment in the same effort.
- Durable history after reopening the database; private-context canary isolation.
- Audience checks, author-only resolution, invalid reply parents, exact retries,
  selected-peer restrictions, workspace/team denial, tool revocation, stale leases,
  stop fencing, pagination and bounded contributions.

UI tests exercise concise topics, inline replies, long-message disclosure,
unavailable endpoints, canceled reads, scoped pagination, and the authenticated
query contract. Provider catalog tests distinguish availability from grants.

Initial collaboration slice automated results:

| Check | Result |
|---|---|
| `tetonic-app` library suite | 189 passed; 4 environment/subprocess entry tests ignored |
| Local UI API tests | 9 passed |
| Capability policy tests | 3 passed |
| Agent revision storage tests | 2 passed |
| Runtime checkpoint tests | 5 passed |
| Blackboard, permissions, team workspace, agent capabilities/creation UI tests | 57 passed |
| TypeScript check and Vite production build (including architecture boundaries) | Passed |
| `git diff --check` | Passed |

### Follow-up: Slack-style messages and reactions

The follow-up reran the three Blackboard managed-runtime integration scenarios
with actual `react` tool calls before replies. They passed, including persisted
reaction attribution after reopening storage. Added assertions cover duplicate
reactions across calls, explicit removal without removing another author's
reaction, changed-intent retry rejection, invalid message IDs, stale leases,
foreign principals, acknowledgements on resolved topics, and revocation on retries.

The legacy-message/command-validation unit test passed. Blackboard and team-work
UI tests passed (42 tests), including rendered-height collapse, resizing, inline
reaction attribution, and updated root reactions when opening a thread. TypeScript,
the Vite production build, and `git diff --check` also passed. The existing Vite
large-chunk advisory remains; this follow-up does not change the bundling strategy.

The engine was not restarted and the running workspace was not migrated during
this slice. The new backend becomes available after starting the updated engine.

Real-model usefulness and live browser/manual validation are separate from these
scripted checks. This slice does not complete native vendor harnesses, arbitrary
cross-team messaging, durable team parking/resume, or scheduled autonomous teams.

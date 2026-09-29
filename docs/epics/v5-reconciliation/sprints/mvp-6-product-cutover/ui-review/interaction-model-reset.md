# Tetonic: reset the interaction model

September 28, 2026 · Design proposal following the user's correction to the work experience audit. No application implementation is included.

**Further development:** [Shared rooms design](shared-rooms-design.md) extends this proposal to mixed human-and-agent rooms, personally owned agents brought into shared conversations, direct requests, and coordinator-selected teams. It compares two initial layouts and records the remaining design questions.

## What changes in the diagnosis

The earlier audit found real discontinuities, but its recommendation still started too close to a formal work item. That biases the experience toward briefing, staffing, supervision, and completion. It makes an ordinary question feel like the beginning of an organizational process.

The user's two promises are the better starting point:

1. Keep someone oriented when they have many conversations and efforts underway.
2. Let that person take on large problems with agents and teams, without becoming the scheduler for every small step.

These promises must also hold when there is no project yet. Understanding a problem, testing a possibility, asking a quick question, or deciding not to proceed are legitimate successful outcomes. A conversation does not need a completion workflow to justify its existence.

## The main distinction: who carries the next step?

**Thinking together:** the person is actively exploring, asking, comparing, or making. One agent or several teams may contribute. They have not necessarily been given an independent assignment.

**Delegated work:** an agent or team has an understood responsibility to move something forward and return a useful result, or ask when a real boundary is reached. The person can leave without the effort losing its owner.

Number of contributors and degree of autonomy are independent. A five-agent discussion can remain collaborative exploration. A single agent can own a substantial independent investigation. Avoid turning either distinction into a required mode selection.

The interface must make a transfer of responsibility legible. Ordinary discussion must remain easy. Interpret a direct request in context; do not demand a special button after every clearly authorized instruction. When intent is ambiguous, ask a short question where the ambiguity actually matters. Suggestions such as “maybe we could contact customers” must not silently become outreach.

## A simple beginning

The first visit offers one obvious action: say what is on your mind. No required team, project, work type, success criteria, schedule, or organizational setup. A reasonable default collaborator is available immediately. Existing permissions and capabilities determine what it can do.

A quick question receives an answer. An uncertain idea receives useful exploration. A clear request can go directly to execution within authority. Nobody must spend time ideating before delegating something they already understand.

Optional mentions or an invitation bring a specialist or team into the conversation. They are shortcuts, not prerequisites. Users should not need to learn every agent's name or capability to get appropriate help. Team contributions should offer a coherent synthesis, with disagreements and source contributions available to inspect; a wall of near-duplicate replies recreates the swarm in text.

Buzz's public site communicates a comprehensible entry point through conversation with people and agents. The useful lesson is a clear first action, not a requirement to reproduce a team-chat application. Source: https://buzz.xyz/ (public positioning reviewed September 28, 2026; this is not an evaluation of its full product).

## The conversation can grow without changing address

Example: “Would a customer workshop be worthwhile?” starts a discussion. The research team can weigh in while the person is still considering it. Later, “Compare two formats and bring me a recommendation tomorrow; don't contact customers” delegates a bounded investigation.

That handoff leaves a small, understandable record in context: who is responsible, what they will bring back, and any meaningful boundary or return time. Derive this from the conversation; ask only for information needed to proceed. Do not fabricate deadlines or constraints, and do not present inferred staffing as verified capability.

The user can continue exploring while the investigation runs. Research returns beside the originating question. From there the person can discuss the findings, request a revision, delegate the next step, or simply stop. Not every useful answer requires a formal accept button.

A broad assignment can develop subproblems and parallel teams. The user should see a compact account of how those efforts support the outcome, and expand a branch when they need to direct it. Direct manipulation of a document, plan, comparison, or map should remain possible. Conversation is the common entry and context, not a requirement to express every action in text or read an endless transcript.

## Returning is half of the product

Putting fifteen conversations in a sidebar does not solve the fifteen-tabs problem. The return experience must restore intent and orientation:

- What were we trying to understand or accomplish?
- What materially changed since I last looked?
- What useful result or current version is available?
- Is someone carrying the next step, or does it need me?

Use a short, specific continuation line for each ongoing context. “You were choosing between a workshop and an open Q&A” is more useful than “Last updated 4 hours ago.” “Research returned with two options” is more useful than “12 new messages.” Keep machine activity out of the foreground unless it explains a material change.

Bring true human requests and returned results forward while keeping ongoing contexts spatially predictable. Do not constantly reorder the entire list with every event. At higher volume, support user-owned domains, grouping, search, and quiet archival; do not require creating those structures on day one. Do not silently merge different contexts or share their data because they look semantically related.

A request for judgment should recover the necessary context locally: what changed, why the agent needs the person, the recommendation and tradeoff, and the smallest useful action. Evidence is close by. Summaries must identify uncertainty and distinguish observed events from inference. The user can inspect the underlying messages and outputs when the summary is insufficient.

## The role of the map

The map explains how efforts, agents, teams, and dependencies relate. Entering it from a conversation should preserve that conversation's scope and selection. The wider organization remains available for people managing many parallel responsibilities.

The map is particularly useful for “where is this blocked?”, “what depends on this?”, and “which team is handling that branch?” It should not be a required step to ask for help or a canvas on which every user must manually wire execution. It is one view of the same underlying efforts, not a separate universe of agent chat.

## Recurring and event-driven responsibilities

“Every morning, check support and draft replies; ask before sending” can emerge through the same interaction. Confirm the consequential trigger, scope, and authority compactly. Detailed setup belongs where it matters. A standing responsibility retains individual runs, outcomes, exceptions, and a clear pause action; it should not pour every routine run into the main conversation list.

This keeps the experience general-purpose. Personal planning, teaching, product delivery, and incident triage can start with the same simple act while exposing different useful artifacts and controls as their needs diverge.

## What to prototype before another production redesign

Test a thin end-to-end experience, not a new dashboard:

1. A first-time user asks a small question and gets value without setup.
2. They explore an uncertain problem with a team without accidentally assigning it.
3. They hand off one investigation, keep thinking, and later recover the result in context.
4. They return to fifteen mixed contexts: some personal exploration, some delegated, some ongoing responsibilities. They identify what needs them without reading every transcript.
5. They redirect one branch of a complex effort and can tell what changed and what remains active.

Measure first useful action, unnecessary decisions before value, ability to identify who owns the next step, context recovery time, and unintended or misunderstood delegation. Ask participants to explain what they believe will happen after they leave. That is a stronger test than whether the screen looks calm.

The inline concept illustrates selected moments with a scripted workshop example. It is a hypothesis about interaction, not a connected agent system or a validated final design. It does not yet demonstrate fifteen-context navigation, editable artifacts, live orchestration, or permission enforcement.

## Priority

First establish the connection between conversation, continuing context, and responsibility. Then test resumption across many concurrent efforts. Only after those work should we refine the information hierarchy of detailed work views and the map. Fixes in the earlier audit remain relevant when those surfaces exist; their existing framework should not dictate this redesign.

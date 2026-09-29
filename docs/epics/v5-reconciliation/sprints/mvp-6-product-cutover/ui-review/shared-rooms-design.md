# Shared rooms: first interaction design

September 28, 2026 · Design exploration · Builds on the interaction model reset.

**Complete workspace extension:** [Map investigation design](map-investigation-design.md) reconnects these room studies with the Map, Work & decisions, People & agents, and Tools & MCPs, including a team inspection conversation grounded in recorded activity.

## Revised composition: a working space

The initial layouts overused the visual grammar of administration: persistent sidebar navigation, boxed work items, status labels, and a reserved management column. Their interactions were worth exploring, but their composition made people and their contributions feel secondary to the interface.

The next two studies remove the sidebar, use a compact conversation switcher, and place the human and agent cast directly beneath the shared topic. Typography, spacing, and fine rules carry the hierarchy. Agent contributions and ongoing investigations remain visible as attributed text and expandable references. Invitations and agent selection open only when requested.

**Working transcript** keeps the social exchange primary. Authorship sits in the margin, contributions share an unboxed reading surface, and an investigation appears as a short branch from the conversation. This is the stronger continuation of the group-chat direction because the next action stays familiar while the presentation feels more specific to Tetonic.

**Common page** places the shared question and current understanding at the center. Human contributions sit beside it, and conversation continues beneath. It tests whether a stable account of the problem makes return visits easier. Its risk is authorship: a production version must make changes to the shared understanding explicit, retain disagreement, and distinguish a proposal from a group decision. The study uses a fixed sample and does not claim to solve collaborative document editing.

The stylistic change is deliberately limited to useful structure: no floating desktop windows, decorative graph motion, or hidden controls. A larger topic title and quieter navigation provide character while keeping the composer, collaborators, available agents, and delegated work discoverable. Both studies retain direct requests and explicit coordinated investigation. They are local design prototypes, not changes to the application.

## The product experience

A room is a shared place for people and agents to understand something, make progress together, and delegate independent efforts. It can contain one person and their agents, several people bringing agents of their own, or a larger cross-team collaboration. The number of participants does not determine how formal the work must become.

The interface should make three things apparent without opening settings: who is here, how to contribute, and whether someone is carrying work forward. Available capacity and actual participation are separate. A workspace with many agents should communicate that capability without suggesting every message reaches every agent.

The accompanying interactive concept compares two arrangements using the same customer-experience scenario. It uses simulated responses and local state only. Application code has not changed.

## Layouts under consideration

**Room first — preferred initial direction.** The room contains a compact participant strip, human conversation, one composer, and a short entry for each delegated effort. Opening an effort changes the main content to its scoped detail; returning restores the conversation. This preserves reading width and gives new users fewer simultaneous surfaces to interpret. Its main risk is that important work can disappear into chat history. A production version needs a stable, compact route to all active efforts, even when the originating message is far above the fold.

**Work alongside.** The conversation has a narrow neighboring column containing active efforts and available teams. This makes ongoing work and organizational capacity easier to discover, at the cost of a narrower conversation and more information at rest. On narrow screens, the additional column moves below the conversation. That responsive behavior is adequate for comparing the concept; a production mobile design should offer direct access to work without requiring a long scroll.

Both arrangements preserve Tetonic's restrained surfaces, fine dividers, earthy accent, and readable typography. The differences are about attention and navigation, not decoration.

## A room's visible cast

The room header separates the people from the agents using labels and simple avatar shapes. It shows a small representative cast rather than an unbounded roster. The full participant list identifies an agent's role and who brought it. Ownership should never be inferred from an avatar color.

The workspace and agent picker show shared teams and their capacity. In this scenario Research has four agents, Product six, and Engineering twelve. Those are fictional examples, not live availability metrics. The room initially contains three people and three agents; the available pool is larger than the current conversation.

Every participant can bring an agent they are allowed to share. The invitation communicates the context being shared and the access boundary in plain language. Inviting a personal agent does not publish its other conversations or grant others unrestricted access to its tools. Production needs an enforceable agent-sharing model; the prototype only illustrates the disclosure.

## Dispatch: clear intent, flexible routing

| User action | Expected meaning |
| --- | --- |
| Send an ordinary group message | Add to the human conversation; do not automatically launch work. |
| Address a named agent | Ask that agent to participate or act within its existing authority. |
| Ask the coordinator | Let it select suitable available agents and coordinate their contributions. |
| Use “Ask Atlas to investigate” on a message | Prepare a contextual request in the composer; the person can edit and send it. |
| Invite an agent | Add a collaborator to the room; do not imply that it has already been assigned work. |

“Ask Atlas to investigate” is a concrete example of a contextual affordance, not a permanent prescribed action on every message. Production should support mentioning the coordinator directly and natural follow-up in an already active exchange. Users should not need to know an agent's exact name to get help.

The coordinator chooses the smallest useful group, makes the division of effort legible, and expands only when useful and permitted. Automated routing does not grant permission to perform external actions. A short acknowledgment identifies the owner, expected return, and material boundaries. It should state uncertainty when suitability or availability is unknown.

Whether a coordinator can proactively participate is a room-level agreement to establish later. The first design uses addressed participation because it is easier to understand in a mixed human group. We should test interruption tolerance before adding proactive behavior.

## From discussion to delegated effort

In the example, Sam and Jo discuss onboarding. A direct question to Nia produces a contribution to the discussion. Asking Atlas to investigate starts two connected analyses: Nia reviews customer feedback; Tess examines the funnel. A compact work entry makes the effort discoverable without exposing every tool call.

Opening the investigation reveals its purpose, contributors, status, original instruction, and human decision owner. Pause and resume affect that effort, while the room remains available for discussion. These controls should eventually show acknowledgment and in-flight effects rather than claiming an instantaneous runtime stop.

When findings return, Atlas presents a recommendation and the choice required. Discussion can return to the room with the investigation explicitly referenced. Choosing a direction records the decision. Starting implementation is a separate instruction in this example, since the initial authorization was analysis only.

Routine agent messages belong inside the effort's activity history. The room receives meaningful findings, a real decision request, or a substantive change. The user can inspect contributors, disagreements, evidence, and progress; orchestration must not become invisible behind a confident summary.

## Human collaboration and responsibility

Room membership, agent ownership, work ownership, and decision authority are different relationships. A participant may contribute without being responsible for the final choice. Each delegated effort needs a legible decision owner, or an established shared decision rule. That avoids notifying everyone and getting no response.

The initial concept uses “requested by you” and “you own the decision.” Future prototypes must cover a colleague delegating work, another person joining midstream, conflicting instructions, reassignment, and the original requester being absent. Human handoffs deserve the same continuity as agent handoffs.

## Returning and scaling

Workspace rows describe where the group left off or what now needs a person. Their position should stay predictable as routine activity occurs. A returned result and a request for judgment need distinct language; neither should be represented as an undifferentiated unread-message count.

A room may contain many independent efforts. The next scale prototype should include fifteen mixed contexts and multiple simultaneous efforts within one room. Test grouping, material-change summaries, and stable navigation before adding more permanent columns. The orchestration map will be a scoped view of these same efforts and relationships; this prototype intentionally does not redesign it.

## What this first prototype covers

- Sending ordinary local messages without dispatch.
- Asking a named agent for a perspective.
- Preparing and sending a coordinated sample investigation.
- Inviting a personal agent with visible ownership and context scope.
- Opening a work thread, pausing, resuming, and returning to the room.
- Previewing findings, discussing a decision in context, and recording a direction.
- Switching between conversation-centered and work-alongside layouts.
- Returning to a small workspace overview.

Only the two sample prompts generate canned agent responses. Other messages are displayed locally. The transcript is intentionally bounded for the design preview and does not represent production history retention. The preview has no live agents, backend dispatch, real invitations, account permissions, evidence fetching, or durable multi-user synchronization.

## Next design tests

1. Can a new participant identify the people, agents, and wider available teams without instruction?
2. Can they predict which messages will request agent involvement?
3. Can they explain what inviting an agent shares and what it does not share?
4. Can they tell who owns the next step after a direct question, a coordinated handoff, and a decision?
5. After leaving and returning, can they find the relevant outcome without reconstructing the transcript?
6. Does a dedicated work column improve orientation enough to justify its visual cost?

Before application implementation, extend the strongest layout with starting a new room, real evidence inspection, several concurrent efforts, and a human-to-human handoff. These tests will decide the interaction structure; the current workroom model should not dictate it.

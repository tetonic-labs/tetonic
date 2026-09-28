# A place for general-purpose work

September 28, 2026. Interactive frontend design prototype.

The primary experience now starts with work, while the organization map remains available. This tests whether a person can keep track of several contexts and intervene with sufficient information, without continuously watching individual agents.

## Interaction decisions

- A shared work model supports assignments, ongoing responsibilities, and event responses. The examples span operations, products, business, teaching, and personal planning. They are illustrative scenarios, not a live organization.
- The overview separates prepared decisions/results from routine work. Context filtering keeps domains apart; a persistent global attention entry still exposes other work decisions, approvals, and playback exceptions. Rows keep their order until the person explicitly changes the work.
- Starting work requires only an idea. A small task force and an initial exploration agreement are prefilled. Additional context is conversational input; detailed agreement fields are optional and are not rendered until requested. The team can be changed before handing off. The prototype does not infer capability suitability or availability.
- The default first step is a proposed approach, open questions, and a useful first step for review. The default boundaries permit research, organization, and drafting, while reserving external actions and connected-system changes for the person.
- A thinking space holds alternatives and questions separately from instructions. Added context is preserved with the brief. This separation is a prototype to test, not a claim that people will naturally understand it without research.
- A decision brief contains the question, why human input is needed, known facts, uncertainty, accessible evidence, options with consequences, and what continues if the person waits. An investigation request leaves the original decision open.
- Recording direction, acknowledgment, and a reported result are separate states. Example follow-through is loaded explicitly rather than appearing as if agents executed the instruction. Added conditions are retained but are not evaluated by scripted examples. A reported result is still ready for review, not automatically verified.
- Work/Map navigation retains the open work, decision draft, context filter, map state, and playback. Team members link to the existing inspector. Shared agents keep one identity.

## Scope and limitations

No model or engine is connected. Conversation input records context but does not generate agent replies. New work, brainstorms, directions, and loop controls modify this tab only. Reloading clears them. The map's motion examples are separate from the illustrative work scenarios; they do not represent execution of these assignments.

Continuous monitoring, actual triggers, durable execution, permissions, capability-aware staffing, competing commitments, consequence estimation, and fresh evidence need backend contracts. The overview does not prioritize across domains by inferred urgency or business impact. It preserves sample order and provides scope controls.

## Validation

Automated tests cover scoped decisions, duplicate submission, separate acknowledgment/result states, loop dispatch requirements, custom work without simulated execution, decision/context preservation, idea-to-handoff without required form filling, exploratory notes, requests for more evidence, and preview pause/resume. The full suite has 55 tests.

Browser walkthroughs exercised the 80-agent workspace, a one-sentence handoff with additional context, a scripted incident decision, map return, and a decision/evidence flow at 390 × 844. The production build passed. These checks verify implementation; they do not establish reduced cognitive burden with users.

The next research question is whether someone can identify what needs judgment, explain the tradeoff, and resume their work after switching domains. A connected prototype should then test real brainstorming and whether a team can ask a useful follow-up question instead of demanding a complete brief up front.

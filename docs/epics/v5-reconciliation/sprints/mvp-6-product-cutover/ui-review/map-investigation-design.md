# Map investigation and the complete workspace

September 28, 2026 · Interactive design prototype · Application code unchanged.

## The map belongs in the primary experience

The shared-room studies were too isolated to explain Tetonic as a product. The integrated workspace now exposes five persistent destinations: Conversations, Map, Work & decisions, People & agents, and Tools & MCPs. Navigation stays compact and uses the same typography and fine dividers as the working-transcript direction.

The prototype opens on the map. Its twenty-two agents, three teams, three shared resources, two efforts, and eight recorded activity events form one consistent sample. Team inventory, work entries, resource usage, and conversational inspection refer to that same data. This is a schematic interaction study; it does not replace the application's existing map layout, playback, collision handling, or live event model.

## Hover, select, ask

Hovering a team highlights its boundary and connected routes. Keyboard focus provides the same preview. Moving away removes the transient emphasis without changing any ongoing investigation. “Ask about this team” is also visible at rest and reachable by keyboard or touch; hover is an enhancement, not the only access path.

Selecting the team pins the investigation context. Asking opens an inspector with that team's current activity and a timestamped source snapshot. Hovering another team previews that other team on the map without silently retargeting the chat. Clicking a new team's ask action explicitly changes the inspection scope.

The map remains available while the inspector is open. On smaller screens, the inspector follows the map rather than compressing the nodes or making labels unreadable. The example reflows for screen width. In application integration, preserve the current map camera and team positions when opening inspection; drawer layout must not trigger a graph layout recalculation or fit-to-view.

## Investigation is distinct from intervention

“Ask about Research” is a question to the inspection layer using recorded activity, not a message instructing Research. The inspector explicitly says that the question is not sent to the team. Ordinary questions about progress, waiting, and ownership should not interrupt agents, change their priorities, or create additional delegated work.

Supported sample prompts cover what the team is doing, what is blocking it, what changed recently, and who owns the next step. Free text with those intents uses the corresponding recorded summary. Unsupported questions receive an honest prototype limitation rather than an invented answer. Production will need a retrieval-backed model scoped to authorized activity and artifacts; this local prototype makes no model calls.

There should be a deliberate transition from inspection into direction. The prototype offers “Open their conversation,” preserving the team's relevant room. A production version can additionally offer an explicit “Ask the team” or “Give direction” action, with the audience and authority visible. It must not reinterpret observational questions as instructions.

## Ground the explanation in the map

Answers include evidence links to recorded actions. Activating one highlights the relevant team, agent, resource, and connected route, and displays the exact event text with its timestamp. A cross-team handoff highlights both teams. An all-activity view retains access to every event in the sample and provides the same locate action.

The map offers an overview of ongoing and recent activity; the activity record preserves the individual actions. Not every event needs an equally prominent animation. Aggregation must remain inspectable, with honest counts and the ability to find the source event. The user's purpose is to understand the work, not to catch every motion live.

Distinguish an observed wait from a blocker requiring human action. In this sample, Engineering is waiting on a compatibility check, but the log contains no request for human intervention. The answer states both facts rather than turning a waiting status into an urgent approval. A production answer should distinguish observed events, inference, unknown information, and stale data.

## Connected navigation

- A conversation opens its relevant team on the map, keeping the room's topic identifiable.
- An investigation opens the team's conversation without broadcasting to unrelated teams.
- Work entries link both to the responsible team on the map and to the originating room.
- A team in People & agents can be inspected directly.
- A tool or MCP opens its recorded usage and the team using it.
- “Return to map overview” clears the inspection focus without requiring a new room or work item.

The sample preserves local messages when moving from a room to its map context and back. It is not a durable messaging implementation. The prototype has a bounded sample transcript and no real dispatch, invitations, permissions, backend data, pan/zoom camera, or autonomous agents.

## Application integration requirements

Keep the existing map and introduce the investigation as a layer over it. Reuse its team identifiers, visible membership, activity stream, selection state, and camera rather than building a second graph. A team with shared agents should highlight the existing agent identity once, while revealing its relevant memberships.

### Preserve activity and agent identity

The new shell is not a replacement graph renderer. `TeamActivityMap` already provides work playback, traveling work markers, tool interactions, recent trails, waiting states, selection, and pan/zoom. Retain those behaviors when adding conversation and team investigation. The schematic team containers in this study are not a specification to flatten the application graph into static cards. Keep existing home positions and local collision handling; opening an inspector must not restart the simulation or reset playback.

At organization scale, stable agent portraits provide landmarks while work markers communicate activity. Team hover should make activity easier to trace, not stop it or hide every unrelated event. A pinned investigation keeps its scope even while the activity stream continues. Preserve explicit waits, failures, and human requests with distinguishable states; do not infer a request for intervention merely from a wait.

Agent images belong inside the square map nodes, center cropped with a modest corner radius. Keep names available on focus/hover and in inspection, and put status indicators outside the image rather than tinting the portrait. Reuse the existing per-agent image identity from `agentImages.ts` and the validation/cropping flow in `AgentImagePicker`; add a square presentation for map nodes without forcing every circular `Portrait` elsewhere to change. Shared memberships must reference the same agent and image, not create duplicate agents.

The updated interaction study includes a finite replay of the eight recorded sample events, with play/pause/resume and replay. This is compressed historical activity, not a live connection. Team answers and activity dots remain explicitly labeled as the 10:24 snapshot. Reduced motion retains discrete activity emphasis without moving work markers. A local image picker illustrates square cropping; preview images stay in memory and clear on reload, while the application already has browser-local persistence. The study does not reproduce the application's full physics or camera.

Inspection needs an explicit scope object: team identifier, selected effort when known, source time window, snapshot revision, and evidence identifiers. The visible chat scope must not follow transient hover. Store independent investigation history by scope so returning to another team does not mix evidence or lose the earlier discussion.

Use the graph's actual hit areas and a reachable pointer path to its ask action. Make keyboard focus and tap equivalent. Closing inspection restores the previous working context; it should not reset camera position, playback time, or the user's selected effort. Incoming activity should not reorder the conversation or quietly rewrite an old answer. Offer a clear refresh when the underlying snapshot changes.

## What to validate next

Test hover-to-ask across small and large teams; keyboard-only and touch access; a pinned team while previewing another; multi-team agents; cross-team handoffs; long team names; a fast event stream; stale snapshots; and a room-to-map-to-room round trip. Verify that people can explain the difference between asking about a team and asking that team to act.

The key success criterion is that someone can locate an unclear part of the map, ask a plain-language question, inspect the evidence, and decide whether any intervention is necessary without reconstructing the entire event stream.

September 28 prototype check: verified finite activity playback, pause/resume, pinned team questions during playback, square node geometry, and no horizontal overflow at 390px and 320px browser widths. The embedded preview's native file chooser did not return during browser automation, so applying a chosen portrait remains unverified in that preview. No application graph, physics, image storage, or playback source was changed.

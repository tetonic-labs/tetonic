# Team conversations in the app

Implemented September 28, 2026 in `web/`.

## Interaction

Selecting a team name on the map focuses that landmark and opens its shared room. Scrolling the map remains spatial; zoom never silently switches to chat. Back restores the prior camera and selection. Team activity opens the team's scoped map. Both the map and the organization clock stay mounted throughout. Team-directory names open conversations; their map arrows retain direct map access.

The room shows people and agents together, with square custom agent images and an expandable roster. Ordinary messages are local conversation, with no automatic dispatch. A message can become a reply thread without a work-creation form. Drafts, replies, and room reading positions survive navigation within the current session. Map quick messages and room messages use the same underlying event list; private agent messages stay separate.

Existing work has a contextual thread containing the request, checkpoints, evidence, the pending decision, and replies. Decisions update the same work reducer as the Work dashboard. Recording one does not invent an execution acknowledgment or overwrite the earlier illustrative checkpoint. Agent profiles remain reachable from contributors, but are not required to follow the work.

Map activity is grouped by recorded workflow identity. Each workflow has the same thread and replies in every room with participating contributors, including agents with multiple memberships. Calls without a workflow ID remain separate; timing alone does not imply causality. Routine operations are summarized, current and recent work is readable directly, and the detailed event record opens on demand. Blocked workflows receive priority in the compact overview. A completed operation is not presented as a completed goal.

## Space and color revision

The header is a compact 54 px row. Room names use ordinary navigation typography. There is no centered reading-width cap: the transcript uses the available room width. Opening a thread uses that same full-width surface; returning restores the conversation position.

The shared app treatment uses a dark navigation frame, a compact mineral-toned room header, open conversation space, dark input surfaces, and copper decision checkpoints. Records are not individually enclosed in outlined cards. Agents retain square portraits and original image colors. Working, waiting, blocked, and judgment states use consistent shared tokens, accompanied by names and shapes. Foreground room interaction remains separate from the mounted map; details and draft state survive return navigation. See `app-visual-language.md` for the styling applied to the other views.

The large map composer is collapsed into a quiet conversation entry. Quick messages remain available on demand.

## Verification

- All 76 tests across 14 files pass; production build and TypeScript pass.
- Integration coverage includes distinct drafts, thread replies, message-to-thread branching, cross-surface message consistency, decisions, earlier-checkpoint retention, team switching, restored camera and selection, keyboard focus, and continued sample playback.
- Workflow projection tests cover shared agents, shared workflow identity, independent simultaneous operations, unresolved failures, and completed retries.
- Browser checked at 1280 × 720 and 390 × 844, in light and dark themes. Rooms occupy the viewport width below navigation, with no horizontal page overflow.
- Visually exercised the 80-agent network and a running organization-wide delivery trace. A reply to a shared workstream stayed visible after following its Research-room link. Checked people and image handling in the original workspace through integration tests.
- Screenshots: `screenshots/team-room-wide.png`, `screenshots/team-room-mobile.png`.

## Limits

This is the existing frontend preview, not a connected engine or multi-user service. Messages and decisions survive navigation within the tab but not reload. Historical work is explicitly illustrative. There is no autonomous response, actual dispatch, server-side persistence, invitation, access-control change, or live collaboration.

Cross-team association comes from recorded agent and target membership. The fixtures do not express a complete authorization or causal dependency model. Detailed event rendering is progressively paged; there is no server history pagination or transcript virtualization yet. Thread navigation and custom room state are not URL-addressable. The earlier Work dashboard and agent inspector remain available; this change establishes the team conversation path rather than replacing every existing work surface.

The production bundle still emits the existing size advisory (now about 531 kB minified / 162 kB gzip). No separate performance benchmark was run for the room projection.

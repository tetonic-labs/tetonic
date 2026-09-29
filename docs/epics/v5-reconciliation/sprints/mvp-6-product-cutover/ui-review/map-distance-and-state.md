# One map at different distances

September 28, 2026 · Design exploration · Application source unchanged.

## Recommendation

Use semantic zoom: the information changes with distance, while the underlying organization, agent identities, activity clock, selection, and inspection scope stay consistent. The broad map and the original local orbital view serve different questions. They should be representations of the same graph, not disconnected modes with independent simulations.

| Distance | Question | Representation |
| --- | --- | --- |
| Organization | Where is work happening, and where am I needed? | Stable team landmarks, major dependencies, working counts, explicit human requests and blockers. Aggregate routine traffic. |
| Teams | Who is involved, and what is each team doing? | Square agent portraits, readable names on inspection, team resources, state markers, grouped routes. Agents remain useful identity landmarks. |
| Local activity | What is this agent or group actually doing? | Original docking and orbital movement, individual tools, task labels, handoffs, evidence, and asking about the work. Limit animated scope to a manageable work group. |

A large team may still be too dense at the closest level. Move into a work group or selected workflow, with the rest of the membership reachable. Do not animate forty agents simply because their team is selected. Keep cross-team collaborators visible under their original identity and identify remote resources as connections to the wider map.

## Navigation and continuity

- Clicking a team focuses it; an explicit zoom action or further exploration reveals local activity. Hover only previews; it never moves the camera.
- Smoothly move the camera toward the selected object and progressively reveal detail. Maintain positions and team geography. Avoid replacing an overview with an unrelated arrangement of cards.
- Use available pixel space and density to decide what to reveal, not just a universal zoom percentage. Use separate entry and exit thresholds so labels and representations do not flicker at a boundary.
- Keep simulation and activity time separate from camera and representation state. Zooming, selecting, and opening chat must not restart replay or physics.
- Breadcrumbs and a one-action organization overview restore orientation. Back should return to the previous camera and scope. No automatic zoom or camera chasing when events arrive.
- While focused, retain a quiet, actionable cue for relevant human requests and blockers outside the current scope. Never automatically jump to them.
- Reduced motion preserves the same information and activity state without animated travel.

The new `tetonic-map-distances` study demonstrates three discrete levels and finite local movement. It is not the production camera, continuous pinch/wheel zoom, original physics engine, or a live event stream. Production transitions should preserve spatial continuity more completely than this schematic drill-down.

## Color is state, not decoration

| Signal | Meaning | Other cues |
| --- | --- | --- |
| Muted teal | Work currently progressing | Working label, round activity marker, restrained motion at appropriate scale. |
| Amber | An explicit request for human input | Needs you label, diamond marker, decision count and inspectable request. |
| Brick red | Work cannot proceed or an unresolved failure matters | Blocked/failed label, square marker, specific cause and affected work. |
| Neutral | Idle or ordinary waiting | Explicit idle/waiting label; hollow marker for waiting. |

Color must never be the only cue. A completed task can receive a brief success confirmation; a green agent is not proof that its work is correct or verified. Stale and unknown state must be identified explicitly rather than interpreted as idle or healthy.

Team identity comes from position, names, portraits, and membership. The brand accent and neutral selection outline should not acquire competing status meanings. Keep state markers outside portraits and keep colors stable across the map, conversation, work, and inspection views.

Do not color an entire team red because one task is blocked. Show both routine activity and exception counts: for example, five working, one needs you, one blocked. Derive these from actual distinct work/request identities and make them inspectable. Define deduplication where a single incident affects several agents. Requests and blockers must survive aggregation; a high volume of routine work must not average them away.

## Integration and evaluation

Reuse the existing graph, event stream, portrait store, physics, playback, and team investigation. Rendering fewer details at a distance must not discard events or create duplicate agents. Retain access to the underlying activity record. Semantic zoom controls presentation; it does not dispatch work.

Validate moving between levels while activity is running, returning to a previous camera, shared agents, unusually large teams, simultaneous requests and failures, collapsed exceptions, color-vision differences, narrow viewports, reduced motion, and stale data. In a usability check, ask someone to find a request, identify the affected work, inspect its context, and return to the overview without losing their place. Measure successful interpretation rather than rewarding more animation.

References: [Microsoft's semantic zoom overview](https://learn.microsoft.com/en-us/windows/apps/develop/ui/controls/semantic-zoom) describes changing representations with scale. [W3C guidance on use of color](https://www.w3.org/WAI/WCAG22/Understanding/use-of-color) requires information to remain available through cues other than color.

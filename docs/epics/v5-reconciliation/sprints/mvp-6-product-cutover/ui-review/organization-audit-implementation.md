# Organization audit: immediate improvements

September 28, 2026. Frontend preview changes; no engine integration.

This pass implements the audit's two immediate priorities: truthful, connected activity state and continuity while investigating. It also improves overview exception visibility and narrow-screen inspection. The existing map, visual style, and navigation remain the foundation.

## Changes

- One organization-wide sample timeline supplies the map, agent profile, Activity view, resource inspector, and attention view. Team filtering no longer rebuilds or restarts that timeline. Newly created preview agents are not assigned fabricated sample activity.
- The existing attention entry includes approval requests and recorded unresolved work. The view separates decisions from work to review; waits with unknown causes do not claim that a human decision is required. Approval history remains available. Empty states describe their coverage rather than claiming organizational health.
- Failures remain unresolved through unrelated successful calls. An explicit `retryOf` link marks recovery and only a successful linked retry resolves the original failure. Cancelled or failed retries leave the original issue open, without creating duplicate issues for that retry. Multiple independent failures remain inspectable.
- Agent details and Activity show the current sample status and matching event evidence instead of an unrelated Ready badge or empty stream. Focus cards show recent outcomes as well as starts. Resource details distinguish current playback evidence from configured snapshots.
- Team changes retain sample time and the followed agent. Each scope remembers its camera. Shared agents retain one home and identity; team membership labels distinguish contributors, shared contributions, and home counts.
- Overview bundles prioritize unresolved work and waits ahead of routine volume. Team review counts occupy a separate, protected line. Selected unresolved resource relationships remain highlighted beyond the short recent-work trail.
- On narrow screens the focus card becomes a bottom sheet. The composer is temporarily hidden while the sheet is open, retaining its drafts. Teams and the Agents directory provide navigation without requiring tiny portrait targets.
- The composer names its team or All teams explicitly and distinguishes a message saved in the preview from an accepted or executed instruction. Playback controls explicitly affect the shared sample only.
- Initial framing waits for measured viewport dimensions. Small team views include their resources; larger team scopes start at team landmarks while resources retain their stable organization positions.

## Validation

Automated coverage includes independent failures, unrelated successful work, linked retries, cancellation during recovery, matching failure evidence across attention and Activity, and preservation of time, selection, home positions, and camera across team navigation.

Browser walkthroughs covered the original workspace, the 18-agent Studio workspace, and the 80-agent Network workspace at desktop size. The Network walkthrough exercised the rush failure, attention-to-evidence navigation, Research's shared memberships, and a 390 × 844 mobile focus sheet. These are representative interaction checks, not full-length trace runs or a usability study.

## Remaining work and limits

- This is still an in-memory fixture preview. It does not supply durable outcomes, initiative ownership, dependency impact, live commands, acknowledgments, reconnect recovery, or a changed-since-review briefing.
- A recorded failure is work to inspect, not proof of business impact or a required human decision. Recovery ownership and wait causes are unknown where the sample supplies no evidence.
- The preview models one current interaction per agent, and recovery correlation is within that agent. Cross-agent recovery and concurrent-operation contracts need durable engine identities.
- The overview still limits visible route bundles to six. Exception routes take priority; the attention view contains all recorded unresolved items. The map does not establish which issue has the highest business consequence.
- Team inspection preserves organization positions, so some shared resources can remain outside a large team's initial frame. Pan or zoom to explore them; small teams include resources in their opening frame.
- Playback continues while inspecting details unless explicitly paused. A stable review snapshot with an explicit Review latest action remains a follow-up.

The next product slice should connect one outcome, one blocked dependency, one decision, one scoped intervention, and one verified result using coherent scenario data.

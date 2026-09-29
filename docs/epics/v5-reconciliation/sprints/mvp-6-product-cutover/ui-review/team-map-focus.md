# Team focus on the organization map

September 29, 2026

Clicking a team's square or name now frames that team on the existing camera. Other teams fade with camera distance. Scrolling out reverses the transition and restores the organization and its shared resource areas. The conversation is a separate action; entering and leaving it preserves the focused camera.

The close view keeps an assigned-resource shelf in the viewport, to the right on desktop and below the team on narrow screens. Its contents come from the editable Tools & MCPs library, including empty assignments and explicitly marked setup drafts. Recorded activity counts are scoped to the focused team. Long lists scroll inside the shelf. Inspect and Manage retain the map underneath them.

## Spatial behavior

- No home positions, organization clock, or physics world are reset by the lens.
- The camera reserves space for the shelf and stores the team context with its view history. Returning through previous views restores the corresponding team and toolkit.
- Exploring from a filtered team widens the underlying map to the organization without interrupting the incoming zoom. Zooming out can therefore recover all teams.
- Neighboring team controls become inert when substantially faded. The square is keyboard accessible; zoom controls, Home, and previous-view navigation remain available.
- At close range, the existing bounded local-motion solver remains active. Remote travel is projected into a 20-world-unit radius of the agent's home cell, so tools represented in the shelf or distant teams cannot pull an agent off-screen. In-team motion remains physical. This is a presentation change, not a simulated execution or a change to attachment state.
- The overview inquiry affordance is a corner icon, avoiding a hover-induced layout shift that previously intercepted clicks on the square.

## Verification

Automated coverage includes resource assignment changes, empty and draft resources, wheel fade in both directions, preserving home positions and playback time, starting from a filtered team, returning through two different team views, keyboard entry, independent conversation access, reduced motion, desktop/mobile camera clearance, and busy remote work remaining visible. Existing camera gesture and workspace tests remain in the suite.

Manually checked the 80-agent / 10-team network in the browser at 1280 × 720 and 390 × 844, including square clicks, progressive wheel zoom-out, all-hands playback, assigned-resource scrolling, and the compact mobile shelf.

## Limits

Resources are a viewport shelf in the team view; they do not occupy new physical docking locations. Distant work uses contained agent movement and resource status rather than displaying its full travel across the organization. The existing six-agent local-motion limit remains. Large assignment sets require scrolling, and shared agents retain one home identity instead of being duplicated into each team. Resource setup remains the existing tab-local preview, not a live MCP connection.

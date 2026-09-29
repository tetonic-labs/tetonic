# App visual language

The landing page's copper fields, dark ink, contrasting typography, and layered geometry now inform the actual app. This pass covers Work, the map, team rooms, Teams, Agents, agent details, attention, work decisions, Settings, and resource/evidence surfaces.

## Visual decisions

- A compact dark navigation frame gives every view the same anchor. Mobile retains theme and Settings access.
- The Work entry uses a copper plane and dark composer. Serif contrast is confined to short identity text; reading and controls remain sans serif. Work records have open spacing and stronger titles. Copper edges and explicit labels distinguish judgment requests.
- Teams and Agents use wider two-column directories on desktop and one column on narrow screens. Team membership occupies a shallow raised surface; agent identities remain open rows. Portraits, including custom images, are square throughout.
- Dialogs and inspectors have a visible lower or side edge and restrained elevation. Their controls remain simple, slightly softened rectangles. Chat and evidence do not inherit the same enclosing treatment as dialogs.
- The map's team boundaries use a more structured silhouette. Teams occupy aligned rows, with the initial column count chosen for the viewport and team sizes. Geography stays stable after that: adding members expands a team downward and only moves teams below it in the same column when clearance is needed. Shared resources have labeled areas below the teams; those areas yield downward when teams grow. Local collision and activity physics remain intact. Long team names can wrap.
- Map gestures prevent native text selection and image dragging. A drag can begin on a team label or portrait; ordinary clicks still activate them. Starting another gesture clears any map text selection.
- Team rooms have a compact left navigation column with a clear current-room marker. Switching teams preserves separate drafts and the original map return location. Threads use the same conversation region. Below 701px the column is hidden and the existing Teams navigation remains available.
- Shared light/dark tokens govern content, inputs, and status. Working is green, waiting is neutral, human input is copper, and unresolved failure is red. Labels and markers still carry the meaning without color.
- New motion is limited to brief surface arrival and tiny hover displacement. Reduced-motion styles suppress those transforms. Existing live activity and semantic zoom remain intact.

`web/src/brand.css` owns the shared appearance and loads after feature layout styles. Room and canvas palette duplicates were removed. No new external fonts, dependencies, or media were added.

## Verification

- Production build and TypeScript; interaction suite: 79 tests across 14 files. Added coverage for adaptive team arrangement, growth clearance and stable positions, successive drags over controls/text, and room-sidebar draft isolation.
- Browser review at desktop 1280 × 720 and mobile 390 × 844, including light/dark Work, directories, agent details, Settings, decisions, and team threads.
- Checked the 80-agent network, sample playback, closer team map, quick-message input, and work-to-team navigation. Checked mobile document overflow and decision selection.
- Corrected mobile work-row wrapping, textarea overflow, oversized Settings labels, and copper context-selector contrast during review.

The underlying frontend is still a local preview. No backend, work-dispatch, physics, or permission behavior was changed. Team geography is chosen at initial load and deliberately does not rearrange on later viewport resize; use fit/pan/zoom to explore it. The existing JavaScript bundle-size advisory remains; this is not a performance benchmark or a complete accessibility audit.

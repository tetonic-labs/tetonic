# Tools and MCPs workspace

Resources have a workspace home rather than existing only as small map destinations. The Tools entry opens a searchable library, separating MCP connections from runtime tools and storage. Team filtering includes an Unassigned option, so unused resources are still discoverable. A single shared connection can serve multiple teams without duplicating its identity.

The picker offers preview setup templates and a custom remote MCP option. MCP setup asks for a recognizable name and server URL; runtime tools do not ask for a URL. Existing MCPs support another connection, for distinct servers or accounts. Setup drafts can be edited or removed, and team availability can be adjusted. Duplicate endpoints and URLs containing credentials, query parameters, or unsupported protocols are rejected. HTTPS and local HTTP endpoints are accepted as configuration, not contacted.

Team rooms have a Team tools entry that opens the library filtered to that team. Resource inspectors can open their library details; library details link back to recorded activity. These surfaces preserve room drafts, map position, and the organization timeline.

On the map, shared MCPs and tools occupy labeled areas below the teams. Slots stay stable across connection changes. Team growth can move the resource areas down to maintain clearance; local docking and repulsion remain unchanged. The overview emphasizes area names and counts, with individual resources becoming inspectable as users move closer. Resources without recorded topology, including setup drafts, remain in the library rather than implying live activity on the map.

## Boundaries

This is a functioning local UI prototype, not a live installer. The engine is not connected. The picker is not a verified MCP registry. No packages are downloaded, no authentication occurs, no credentials are collected, and no live permissions change. Setup and team availability are tab-local and clear on reload, consistently with the rest of this preview. Remote MCP setup is supported; stdio/package installation, connection testing, engine persistence, and actual access enforcement remain backend work.

## Verification

Interaction tests cover library discovery, unassigned/shared resources, URL validation, draft creation/edit/removal, team assignment, runtime-tool setup, map continuity, room-scoped entry, and recorded-activity navigation. Layout tests cover resource areas, unchanged seats as connections change, team growth, and collision clearance. Browser review covers the 80-agent/24-resource workspace, desktop and phone layouts, custom setup, library/map navigation, and light/dark appearance.

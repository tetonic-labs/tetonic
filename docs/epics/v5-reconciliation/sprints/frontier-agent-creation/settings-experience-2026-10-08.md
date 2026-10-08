# Settings and ability setup: experience refinement

The agent editor, workspace library, and settings panel now share a quieter hierarchy within the existing map and inspector. This slice changes presentation and navigation, not engine capabilities or authorization.

## User-facing changes

- Agent setup follows identity, model and connection, access, then working limits. Local provider/model controls share a row when space permits; hosted discovery keeps its full width. A single available team is shown as context instead of a redundant dropdown.
- Technical execution details are expandable. Hosted conversation/tool disclosures, unavailable selections, and provider errors remain explicit. The save action explains its current blocker.
- Core tools and skills use consistent selection treatments. Empty MCP and skill sections are concise. Working limits show their current values before expansion.
- The workspace library presents three clear choices: write a skill, import a skill, or browse the official MCP registry. Choosing a skill action replaces those choices with its focused editor. Back navigation preserves the draft and restores focus.
- Import starts with a file choice or an explicit paste action; the instructions are reviewed before saving. Attribution is optional and expandable. Nothing is granted to an agent implicitly.
- Workspace settings use aligned appearance and connection controls, explicit connection status, expandable explanations, and visible active restrictions. Extracted this presentation from `TeamPanels` into `WorkspaceSettings` without creating a new state store.

## Verification

- 175 web tests passed, including draft retention, explicit skill grants, keyboard focus restoration, and retry after failed import.
- Type checking, production build, formatting, architecture and quality gates passed. The production build still emits its existing large-bundle advisory.
- Browser checks used the live workspace for settings, toolkit availability, and agent setup. An isolated fixture verified skill editing, provider discovery presentation, light/dark themes and a 430px viewport without modifying saved agents or spending provider credits.
- Live evidence: `.lokai/manual-testing/settings-refinement-live.jpg`. Test fixtures and screenshots remain outside the shipped source.

The running local engine still predates workspace skill support. Its create/import actions correctly remain disabled with an engine-update explanation. This UI slice does not restart or migrate the user's live engine, or claim remote MCP authentication, MCP mutations, or native vendor harness support.

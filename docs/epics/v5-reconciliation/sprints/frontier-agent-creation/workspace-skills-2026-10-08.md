# Workspace capability library — October 8, 2026

## Delivered

The existing Tools & MCPs screen now includes Skills, with the same resource list and inspector. Users can author a skill or review/import a standalone SKILL.md, inspect its source and immutable version, see agents with saved access, and revoke the version. The agent create/edit form exposes the same workspace library inline: adding a capability preserves the draft and does not automatically grant it. Users select the new skill and save the agent. Revoked or missing skills become an actionable setup problem in the existing agent inspector; new assignments are blocked until the saved selection is repaired.

Browse entries lead to the [official MCP registry](https://registry.modelcontextprotocol.io/) and [Anthropic skill examples](https://github.com/anthropics/skills). These are discovery links, not an installed catalog or a claim that listed services are free, safe, connected, or compatible. There is no Tetonic marketplace purchase flow.

## Integration

`workspace_skills` lives in the existing SQLite Store (schema 67), scoped by organization and team. Enabled participants may read; only the current team owner may import/revoke. The content-derived ID includes workspace scope and fits provider tool-name limits. Imported versions are immutable; an identical import retries without undoing revocation. Source labels are provenance supplied by the owner, not verified signatures. Catalog polling returns metadata only.

The agent's existing `requested_tools` definition and job capability bindings carry skill IDs. No parallel agent identity, grants table, scheduler, credential store, or inference loop was added. Runtime profiles and the shared saved-agent resolver admit current skill IDs. `SkillToolHost` composes around the existing registered/MCP host: only granted metadata is advertised; a skill call loads the reviewed instructions as a normal tool result. The current database authority is rechecked on load. The registered executor wraps its existing execution authority to deny stale grants and interrupt active work on revocation. Checkpoint readiness rechecks availability; the content-derived ID pins the version across reconstruction. Skills require no filesystem root.

Hosted inference retains its existing provider/endpoint/tool disclosure and brokered execution. Skill content uses that same consent and data path. A skill's `allowed-tools` frontmatter is not a permission grant. Skills cannot install software, provision credentials, execute scripts, or alter permissions. Other tools remain subject to their existing authority, approvals, sandbox, and egress controls.

## Supported profile

- Standalone UTF-8 SKILL.md with required name/description YAML frontmatter and Markdown instructions; see the [Agent Skills specification](https://agentskills.io/specification).
- 32 KiB file, 4 KiB frontmatter, 128 versions per workspace. Bounded YAML nesting; aliases, anchors and tags are rejected. The parser supports ordinary quoted/multiline YAML metadata.
- No skill bundles, reference-file ingestion, executable resources, arbitrary path reads, URL downloads, automatic updates, or implicit capability grants. Referenced files are not materialized.
- Revocation blocks future admission and further execution via existing authority checks/watchers. It cannot erase content already delivered to a model or undo effects completed before revocation. Reimport does not restore a revoked version; an explicit restore workflow remains open.
- General Tetonic harness only. Guide/director managed profiles keep their existing restricted controls. Native vendor harnesses are not newly enabled by this change.

## Evidence

- Memory tests: scope isolation, owner-only import, persistence across restart, immutable content versions, revocation surviving duplicate import. Full memory library suite: **178 passed** (including migration crash/rollback recovery).
- Production managed executor tests with mocked OpenAI, Anthropic and Google transports: all three advertise only granted skills, deliver skill bodies only after the tool call, preserve provider result IDs, do not gain shell access from frontmatter, deny subsequent admission after revocation, and interrupt an inference call on revocation. No paid provider requests used. Full application library suite: **250 passed, 3 ignored**.
- Frontend suite: **173 passed**, including preserving an edited agent draft while creating a skill, requiring explicit skill selection, hosted skill-only access without a folder, and explicit revocation.
- TypeScript/Vite build, strict Clippy for app/memory/CLI, architecture and quality gates passed. Browser inspection exercised the author/select flow using an isolated fixture with the actual components.
- A newly built CLI running against a disposable database passed authenticated HTTP import/read/catalog/revoke/reimport checks and rejected unauthenticated skill reads. The validation process was stopped afterward. The user's running server/database was not restarted or migrated; the new engine build must be used to enable the backend in that workspace.

## Next work, still open

1. Authenticated remote MCP profile: durable connection definitions, existing secret vault references, endpoint/issuer/audience binding, connection test/discovery, explicit authorization and revocation. OAuth requires a real managed lifecycle; a pasted API token is not OAuth support.
2. Governed mutations: effect proposals, exact reviewed arguments, approvals where required, cancellation/uncertain-outcome receipts and retry semantics through the existing ActionBroker. A `readOnlyHint` is not authority.
3. Managed native vendor harness: bridge the same saved agent capability bindings into vendor execution without ambient tools bypassing Tetonic's controls.
4. Bundle-aware skill import and explicit version restore/update; qualify delegated-child and reconstruction cases with skills.
5. Broader operator experience: ongoing efforts, human requests, completed outcomes and since-last-visit changes through the existing map/inspector. This slice improves capability availability and revocation visibility; it does not claim to deliver that entire workstream.

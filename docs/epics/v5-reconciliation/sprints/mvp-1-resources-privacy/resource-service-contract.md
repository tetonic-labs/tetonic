# Resource service boundary — MVP-101

Status: service boundary, persistent membership authority, local bearer verification and local operator bootstrap implemented; remote transport integration and authorized membership administration remain required. See the [local operator runbook](local-control-runbook.md).

The Application composes ResourceService from the same SharedStore used by its managed run service. Missing storage is an error; no in-memory substitute or separate control database is created. The service currently creates and reads organizations and teams. It does not activate agents, allocate budgets, or grant access to execution, tools or knowledge.

Every operation, including an exact retry, asks an explicitly supplied ResourceAuthority to authenticate a credential and authorize an exact typed action. Organization creation is a separate action from team creation. A read of one team must not imply access to every team in that organization. The authority is a trusted server composition dependency, never selected by a request or agent. There is no default allow provider.

An authenticated principal becomes the owner of a newly created team. The client cannot supply another owner or assert roles through the request body. Ownership remains a stored reference, not a substitute for current membership and policy checks. Ownership transfer will need its own authorized operation. Exact creation retries preserve the original record; another principal cannot overwrite ownership by repeating the create request.

Authorization occurs before resource lookup. Denied requests cannot distinguish missing resources through storage results. Errors exposed by the service omit raw database errors and credentials. The provider is called again for every operation; the service does not cache a prior allow decision. This slice does not make revocation atomic with an already admitted database operation. Stronger cancellation of queued mutations needs a versioned grant/admission transaction, not an undocumented guarantee.

## Persistent membership authority

`Application::membership_resource_service` binds a CredentialVerifier to the application's own store. Verification establishes a stable issuer-qualified employee principal; the single-statement storage decision checks its enabled state and current memberships on every operation. LocalCredentials implements the verifier for locally issued bearer credentials. Enterprise identity providers remain an integration contract, not an implemented login protocol. Worker certificates are not employee credentials.

Schema 30 adds enabled principals, a platform-administrator bit, organization roles, and explicit team membership. Initial metadata permissions are:

| Role or relationship | Allowed resource operations |
|---|---|
| Enabled platform administrator | Create organizations; existing-organization access still requires explicit membership |
| Organization administrator | Read organization, create teams, read team metadata in that organization |
| Organization team creator | Read organization and create teams |
| Organization member | Read organization |
| Team owner or explicit team member, with current organization membership | Read that team's metadata |

These roles grant no execution, tool, budget, private conversation or knowledge permissions. A disabled principal fails every operation. Removing organization membership cascades removal of explicit team memberships; rejoining the organization does not restore those memberships. The durable owner reference is retained, so an owner who rejoins regains owner metadata access unless ownership is explicitly transferred. There is no transfer endpoint yet. Revoking an explicit team membership does not override a separate owner or organization-administrator entitlement.

Migration preserves existing teams but invents no principals or memberships. Bootstrap must explicitly provision the first principal and organization administrator. Store mutation methods are trusted internal provisioning primitives; exposing them directly would bypass the service boundary. Membership administration still needs authenticated operations, top-down policy ceilings and audit events before transport exposure.

## Required before transport exposure

- Remote credential delivery and authenticated administrator operations; the existing bootstrap/issuance path is restricted to local database operators. Enterprise SSO remains a separate adapter.
- Authorized administration of the persisted memberships, plus capability grants and top-down policy limits.
- Audit records for administrative mutations and grant changes, without recording credentials.
- Request bounds and supported transport/session security.
- Transport-level integration tests, beyond the local verifier/resource-service integration test.

The existing fleet dispatcher is not routed through this service yet. Do not dual-write its maps or call it behind an authenticated facade and imply that execution is reconciled. Resource migration, immutable agent definition revisions, managed activation and operator control cutover remain separate acceptance obligations in the epic.

## Local credential profile

`Application::local_credentials` uses the same store and a required deployment audience. Trusted provisioning can issue a credential only for an existing enabled principal. Issuance grants no memberships or platform role. Credentials have a requested lifetime of 1–86,400 seconds, independent random secrets, and a separate public identifier for revocation. The secret is returned in memory for protected delivery; Debug output redacts it. Database rows contain a domain-separated SHA-256 digest, never the bearer value. The random source is the existing UUID v4 dependency's OS random generator, with two independent UUIDs per secret.

Verification rejects malformed/unknown secrets, the wrong audience, not-yet-valid or expired credentials, revocation and disabled principals. It uses current wall-clock UTC and persisted state without an allow cache; operators must maintain correct clocks. A restarted service can still verify an unexpired credential and rejects a revoked one. Separate credentials can be revoked independently.

Issuance and first revocation append credential lifecycle events in the same database transaction. Event failure rolls back the mutation and returns failure. Those events record the credential's principal, not an authenticated administrative actor: full administrator audit and authorized remote issuance are still unimplemented. The public Rust provisioning methods are trusted host-code operations, not transport endpoints. Exposing them to request payloads would be a privilege bypass.

Deployment audience must be stable for a deployment and changed for a separately restored/cloned deployment if credentials should not carry over. Deliver bearer values only through protected local output or authenticated encrypted transport. No HTTP login, refresh protocol, token file persistence, retention cleanup or TLS listener is introduced by this slice. Revocation rejects subsequent verification; it does not retroactively cancel already admitted mutations or executions.

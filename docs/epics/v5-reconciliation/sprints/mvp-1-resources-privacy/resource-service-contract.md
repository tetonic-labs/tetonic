# Resource service boundary — MVP-101

Status: first service slice; production authentication and membership persistence remain required.

The Application composes ResourceService from the same SharedStore used by its managed run service. Missing storage is an error; no in-memory substitute or separate control database is created. The service currently creates and reads organizations and teams. It does not activate agents, allocate budgets, or grant access to execution, tools or knowledge.

Every operation, including an exact retry, asks an explicitly supplied ResourceAuthority to authenticate a credential and authorize an exact typed action. Organization creation is a separate action from team creation. A read of one team must not imply access to every team in that organization. The authority is a trusted server composition dependency, never selected by a request or agent. There is no default allow provider.

An authenticated principal becomes the owner of a newly created team. The client cannot supply another owner or assert roles through the request body. Ownership remains a stored reference, not a substitute for current membership and policy checks. Ownership transfer will need its own authorized operation. Exact creation retries preserve the original record; another principal cannot overwrite ownership by repeating the create request.

Authorization occurs before resource lookup. Denied requests cannot distinguish missing resources through storage results. Errors exposed by the service omit raw database errors and credentials. The provider is called again for every operation; the service does not cache a prior allow decision. This slice does not make revocation atomic with an already admitted database operation. Stronger cancellation of queued mutations needs a versioned grant/admission transaction, not an undocumented guarantee.

Required before transport exposure:

- A real credential verifier with expiry/revocation handling and an explicit local bootstrap path.
- Durable principal, organization membership, team membership and capability grants with top-down policy limits.
- Audit records for administrative mutations and grant changes, without recording credentials.
- Request bounds and supported transport/session security.
- Integration tests against that concrete verifier and grant store; the current test authority is only a service-boundary fixture.

The existing fleet dispatcher is not routed through this service yet. Do not dual-write its maps or call it behind an authenticated facade and imply that execution is reconciled. Resource migration, immutable agent definition revisions, managed activation and operator control cutover remain separate acceptance obligations in the epic.

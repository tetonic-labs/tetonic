# Current provider models in the agent editor

October 7, 2026. Follow-on to FAR-002, reusing account-backed discovery.

## Finding and change

The agent and Guide editors already query provider models after a key is saved. The manual workspace has no saved OpenAI, Anthropic or Google key, so it cannot retrieve an authenticated account catalog. The previous editor offered an empty/raw-ID dropdown; the server alphabetized results and discarded provider names and dates.

The same authenticated endpoint now returns its existing `models` array plus `entries` metadata and a successful `fetched_at` timestamp. Discovery continues through the OS credential store, bounded pagination and EgressGuard, without inference or a hardcoded model shortlist. New OpenAI/Anthropic entries sort by provider creation date, newest first; undated entries preserve provider order. Anthropic/Google display names accompany exact IDs. Google retains its `generateContent` filter. Listing does not certify tool capability or inference entitlement.

The existing agent/Guide picker has search, refresh, last successful check, readable names and an exact selected-ID readout. Search and refresh never change the selection. Failed refresh retains the previous list with a stale message. Replacing/removing keys invalidates the old account catalog, and canceled requests cannot repopulate it. Manual IDs and saved selections remain supported; the local selector is unchanged.

Before connecting, the picker links to the lab's official public catalog and explains the API-key requirement. Public listings are not imported or represented as account access. Browsing changes no agent, tool grant, model default or execution path. Optional frontend metadata fields preserve compatibility with the existing engine response.

## Verification and boundaries

- Frontend: 170 tests passed across 26 files; TypeScript and production build passed. Existing Vite chunk-size advisory remains. Seven discovery tests passed again after adding the exact selected-ID readout.
- Application provider suite: 14 passed, covering pagination, unfamiliar future IDs, metadata ordering/validation, creation, Guide planning and mixed-provider team/tool execution with controlled transports.
- Strict Clippy passed for application and CLI with tests and warnings denied. Architecture/static-quality and diff-whitespace gates passed.
- Browser: verified OpenAI connection guidance in the current Create agent view. An isolated, explicitly labeled fixture exercised the connected list, filtering and retained selection using existing editor styles. No test agents or credentials were saved in the user's workspace.
- Logs: `.lokai/manual-testing/model-catalog-{web-tests,web-build,rust-tests,clippy}.log`. Ignored visual fixture: `web/.lokai/model-catalog-preview/`.

Vite serves the frontend changes. The running engine was not restarted or its database migrated: metadata and ordering require running the updated engine. Its existing endpoint already fetches current provider IDs after connecting a key. Live authenticated lab discovery remains unverified because the workspace has no provider keys. No paid inference occurred. Vendor harnesses and broader FAR-002 qualification remain open.

## Protocol references checked

- [OpenAI list models](https://developers.openai.com/api/reference/resources/models/methods/list): model IDs and creation timestamps.
- [Anthropic list models](https://platform.claude.com/docs/en/api/models/list): names, creation dates and cursor pagination.
- [Google Models API](https://ai.google.dev/api/models): display names, generation methods and page tokens.

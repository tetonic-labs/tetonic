# One-process local product entry

Date: October 13, 2026. Parent: OCT-301; supports EXP-001/009. Scope: one owner, one machine. Acceptance is partial; this is not a release-ready declaration.

## Problem and delivered behavior

The owner repeatedly reached a disconnected preloader when opening the local address in a different browser. The preview required both a Vite process and an engine process, and the connection credential was intentionally tab-scoped. The entry screen could retry but could not accept a connection link. A working browser had no product control for copying one.

- `tetonic ui --ui-dir <built-ui-directory>` serves the existing built interface and authenticated API from the same loopback listener. The printed connection URL opens that interface. `--port 0` selects a free port and prints the actual address. No Vite process, proxy or frontend development runtime is needed once the UI and executable have been built.
- The original development mode remains available. Explicit `--ui-origin` and `--ui-dir` are mutually exclusive.
- The preloader distinguishes missing credentials, rejected credentials, transport failure, timeouts and API-version mismatch. A labeled connection form accepts a full local link, with normal form submission. Version mismatches direct the operator to update the app and engine.
- Workspace settings can copy a working connection link for another browser on the same computer. It also offers the recovery form after disconnection.
- Connecting to a different engine session clears the previous workspace projections and rotates draft identity. The existing engine connection owner and polling implementation remain authoritative.

The map, Guide workflow, engine data model, execution controls and visual design remain the existing product. This does not add another server or scheduling path.

## Boundaries

Only an explicit built UI directory is served. Startup reads a bounded immutable bundle: index, the product icon/favicon and recognized assets. Hidden files, source maps, databases, connection files and unrelated extensions are excluded. Links/junctions inside the bundle are rejected; the operator-supplied bundle remains trusted application code. Requests never turn URL paths into filesystem reads.

The existing bearer, Host, Origin and API-version checks still protect `/api`. Serving public product files does not make workspace state public. Static files allow GET/HEAD, reject unexpected Host/Origin and carry no-store, no-referrer and framing/content-type protections. Rebuild/restart is required to change loaded UI files.

Connection keys remain in session storage, are removed from the visible URL on entry and are never embedded in HTML or persistent browser storage. A copied link grants the same owner access as the launcher link; it is explicitly labeled accordingly. The form does not navigate to pasted input or send it to another endpoint. Mismatched local origins require opening that address explicitly.

## Validation

Validated on Windows, on `codex/working-setup` over base `7f8202b6`:

| Check | Observed result |
|---|---|
| `cargo test --locked -p tetonic-cli --bin tetonic --test local_ui_product` | 16 binary tests and one real-process HTTP integration test passed. The integration starts an isolated database on an ephemeral port, serves public product files, rejects missing/wrong API credentials and returns the authenticated empty workspace. |
| `cargo run --locked -p tetonic-arch-gate -- verify package` | Passed formatting, workspace Clippy with warnings denied, architecture and static quality checks. Request definitions and their existing boundary tests were moved into `local_ui/requests.rs`; the adapter remains under the size limit. No gate exception was added. |
| `npm test -- --maxWorkers=2` | All 314 Vitest cases and 13 architecture/appearance checks passed. Includes actual entry-form Enter submission, expired-key recovery, connection isolation, cancellation versus timeout, malformed links and API mismatch. |
| `npm run build` | Passed TypeScript and production build. Existing bundle-size advisory remains (about 744 kB minified JS before gzip). |
| Prettier on all changed frontend files | Passed. Repository-wide `npm run format:check` separately reported 65 unchanged files with formatting drift; this slice does not rewrite those files. |
| Real built UI in the in-app browser | A fresh unauthenticated session showed the connection form. Opening the engine-generated link entered the empty map; the URL no longer contained its credential. Workspace settings showed Connected and acknowledged Copy connection link. No warning/error entries were observed in that connected tab's developer log. |

The browser test used a new isolated profile, no filesystem grants, no provider key and no inference calls. The temporary engine was stopped afterward; existing review data was not reset. The browser automation exposes a separate virtual clipboard that remained empty after the app's successful Clipboard API write. Consequently, real cross-browser paste is **not** claimed: connection parsing/form behavior and clipboard success/failure are covered by automated frontend tests, while copying into an external browser remains a manual acceptance check. The symlink-specific Unix test was not run on this Windows machine.

The full Rust workspace test suite, packaged installation and live hosted-model work were not rerun for this transport/UI entry change. Existing October 11 hosted-model evidence remains separate.

## Remaining October gates and order

This entry fix does not close OCT-301 or EXP-009. No installer or desktop wrapper is delivered here, and no clean-machine installation, useful model execution or multi-day soak is claimed by these entry tests.

Return immediately to the existing first-use and coordinated-work scope:

1. Complete the actual browser journey with a qualified hosted model and useful tools. Include two independent efforts, one waiting for a human while the other progresses, clear decisions and combined results. Preserve the existing map and Guide interaction.
2. Complete parked team restoration and restart handling under OCT-203, including descendants and uncertain tool effects. Existing coordinator checkpoints do not constitute full team recovery.
3. Complete the bounded recurrence already required by OCT-204. New autonomous-team families, learning experiments, marketplace work and visual redesign stay out of this work.
4. Finish supported installation and operator configuration, then clean-install/upgrade/restore checks, fresh-user observations and the required bounded soak. Feature freeze remains October 18; the October 25 decision uses observed evidence.

The original October P0 gates and additional first-use gate remain in force. Any reduction requires an explicit owner decision; it is not hidden in implementation notes or a completion percentage.

# Litho Layer (`engine/litho/`)

## Purpose
The **Litho** layer contains human workbenches, interfaces, and developer coding tools. It provides user-facing desktop and CLI frontends, the background service host daemon, language server protocol integrations, and execution tool implementations.

## Packages
- [`tetonic-cli`](./tetonic-cli): Interactive terminal user interface, REPL, and developer command center.
- [`lokai-app`](./lokai-app): Unified application service layer, workflow orchestrator, and service host.
- [`tetonicd`](./tetonicd): Headless background daemon serving JSON-RPC APIs for editor extensions and GUI clients.
- [`lokai-lsp`](./lokai-lsp): Language Server Protocol client communicating with external language servers.
- [`lokai-tools`](./lokai-tools): Standard tool implementations (file viewing, diffing, patch application, shell execution) bound to sandboxed executors.

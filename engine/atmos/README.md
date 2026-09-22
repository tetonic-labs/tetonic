# Atmos Layer (`engine/atmos/`)

## Purpose
The **Atmos** layer serves as the external gateway and distributed network mesh. It mediates all outbound and inbound communication, including multi-provider inference gateways, peer-to-peer fabric networking, RPC wire protocols, and security egress guardrails.

## Packages
- [`lokai-inference`](./lokai-inference): Unified inference adapter supporting local backends (Ollama, vLLM) and cloud APIs (OpenAI, Anthropic).
- [`lokai-egress`](./lokai-egress): Strict network proxy and egress policy firewall preventing unauthorized network traffic.
- [`lokai-rpc`](./lokai-rpc): Transport schemas, message serialization, and RPC client/server bindings.
- [`lokai-fabric-client`](./lokai-fabric-client): Coordinator and client logic for participating in the distributed fabric compute network.
- [`lokai-fabric-protocol`](./lokai-fabric-protocol): Transport-neutral wire specifications, envelope schemas, and signed result protocols.

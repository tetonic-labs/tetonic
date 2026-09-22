# Hosted inference adapter

This is a library integration point for a **Chat Completions-compatible** service,
not universal vendor support. Local CLI/daemon assembly remains unchanged. No key
is loaded, endpoint enrolled, or paid request made just by installing this code.

## Ownership and construction

- **Product composition** selects the endpoint/model and derives an explicit
  `HostedInferencePolicy` from effective project/session settings. Local-only
  policy must produce the default disabled policy. Rebuild the binding when the
  effective disclosure permission changes.
- **Policy** decides whether the aggregated data class may leave the machine.
  Even an explicitly permissive ceiling cannot disclose `Secret` material.
- **Inference** owns model-specific wire translation and completion validation.
  A mandatory domain `SecretScanner` scans the complete serialized outbound body
  (messages, arguments, tool schemas, and output schema). Findings or scanner
  failures stop the request; this adapter never silently redacts and retries.
- **Egress** owns credentials on the wire, exact endpoint grants, TLS, DNS pinning,
  timeout and byte limits. A hosted grant never enrolls a fabric node.
- **Core/tools** continue to execute model-proposed operations with normal gating.

Construction order:

1. Obtain an `Arc<EgressGuard>` and explicitly call
   `allow_hosted_endpoint("https://your-provider.example/v1/chat/completions")`.
   The complete URL is enrolled; other paths are not allowed. Grants are revocable
   with `revoke_hosted_endpoint`. Ordinary egress methods do not inherit them.
2. Implement `HostedCredentialSource` using the host's existing credential store.
   Return `BearerCredential::new(secret)` at request time. Do not store credentials
   in the model profile. Credential diagnostics are redacted by the transport.
3. Construct `EgressHostedTransport` with the same guard, endpoint, and credential
   source. Construction itself does not enroll or contact the endpoint.
4. Create `HostedModelConfig` with an exact model ID, positive output-token cap,
   supported tools/schema flags, whether temperature is accepted, and either
   `OutputLimitField::MaxTokens` or `MaxCompletionTokens` for that service/model.
5. Construct `HostedChatProvider::new(config, policy, scanner, transport)` and use
   it through `Arc<dyn InferenceProvider>` at the host's admitted inference boundary.

Do **not** put it in `PooledProvider`'s local slot: that slot means local execution.
Do not bypass ComputeBroker admission, budgeting, or attempt lifecycle to expose
it as a production session option. The existing product compute-plane builder is
still local/fabric-specific; first-class hosted scheduler/admission integration and
hosted registration are follow-up work. CLI/session selection can now choose
host-registered admitted profiles through the [application selection API](../../product/lokai-app/INFERENCE-SELECTION.md),
but a raw hosted adapter is not such a profile. This supplies the callable adapter
and transport, not a cloud-enabled product switch.

The protocol follows the documented [Chat Completion interface](https://huggingface.co/docs/inference-providers/tasks/chat-completion).
Profiles must be validated against the selected service; compatibility is not
inferred from a vendor/model name.

## Request and response behavior

- Requests bind to exactly one configured model; mismatches fail before transport.
- `num_ctx` and `keep_alive` remain local allocation hints and are not transmitted.
  Model digests and speculative draft settings are rejected. Unsupported tools
  or JSON schema requests fail explicitly.
- Core currently stores ordered tool calls without vendor call IDs. The adapter
  reconstructs matching IDs for assistant calls and following tool results on
  each request. Missing, orphaned, or reordered tool results are rejected.
- A successful response requires exactly one assistant choice and a complete
  `stop` or `tool_calls` finish status. Truncation, refusal, malformed arguments,
  inconsistent status, and provider errors do not publish a partial answer.
- Token counts are retained when supplied; unknown timing/usage remains unknown.
- Each HTTP request has a 120-second total timeout (including DNS/body receipt),
  10-second connect timeout, and 8 MiB request/response limits. TLS verification
  stays enabled. Redirects and inherited proxies are disabled. Resolved private
  and special-purpose addresses are rejected; local test servers use mock transport.
- There is no automatic retry, fallback, or provider-error-body logging. HTTP
  failures expose status codes without echoing potentially sensitive bodies.
- Revocation blocks subsequent requests. It does not recall bytes already sent
  or cancel an already authorized in-flight request.

## Deliberate limits

Completions are **buffered**, then delivered once through the existing token sink.
No SSE streaming, images/audio, provider reasoning-state continuation, native
Responses/Messages protocols, automatic hybrid routing, or monetary accounting is
implemented. Models requiring opaque reasoning state need a dedicated adapter.
The output-token cap bounds a request; it is not a session currency budget.
Dropping the request future stops local waiting, but cannot guarantee the remote
service stops generation or billing. Existing agent cancellation may only be
observed between model calls; live provider cancellation needs product wiring.

Tests use mock completions, injected scanners, and denied egress requests. They
exercise tool round trips, schema and budget translation, denial-before-send,
credential redaction, and endpoint/IP validation. No live provider account was used.

# Generated-client and server-only combinator scope

The generated client intentionally follows Servant's `HasClient` shape where an
API combinator contributes request data that a client can actually send. Some
server combinators observe server-side state instead of request arguments and
therefore remain server-side by design.

## Generated-client supported

- Static paths, captures, capture-all, query params, query flags, headers,
  request bodies, normal verbs, no-content verbs, response headers, `UVerb`
  response unions, and streaming `StreamVerb` endpoints.
- Streaming endpoints use `call_stream` over a `RunStreamingClient` transport.
  `sse_get()` uses `EventStreamFraming`, so SSE clients yield one parsed
  `ServerEvent` per event block.

## Server-only by design

- `Vault`, `WithResource`, `IsSecure`, `HttpVersion`, and `RemoteHost` read
  server adapter/context state. They are not sent by a generated HTTP client.
- `AuthProtect` is server-side generalized authentication. Clients should send
  the concrete headers or bodies the chosen authentication scheme requires.
- `BasicAuth` is currently server-side plus docs/links. A future ergonomic
  client helper may add explicit `Authorization` construction, but the core
  client should not silently capture credentials from context.

Keeping these boundaries explicit prevents generated clients from pretending to
control transport facts such as TLS state or peer address.
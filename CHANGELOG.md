# Changelog

All notable changes to this workspace are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project uses semantic versioning once published.

## [Unreleased]

## [0.2.0] - Unreleased

### Added

- Servant parity combinators across the shared API description and relevant
  interpretations: `OperationId`, `Fragment`, `QueryString`, `DeepQuery`,
  `Host`, `Raw`, `RawM`, and request `StreamBody`.
- Generated-client support for full-query replacement, structured deep-object
  queries, `Host` headers, raw responses, and streaming request bodies.
- Markdown docs and OpenAPI Specification document generation for the new
  metadata, query, host, schema, and streaming surface, with Raw/RawM documented
  as opaque and omitted from OAS output by policy.
- Checked OAS generation for duplicate `operationId` values and incompatible
  component schemas.
- ComprehensiveAPI-style parity coverage that exercises server, generated
  client, links, Markdown docs, and OAS output from one API fixture.
- Streaming Server-Sent Event client parsing via `EventStreamFraming` and
  `MimeUnrender<EventStream>` for `ServerEvent`.
- `MultiVerb`-style union response helpers: `WithStatusNoBody`,
  `WithFixedStatus`, and `WithStreamingStatus`.
- Nested structural `#[derive(ToSchema)]` support for object fields,
  `Option<T>`, `Vec<T>`, and arrays.
- GitHub Actions CI for `cargo fmt --all -- --check`, workspace `clippy`, and
  workspace tests.
- Release-hardening documentation for versioning, publishing, TLS termination,
  and generated-client scope.

### Changed

- `sse_get()` now uses `EventStreamFraming`, so generated streaming clients can
  yield each SSE event incrementally instead of waiting for EOF.
- Streaming generated clients validate response status and `Content-Type` before
  exposing the decoded item stream.
- Workspace crates now use version `0.2.0` for the public parity API additions.

### Unsupported or deferred

- servant-rs does not provide Haskell syntax compatibility, a built-in OpenAPI
  Overlay engine, JSONPath evaluator, Arazzo support, browser EventSource
  adapters, or a complete TLS deployment matrix.
- `Vault`, `WithResource`, `WithNamedContext`, `AuthProtect`,
  `IsSecure`, `HttpVersion`, and `RemoteHost` remain server-side by design;
  `BasicAuth` remains server/docs/links-only for generated-client ergonomics.

## [0.1.0] - Unreleased

Initial public release target.

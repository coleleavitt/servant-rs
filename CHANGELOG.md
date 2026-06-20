# Changelog

All notable changes to this workspace are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project uses semantic versioning once published.

## [Unreleased]

### Added

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

## [0.1.0] - Unreleased

Initial public release target.
# Testing Rules

These rules always apply.

- Add tests for every supported combinator and every routing or extraction behavior.
- Add integration tests that exercise multiple implemented features together. Avoid accepting isolated examples that break when combined.
- Keep an equivalent of `Servant.Test.ComprehensiveAPI` in Rust once the DSL exists.
- Test route precedence, left-biased alternatives, static versus capture routes, capture-all routes, raw routes, trailing slashes, and best-error selection.
- Test content negotiation for `Accept` and `Content-Type`, including missing, wildcard, unsupported, and malformed headers.
- Test body extraction failures, JSON/form/plain/octet codecs, response headers, no-content responses, and custom error formatting.
- Test typed client generation with local round-trip servers when possible.
- Use property tests for URL encoding/decoding, query serialization, route layout stability, and generated link safety.
- Use golden tests for docs/openapi output.
- Run `cargo test --workspace --all-targets` before finalizing implementation work. If it cannot run, record the reason.

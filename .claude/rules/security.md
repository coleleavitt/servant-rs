# Security Rules

These rules always apply.

- Treat all request path segments, query values, headers, bodies, cookies, and auth tokens as untrusted.
- Use structured parsers for HTTP, MIME/media types, paths, query strings, headers, JSON, forms, and cookies. Avoid ad hoc string parsing when a maintained crate or existing project parser is available.
- Preserve percent-encoding and UTF-8 behavior intentionally. Add tests for malformed encodings, duplicate query keys, empty path segments, trailing slashes, and reserved characters.
- Never allow `Raw` or static-file support to bypass path traversal protections.
- Bound request bodies and streaming buffers. Do not add unbounded buffering in extraction, content negotiation, or client response handling.
- Keep auth data out of logs and error messages. Redact `Authorization`, cookies, JWTs, API keys, and session identifiers.
- Model auth failures distinctly from parse failures and internal errors. Use stable status codes and response shapes.
- For JWT/cookie work, require explicit algorithms, validate expiration and audience/issuer when configured, and use constant-time comparisons for secrets.
- Do not introduce network calls in tests unless they are explicitly integration tests with a local server.
- If generated clients or docs include examples, make sure they do not embed real secrets or local credentials.

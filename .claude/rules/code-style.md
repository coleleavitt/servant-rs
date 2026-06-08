# Code Style Rules

These rules always apply.

- Write idiomatic Rust first. Preserve Servant's guarantees, not Haskell's syntax.
- Keep modules small and named around behavior: routing, extraction, codecs, errors, client, docs, auth, streaming.
- Do not create god objects. Split API descriptions, routing trees, extractors, codecs, handlers, context, clients, and docs into separate types/modules with explicit composition.
- Do not add feature-local state that bypasses the shared API/routing/client/docs architecture.
- Prefer explicit data types over stringly typed internal protocols.
- Use `Result` and typed errors. Avoid panics in library code except for impossible internal invariant violations that are documented and tested.
- Keep public traits coherent and hard to misuse. Use sealed traits when external implementations would break invariants.
- Avoid macros until normal Rust APIs prove too noisy. If a macro is added, keep the expanded model documented and test it with compile tests where practical.
- Make feature flags additive. Default features should be useful but not pull in every backend.
- Keep examples short, compiling, and realistic.
- Use rustdoc on public APIs that define the user-facing DSL or behavior.
- Format with `cargo fmt --all`; do not hand-align code that rustfmt will rewrite.

# Project Memory

- Goal: rewrite Haskell Servant as idiomatic Rust in `servant-rs`.
- Reference checkout: `research/servant/`, ignored by git and treated as read-only research input.
- Current source of truth for upstream package boundaries: `research/servant/cabal.project`.
- Core parity target: `research/servant/servant/src/Servant/Test/ComprehensiveAPI.hs`.
- Router behavior reference: `research/servant/servant-server/src/Servant/Server/Internal/Router.hs`.
- Client generation reference: `research/servant/servant-client-core/src/Servant/Client/Core/HasClient.hs`.
- Codegraph is initialized locally; it becomes most useful once Rust source exists.
- Architecture guardrail: build one typed API model that can be interpreted as server routing, client generation, docs/openapi, links, and tests.
- Scope guardrail: first version is not a docs-only `utoipa` wrapper, not an Axum clone, and not a macro-heavy DSL before the core model is proven.
- Quality guardrail: every feature needs integration coverage with other features, not just an isolated example.
- [Env & resources](memory/env-and-resources.md) — toolchain (rustc 1.98-nightly), network available, hyper fork codegraph-indexed at `~/RustProjects/forks/hyper`, codegraph won't parse Haskell.
- [Handler-derivation design](memory/handler-derivation-design.md) — committed architecture: one description, HList args+arity macro, ArgShape matrix, router/RouteResult semantics, sealed combinators. See docs/DESIGN.md.
- Design docs live in `docs/`: DESIGN.md (committed spec), RESEARCH-NOTES.md (per-subsystem reference map), DESIGN-CRITIQUE.md (reconciliation + build sequence).
- [Implementation status](memory/implementation-status.md) — what's built (4 crates, 4 interpretations, ~85 tests green) and what's deferred; how to add a combinator.

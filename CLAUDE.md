# servant-rs Claude Instructions

## Mission

Rewrite Haskell Servant into a Rust project named `servant-rs`.

The Haskell reference checkout lives at `research/servant/`. Treat it as read-only research input and do not commit it. The Rust implementation should be idiomatic Rust that preserves Servant's core ideas: typed API descriptions, composable routes, request extraction, content negotiation, generated clients, documentation/openapi support, auth, streaming, and strong test coverage.

This project is not meant to become a docs-only OpenAPI helper like a thin `utoipa` wrapper. The central value is that one typed API description drives server routing, handler shape, client generation, links, and documentation where practical. Documentation should fall out of the API model; it should not be the only thing the model validates.

If a Rust crate/workspace does not exist yet, scaffold it before implementing features. Prefer a workspace that can grow in layers:

- `servant`: core API description types, combinators, codecs, links, request/response abstractions.
- `servant-server`: runtime routing, extractors, handlers, context, errors, static/raw routes.
- `servant-client`: typed client generation and transport adapters.
- `servant-docs` or `servant-openapi`: generated docs/spec output.
- Optional crates for auth, quickcheck/proptest support, and streaming adapters once the core stabilizes.

## Architecture First

Do not build isolated features that only work in their own examples. Every feature must fit the same shared architecture:

- API description layer: typed route/combinator model and metadata.
- Interpretation layer: server, client, docs/openapi, links, tests.
- Runtime layer: request/response, routing tree, extractors, codecs, context, errors.
- Adapter layer: Hyper/Tower integration first where useful; Axum integration can exist later as an adapter, not as the core model.

Before adding a feature, decide which layer it belongs to and which existing traits/types it composes with. If a feature needs new shared state, define the ownership boundary before coding.

Avoid god objects. Do not let one `App`, `Server`, `Router`, `Context`, or `Api` struct accumulate routing, extraction, docs, clients, auth, codecs, and runtime configuration. Split responsibilities into focused modules with explicit data flow.

## Scope Boundaries

Build for library authors and Rust service developers who want Servant-style typed APIs. Do not optimize the first version for:

- A full replacement for Axum, Actix, Poem, or Salvo application ergonomics.
- A generic OpenAPI annotation framework.
- A macro-heavy DSL before the trait/data model is proven.
- Every Servant package at once.
- Browser UI, dashboards, CLIs, or deployment tooling.
- Backward compatibility with Haskell syntax.

Initial priority is the smallest end-to-end slice: define an API, serve it, call it with a typed client, generate basic docs, and test that those interpretations stay consistent.

## Reference Map

Start research from these files:

- `research/servant/README.md`: upstream project overview.
- `research/servant/cabal.project`: package boundaries.
- `research/servant/servant/src/Servant/API.hs`: public combinator surface.
- `research/servant/servant/src/Servant/API/*.hs`: individual API combinators.
- `research/servant/servant/src/Servant/Test/ComprehensiveAPI.hs`: coverage target for supported combinators.
- `research/servant/servant-server/src/Servant/Server.hs`: public server API.
- `research/servant/servant-server/src/Servant/Server/Internal/Router.hs`: router tree and route selection behavior.
- `research/servant/servant-server/src/Servant/Server/Internal/Delayed*.hs`: request extraction and delayed checks.
- `research/servant/servant-server/src/Servant/Server/Internal/ErrorFormatter.hs`: parse and not-found errors.
- `research/servant/servant-client-core/src/Servant/Client/Core/HasClient.hs`: client generation semantics.
- `research/servant/servant-client-core/src/Servant/Client/Core/Request.hs`: client request representation.
- `research/servant/servant-docs`, `servant-swagger`, `servant-auth`, `servant-quickcheck`: later parity references.

Codegraph has been initialized for this repo. It currently indexes supported non-Haskell files from the reference checkout; after Rust code exists, use it for Rust symbol exploration. For Haskell reference code, use `rg`, `sed`, and focused file reads.

## Design Principles

Do not port Haskell type-level machinery mechanically. Preserve the developer-facing guarantees in Rust terms:

- Make route composition explicit and type checked where practical.
- Keep runtime routing predictable and inspectable.
- Keep server, client, docs, links, and tests tied to the same API description instead of duplicating route definitions.
- Model extraction failures, content-type mismatches, unsupported methods, missing routes, and handler errors as structured values.
- Prefer standard ecosystem types: `http`, `bytes`, `serde`, `serde_json`, `tower`, `hyper`, `futures`, and `mime` where appropriate.
- Avoid global mutable state. Pass context explicitly.
- Keep public APIs small until tests prove the shape.
- Document each intentional semantic difference from Haskell Servant.

## Working Rules

- Do not edit `research/servant/` except when deliberately refreshing the reference checkout.
- Before implementing a feature, identify the Haskell reference module and write down the intended Rust equivalent in code comments, tests, or issue notes when useful.
- Add tests with every user-visible behavior change.
- Add integration tests that combine features. A feature is not done if it only works in isolation.
- Prefer property tests for routing, content negotiation, path/query encoding, and client/server round trips.
- Keep examples compiling. They are part of the API contract.
- Run formatting and tests before handing work back:
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace --all-targets`

If the workspace has not been scaffolded yet, do the smallest useful scaffold first, then add tests around the first implemented slice.

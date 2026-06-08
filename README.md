# servant-rs

An idiomatic Rust port of [Haskell Servant](https://www.servant.dev/). You
describe an HTTP API **once**, as a typed value, and that single description
drives four interpretations that can never drift apart:

- **server** — routing + request extraction + a hyper/tower adapter,
- **client** — a typed client whose call arguments are checked against the API,
- **links** — type-safe internal link generation,
- **docs** — a documentation model rendered to Markdown.

```rust
use servant::prelude::*;

// "hello" :> Capture "name" String :> Get '[JSON] Greeting
//   :<|>
// "health" :> Get '[PlainText] String
let api = alt(
    path("hello", capture::<String, _>("name", get::<(Json,), Greeting>())),
    path("health", get::<(PlainText,), String>()),
);
```

```rust,ignore
use servant_server::{serve, RouterService};

let router = serve(api, (hello_handler, health_handler));
let service = RouterService::new(router); // a tower Service over hyper
```

The handler tuple's shapes are derived from, and checked against, the API type —
`hello_handler` must be `Fn(String) -> impl Future<Output = Result<Greeting, ServerError>>`.

See `cargo run -p servant-server --example greet` for the same API producing the
routing tree, Markdown docs, a safe link, and typed client calls.

## Workspace

| crate            | role |
|------------------|------|
| `servant`        | the typed API description: combinators, content types & negotiation, modifiers, links, errors, the `HasArgs`/`Endpoint` traits |
| `servant-server` | routing tree, the phase-ordered extraction pipeline, and the hyper/`tower` adapter |
| `servant-client` | typed clients over a pluggable `RunClient` transport (hyper included) |
| `servant-docs`   | the documentation model and Markdown renderer |

## How it works

Servant encodes the API at the type level; Rust can't put `&'static str` path
literals in type position, so combinators carry runtime strings in the **value**
while the **type** encodes structure and extracted-argument types. Each
value-extracting combinator contributes one argument; a small heterogeneous list
(`HList`) accumulates them and is handed to an ordinary closure via an
arity-generic `HandlerFn` trait. The same argument list is consumed by the
server, the client, and the link builder, so they cannot disagree.

The router tree, left-biased route selection, `Fail`/`FailFatal` recovery, the
error-priority ladder (`404 < 405 < 401 < 415 < 406 < … < 400`), the phase
ordering of checks, and content negotiation all mirror Servant. Intentional
differences from Haskell are documented inline as `[diff]` and collected in
`docs/DESIGN.md`.

## Status & scope

A thorough, faithful port. Implemented and tested across the interpretations
(server / client / links / docs / OpenAPI as applicable):

- static paths, captures (strict/lenient), capture-all;
- query params (the full `Required`/`Optional` × `Strict`/`Lenient` matrix),
  query-param lists (incl. the `name[]` form), query flags (value-sensitive);
- request headers, request bodies; content negotiation (`Accept` q-values +
  wildcards, `Content-Type` matching);
- verbs with status codes, no-content (204) verbs, **response `Headers`**
  (`VerbWithHeaders`), **`UVerb` union responses** (per-arm status), **streaming
  + Server-Sent Events** (`StreamGet`/`sse_get` with newline/netstring framing);
- `Alt` alternatives + the `alt_all!`/`handlers!` named-routes macros;
- **`BasicAuth`**, **`Vault`**, **`WithResource`** via a server `Context`;
- metadata (`Summary`/`Description`);
- **OpenAPI 3.0** output (`servant-openapi`) and Markdown docs (`servant-docs`).

Cross-interpretation consistency (one description → server + client + docs +
links agree) is tested, including a real-socket hyper round-trip.

Also implemented: `AuthProtect` (generalized auth), `WithNamedContext` (named
sub-context scopes), `IsSecure`/`HttpVersion`/`RemoteHost` (connection info),
per-arm response headers on unions (`WithStatusHeaders`), a **typed streaming
client** (`call_stream` over a `RunStreamingClient` transport), and
`#[derive(NamedApi)]` (record routes) / `#[derive(ToSchema)]` (OpenAPI schemas)
in `servant-macros`.

Intentional simplifications (`[diff]` in source / `docs/DESIGN.md`): `BasicAuth`/
`AuthProtect`/`Vault`/`WithResource`/`IsSecure`/`HttpVersion`/`RemoteHost`/
`Stream` are server-side (no generated typed client); response headers and
unions use a runtime header list rather than type-level header lists; OpenAPI
schemas are name-based (a `#[derive(ToSchema)]` gives object schemas). Remaining
niches not built: `MultiVerb`'s full streaming-arm machinery, a streaming SSE
*client* parser, TLS termination, and recursive/nested derive schemas.

## Developing

Builds on **stable Rust** (no nightly features); MSRV **1.83** (verified —
`cargo +1.83.0 test --workspace` passes).

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo bench --workspace            # criterion microbenchmarks
```

### Benchmarks

Criterion microbenchmarks live in `servant/benches/` (content negotiation,
scalar parsing) and `servant-server/benches/` (end-to-end request dispatch).
Indicative single-core numbers (release; absolute values are machine-specific):

| Path | ~time |
|---|---|
| `u64` URL-piece parse | ~7 ns |
| Accept negotiation (exact / wildcard / q-values) | ~22 / 59 / 250 ns |
| Dispatch `GET /users/{id}` → JSON (route + extract + handler + render) | ~0.85 µs |
| Dispatch a 404 | ~0.5 µs |
| Build the router tree | ~0.5 µs |

These measure the framework's own overhead (no socket or large payloads); they
are a regression guardrail, not a cross-framework comparison.

Design notes: `docs/DESIGN.md` (committed architecture), `docs/RESEARCH-NOTES.md`
(per-subsystem map of the Haskell reference), `docs/DESIGN-CRITIQUE.md`
(cross-subsystem reconciliation + build sequence). The read-only Haskell
reference lives in `research/servant/` (git-ignored).

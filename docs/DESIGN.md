# servant-rs design

This is the committed architecture for the Rust port of Haskell Servant. It is
derived from the read-only reference at `research/servant/`. Supporting material:
`docs/RESEARCH-NOTES.md` (per-subsystem reference map) and
`docs/DESIGN-CRITIQUE.md` (cross-subsystem reconciliation). Every intentional
deviation from Haskell Servant is called out as **[diff]**.

## 1. North star

One handler-free *API description value* drives four interpretations that never
duplicate route definitions:

- **server** — routing tree + extraction pipeline + hyper/tower adapter,
- **client** — typed request builders over a pluggable transport,
- **links** — type-checked internal link generation,
- **docs** — a documentation model rendered to markdown (OpenAPI-ready).

No god object: the description types live in `servant`; each interpretation is a
separate trait in its own crate/module that shares only those types and the
`HandlerArgs` accumulator.

## 2. The API description

Servant encodes the API purely at the type level (`Proxy`). Rust cannot put
`&'static str` path literals in type position on stable, so we use a **hybrid**:
the *type* encodes structure + extracted argument types; the *value* carries the
runtime strings (path segments, capture/param/header names). **[diff]**

Combinators are generic structs, each holding its runtime config and its
continuation (the thing to its right under Servant's `:>`):

```text
Path<Next>                 segment: String                          (no handler arg)
Capture<A, Next>           name: String                             (arg: A)
CaptureLenient<A, Next>    name: String                             (arg: Result<A, ParseError>)
CaptureAll<A, Next>        name: String                             (arg: Vec<A>)
QueryParam<A, P, S, Next>  name: String   P=Optional S=Strict       (arg per ArgShape)
QueryParams<A, Next>       name: String                             (arg: Vec<A>)
QueryFlag<Next>            name: String                             (arg: bool)
Header<A, P, S, Next>      name: String   P=Optional S=Strict       (arg per ArgShape)
ReqBody<Ct, A, S, Next>    S=Strict (presence is always Required)   (arg: A or Result<A,_>)
Verb<M, STATUS, Ct, A>     method+status (terminal)                 (output: A)
NoContentVerb<M>           terminal, 204                            (output: NoContent)
Raw / RawM                 terminal, opaque                          (handler owns tail)
Description/Summary/OperationId/Fragment   metadata wrappers        (no arg, no routing)
```

Alternatives: `Alt<L, R>` is a left-biased union (`:<|>`). `EmptyApi` serves
nothing. Builder constructor functions nest forward, matching `:>`:

```rust
// "users" :> Capture "id" u64 :> Get '[JSON] User
//   :<|>
// "users" :> ReqBody '[JSON] NewUser :> Verb 'POST 201 '[JSON] User
let api = alt(
    path("users", capture::<u64>("id", get::<Json, User>())),
    path("users", req_body::<Json, NewUser>(verb::<POST, 201, Json, User>())),
);
```

A `:>`-style macro and `#[derive(Api)]` for record routes are **deferred** until
the trait/data model is proven (project rule: no macro-heavy DSL first).

### Sealing

The combinator set is a **closed world**: every combinator type implements a
sealed `Combinator` marker, so interpretation traits (`HasServer`, `HasClient`,
`HasLink`, `HasDocs`) are implemented inside their crates without orphan-rule
fights. Extension points stay **open**: media-type markers, value codecs
(`ToHttpApiData`/`FromHttpApiData`, `MimeRender`/`MimeUnrender`), and doc samples
are ordinary (non-sealed) traits users can implement.

## 3. Handler-signature derivation (the linchpin)

Servant's `ServerT`/`Client`/`MkLink` type families curry one argument per
value-extracting combinator. Rust has no closed type family computing an
arbitrary-arity function type, so we accumulate a heterogeneous list and convert
to a tuple at the edge: **[diff]**

- An internal cons list `HCons<H, T>` / `HNil` (`servant::hlist`).
- `trait HandlerArgs { type Args: HList; }` over the API fragment:
  - `Verb<..>::Args = HNil` (terminal),
  - `Sub-like` combinator `C<.., Next>`: `Args = <C as Lead<Next::Args>>::Out`,
    where `trait Lead<Tail> { type Out: HList; }` prepends 0/1 element:
    - `Path::Out = Tail`, `Capture<A>::Out = HCons<A, Tail>`,
    - `QueryParam<A,P,S>::Out = HCons<<(P,S) as ArgShape<A>>::Out, Tail>`, etc.
- The HList converts to a tuple `(A, B, C)` via an arity macro (0..=16).
- Handlers are `Fn(A, B, C) -> impl Future<Output = Result<Output, ServerError>>`,
  matched by a `Handler<Args, Fut>` blanket impl per arity (the axum tuple
  pattern). The **handler shape is therefore derived from and checked against
  the API type** — the Servant guarantee, preserved.

Arity is capped at 16 (Haskell is unbounded). Documented limitation. **[diff]**

The **same** `HandlerArgs::Args` is consumed by server (handler shape), client
(call argument tuple), and links (builder arity), so the three cannot drift.

## 4. Modifier matrix

`Required`/`Optional` and `Strict`/`Lenient` are zero-sized marker types, used as
type params with defaults `QueryParam<A, Optional, Strict>` / `Header<A, Optional,
Strict>`. `ReqBody<Ct, A, Strict>` fixes presence Required (only Strict/Lenient
vary). One shared trait:

```rust
trait ArgShape<A> { type Out; }
// (Required, Strict)  => A
// (Required, Lenient) => Result<A, ParseError>
// (Optional, Strict)  => Option<A>
// (Optional, Lenient) => Option<Result<A, ParseError>>
```

Capture uses a distinct 2-impl `CaptureShape<A>` (`Strict=>A`, `Lenient=>Result`)
— there is no optional capture. Last-wins fold is realized by **builder methods**
`.required()/.optional()/.strict()/.lenient()` that swap the type param (ordinary
method chaining), not a type-level list fold. **[diff]**

## 5. Server routing & extraction

Router tree mirrors Servant's `Router'` exactly (`servant-server::router`):

```rust
enum Router {
    Static(BTreeMap<String, Router>, Vec<Leaf>), // map of next segment + empty-path leaves
    Capture(Vec<CaptureHint>, Box<Router>),
    CaptureAll(Vec<CaptureHint>, Box<Router>),
    Raw(RawHandler),
    Choice(Box<Router>, Box<Router>),
}
```

`choice` smart-constructor merges static maps (union, recursively) and capture
routers (merge hints) and reorders nested choices — same optimizations as
`Router.hs`. Trailing slash: a single `[""]` segment is treated as empty path.

Run order is the verified `runDelayed` sequence — **captures → method → auth →
accept → content-type → query params → headers → body → handler** — short-circuit
on the first failure. The accept(406)-before-body(400) inversion is intentional
(allows non-backtracking streaming bodies).

```rust
enum RouteResult<T> { Route(T), Fail(ServerError), FailFatal(ServerError) }
```

- `Fail` = recoverable, the `Choice` loop tries the next sibling.
- `FailFatal` = this route commits; stop trying alternatives.
- Per-extractor constructor (fixed data, each tested):
  - **Fail**: capture parse, method mismatch (405), accept unsupported (406),
    content-type unsupported (415), host mismatch.
  - **FailFatal**: missing required query/header, strict param/header parse,
    strict body parse (all 400), BasicAuth (401/403).
- Handler-returned `Err(ServerError)` becomes `Route(error_response)` — never
  Fail/FailFatal.

`run_choice` left-bias + best-error fold uses the hardcoded, table-verified
priority (never sort by numeric status):

```text
priority(404)=0  priority(405)=1  priority(401)=2  priority(415)=3
priority(406)=4  priority(others)=5  priority(400)=6   // higher wins; ties keep left
```

Adapter: a `tower_service::Service<http::Request<Incoming>>` over hyper 1.x
(`call(&self, ..)`), bounded body buffering, HEAD-of-GET handling.

## 6. Content negotiation & codecs

`servant::content`:

- `trait MediaType { fn media_type() -> mime::Mime; fn media_types() -> Vec<Mime> }`
  markers: `Json` (`application/json`), `PlainText` (`text/plain; charset=utf-8`),
  `FormUrlEncoded` (`application/x-www-form-urlencoded`), `OctetStream`
  (`application/octet-stream`).
- `trait MimeRender<A> { fn render(&A) -> Result<Bytes, String> }` and
  `trait MimeUnrender<A> { fn unrender(Bytes) -> Result<A, String> }` per marker
  (see §12 for why render is fallible).
- Content-type *lists* are tuples `(Json,)`, `(Json, PlainText)` implementing
  `AllMimeRender`/`AllMimeUnrender`/`AllMime`.
- **Accept** (response): mirror `mapAcceptMedia` — parse the `Accept` header with
  q-values + wildcards, choose the best match; ties and missing/`*/*` header pick
  the first listed type; no match → 406. Response `Content-Type` is the matched
  type's canonical render.
- **Content-Type** (request): mirror `mapContentMedia` — match the request
  `Content-Type` against the list; no match → 415; absent header → octet-stream
  default *on the server request side only*.
- Client response decode has its **own** octet-stream default — deliberately not
  shared with the server (reconciliation C3).

## 7. Errors

`servant::error::ServerError { status, reason: Option<String>, body: Bytes,
headers: HeaderMap }` with constructors `err300..err505` carrying Servant's exact
reason phrases. `ErrorFormatters { body, url, header, not_found }` hooks
(defaults: parse errors → 400, not-found → 404 "404 Not Found"); `ErrorSource`
enum tags which check failed. Auth/secret material is never logged (security
rule): `ClientRequest`/headers `Debug` redacts `Authorization`, cookies, tokens.

## 8. Links

`servant::link::Link { segments: Vec<Escaped>, query: Vec<Param>, fragment }`,
`Param::{Single(k,v), ArrayElem(k,v), Flag(k)}`, `to_uri`/`to_url_piece` with
RFC3986 escaping (path: percent-encode reserved; `?flag` renders no `=`). A
`HasLink` walk over the description produces a per-endpoint builder whose arity is
`HandlerArgs::Args` restricted to path+query contributors; membership safety via a
sealed `IsElem` derived from the same tree.

## 9. Client

`servant-client`: `ClientRequest`/`ClientResponse`/`ClientError`/`BaseUrl` per the
client research map; `trait RunClient` transport (RPITIT async) with
`run_request(accept_status, req)`; a hyper-backed transport. Each endpoint becomes
an `Endpoint { type Args; type Output }` descriptor with `run(&transport, args)`
(descriptor form, not closures — avoids the currying problem again). Requests are
built incrementally: `append_path_encoded`, `append_query`, `add_header`,
`set_body_bytes(primary_ctype)`, `accept = all ctypes`.

## 10. Docs

`servant-docs`: a value `ApiDoc(Vec<EndpointDoc>)` produced by a `HasDocs` walk;
left-biased non-commutative merge of endpoints sharing a `(path, method)` key;
markdown renderer in a separate module with a fixed section order. `ToSample`
provides example bodies. Golden tests pin output.

## 11. Build sequence

Follows `docs/DESIGN-CRITIQUE.md` §4: (1) `servant` core types + `ArgShape` +
`HandlerArgs` + codecs; (2) errors + links + router enum; (3) `servant-server`
pipeline + extractors + hyper adapter; (4) `servant-client`; (5) `servant-docs`;
(6) cross-interpretation consistency tests + ComprehensiveAPI-subset harness.
Steps 1–2 are the linchpin (server + client both consume them). Docs (5) depends
only on 1–2. Deferred past the first slice: NamedRoutes macro, UVerb/MultiVerb,
Stream/SSE, AuthProtect, WithNamedContext/WithResource/Vault runtime backing.

## 12. Corrections from the adversarial review

An adversarial multi-dimension review (correctness/security/parity/soundness)
produced 17 confirmed findings, all addressed with regression tests:

- **`QueryFlag` is value-sensitive** (Servant `examine`): absent → `false`,
  present-no-value → `true`, present-with-value → `true` only for
  `true`/`1`/empty (so `?flag=false` and `?flag=0` are `false`).
- **A bare query key (`?x`, no `=`) is *absent*** (Servant's `join`), distinct
  from `?x=` (present-empty). Fixes spurious 400s on optional params.
- **`QueryParams` accepts both `name=` and the bracketed `name[]=`** and drops
  valueless entries (`mapMaybe snd`).
- **Accept negotiation is specificity-first**: each server type takes the
  quality of the *most specific* matching range, so `application/json;q=0`
  excludes JSON even when `*/*` also matches. A malformed/out-of-range `q` drops
  that range; a present-but-unparseable `Accept` is 406 (no `*/*` fallback).
- **Request `Content-Type` matching is strict** (no wildcards on the request
  side): `Content-Type: */*` does not match a concrete declared codec.
- **`MimeRender` is fallible** (**[diff]**: `serde` encoders, unlike `aeson`,
  can fail). A failure renders a `500` on the server (rendering only the
  *negotiated* representation, so an unused format's failure can't 500 a
  serviceable request) and a `ClientError::EncodeFailure` on the client — never
  a panic.
- **Security**: the client bounds the buffered response body; `ClientRequest`,
  `ClientResponse`, and `RequestData` `Debug` redact sensitive headers (shared
  `servant::redact`) and print body length only; query/body parse-error bodies
  no longer echo raw values (which may be secrets).

## 13. Extended combinators (second pass)

Built on the same architecture, all tested:

- **Response headers** — `VerbWithHeaders` (a *distinct* terminal type, since
  `Verb<.., Wrapper<A>>` collides with the generic `Verb<.., A>` render impl
  under coherence) whose handler returns `Headers<A>` (runtime value + headers).
- **Union responses** — `UVerb<M, CTypes, Resp>` where `Resp` is `Union2/3/4` of
  `WithStatus<S, T>` arms. `UnionResponse`/`UnionDecode` (in core, needing only
  content negotiation) let the server render the active arm with its status and
  the client decode by matching the response status.
- **Streaming / SSE** — the server response body became
  `UnsyncBoxBody<Bytes, BoxError>` (a streaming body can't be `Sync`).
  `StreamVerb<M, STATUS, Framing, CType, T>`; the handler returns
  `SourceStream<T>`, each item rendered as `CType` and delimited by `Framing`
  (`NoFraming`/`NewlineFraming`/`NetstringFraming`). SSE reuses this with the
  `EventStream` content type and a `ServerEvent` `MimeRender` (so `sse_get` is
  just `StreamVerb<Get, 200, NoFraming, EventStream, ServerEvent>`).
- **Context combinators** — a server `Context` typemap (Servant's `Context`)
  threaded through `serve_with_context`. `BasicAuth<Usr>` (reads
  `Authorization`, runs a `Context`-supplied `BasicAuthCheck`, 401 + a
  `WWW-Authenticate` realm), `WithResource<R>` (per-request `ResourceProvider`),
  and `Vault` (handler receives `Arc<http::Extensions>`). **[diff]** these are
  server-only: their handler argument has no client meaning, so they implement
  no `HasClient` and an API using them can't be passed to `client()`.
- **Named routes** — `alt_all![...]` and `handlers![...]` macros nest
  identically, so the handler tuple always matches the `Alt` tree. (A proc-macro
  record `#[derive]` is **[diff]** deferred; the macros capture the practical
  benefit.)
- **OpenAPI** — `servant-openapi::to_openapi`/`openapi_for` walk the shared
  `ApiDoc` into an OpenAPI 3.0 document. The OpenAPI-specific walk records
  `ToSchema` metadata for typed request and response bodies, emits compatible
  named schemas once under `components.schemas`, and keeps the documented
  type-name fallback for plain docs-model input. Raw and RawM endpoints are
  documented as opaque in Markdown and omitted from the OpenAPI document by
  default because they accept all methods and own an untyped tail.

## 14. Third pass: full combinator + interpretation parity

All previously-deferred items are now implemented and tested:

- **`AuthProtect<Usr>`** — generalized auth via a `Context`-supplied `AuthCheck`
  (`&HeaderMap -> Result<Usr, ServerError>`); server-only.
- **`WithNamedContext<Name>`** — `ExtractState` holds a *stack* of contexts; the
  combinator pushes the parent's `NamedContext<Name>` sub-context for the inner
  walk and pops after, so inner `BasicAuth`/`AuthProtect`/`WithResource` resolve
  against the named scope.
- **`IsSecure`/`HttpVersion`/`RemoteHost`** — server-only combinators reading
  `RequestData.{is_secure, version, remote_addr}`; the adapter fills these from
  the request version and a `ConnectionInfo` extension a serving layer injects
  (the hyper `serve_listener` injects the peer address).
- **Per-arm union headers** — `WithStatusHeaders<S, T>` arm; `UnionResponse`
  now returns headers and `UnionDecode` captures them.
- **Streaming client** — `RunStreamingClient` exposes the response body as a
  chunk stream; `Framing::deframe` + `ClientEndpoint::call_stream` de-frame and
  decode it into a `SourceStream<Result<Item, String>>`. A `StreamInfo` trait
  forwards the terminal's framing/codec/item types through the chain.
- **Derive macros** (`servant-macros`): `#[derive(NamedApi)]` lowers a struct of
  endpoint fields to an `Alt` tree (`into_api`) + a name-keyed `<Name>Handlers`
  record (`into_handlers`); `#[derive(ToSchema)]` produces an OpenAPI object
  schema from a struct's fields (required = non-`Option`).

Still out of scope (niche): a full `components/schemas` registry with recursive
reference de-duplication, browser EventSource adapters, and every TLS deployment
shape. The current scope includes parsed SSE events, `MultiVerb`-style streaming
arms, a small `rustls` HTTP/1 listener adapter, and nested structural
`#[derive(ToSchema)]` output without a shared component registry.

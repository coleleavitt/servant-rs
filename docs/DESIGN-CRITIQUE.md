Both key claims are confirmed against source. The `runDelayed` order is exactly captures→method→auth→accept→content→params→headers→body→server, and `worseHTTPCode` priority is 404=0,405=1,401=2,415=3,406=4,(other)=5,400=6. Now I have enough to deliver the critique.

[
  {
    "section": "1. ComprehensiveAPI combinators NOT covered by any note",
    "comprehensiveAPI_inventory": "Verified against research/servant/servant/src/Servant/Test/ComprehensiveAPI.hs. The type lists: Get '[JSON] NoContent (GET alias), Get '[JSON] Int, Capture' '[Description] , Capture' '[Lenient], Header (default), Header' '[Required,Lenient], HttpVersion, IsSecure, QueryParam (default), QueryParam' '[Required,Lenient], QueryParams, QueryFlag, RemoteHost, ReqBody, ReqBody' '[Lenient], Get '[JSON] (Headers '[Header] NoContent), bare path :> GET, Vault, PostNoContent, Verb 'POST 204 '[JSON] Int, WithNamedContext, CaptureAll, Summary, OperationId, Description, nested :<|> alternative, Fragment, WithResource, EmptyAPI, StreamBody' + Stream (NetstringFraming/JSON/SourceT), Raw.",
    "coverage_verdict": "Every combinator that appears in ComprehensiveAPI is covered by at least one note. Note 1 (combinator surface) is a strict superset of ComprehensiveAPI and explicitly names every member, including the ones ComprehensiveAPI itself omits.",
    "gaps_that_matter": [
      "NO note covers per-combinator behavior for the ComprehensiveAPI member `WithNamedContext` on the CLIENT or DOCS side beyond 'transparent/invisible'. ComprehensiveAPI exercises `\"named-context\" :> WithNamedContext \"foo\" '[] GET`. Notes 4 (client) and 6 (docs) both list it as a no-op pass-through, which is correct, so this is covered — but Note 2 (server/Delayed) never mentions WithNamedContext or the Context-selection mechanism at all, even though it is a server-only combinator and the Delayed note is the natural home for it. This is a real gap in Note 2: the named-context dispatch (which Context subset is visible to inner combinators' auth/resource checks) is undescribed by any note that owns server runtime behavior.",
      "`WithResource` (in ComprehensiveAPI) is named in Note 1 (handler arg `res`) and mentioned in Note 2's gotchas as a params-phase resource allocation, but NO note specifies the *release/drop ordering* relative to the response being sent, or what happens on a FailFatal after allocation. ComprehensiveAPI includes `\"resource\" :> WithResource Int`. The bracket/teardown semantics (Haskell `ResourceT` runs cleanup after the response) are a load-bearing detail left unspecified.",
      "`Vault` (in ComprehensiveAPI) is named only in Note 1. No note describes the Rust analog for the shared per-request middleware storage that Vault reads. Note 1 says 'handler arg Vault' but there is no design for what Rust type backs it (a `http::Extensions` / typemap). Minor but it is a ComprehensiveAPI member with zero design coverage.",
      "ComprehensiveAPI's streaming endpoint uses `StreamBody'` AND `Stream` together with `NetstringFraming`. Note 1 covers both and Note 3 covers the EventStream marker, but framing round-trip *decode* on the server request side (StreamBody' incoming) is only described from the client/encode direction. No note owns server-side incoming stream framing-decode + bounded buffering for `StreamBody'`. (Note 2 explicitly scopes body checks as one-shot but does not address streaming bodies.)"
    ],
    "non_comprehensive_but_unowned": "Combinators outside ComprehensiveAPI but named in Note 1 that have NO interpretation coverage anywhere: UVerb/MultiVerb (Note 1 + Note 4 client cover them; Note 2 server-routing does NOT describe how a union-returning leaf integrates with Delayed/RouteResult; Note 6 docs does NOT describe documenting a UVerb/MultiVerb at all — a docs gap), ServerSentEvents'/EventKind (only Note 1 + the EventStream marker in Note 3; no server, client, or docs interpretation), Host (Note 1 + Note 2 mention 400 recoverable Fail; no docs/client/link coverage — Note 4 client says it adds a Host header, Note 5 link says nothing). DeepQuery is well covered (Notes 1,4,5). These are acceptable for the 'smallest slice' but should be tracked as known interpretation gaps."
  },
  {
    "section": "2. Contradictions between notes",
    "findings": [
      {
        "id": "C1-RESOLVED",
        "topic": "Extraction run order vs error priority",
        "status": "NOT a contradiction — verified consistent against source.",
        "detail": "Note 2 states the fixed runDelayed order as captures, method, auth, accept, content, params, headers, body, server. I confirmed this verbatim in Delayed.hs runDelayed (lines 257-265): capturesD; methodD; authD; acceptD; contentD; paramsD (comment: 'Has to be before body parsing, but after content-type checks'); headersD; bodyD; serverD. Note 2 and Note 3 BOTH independently assert accept(406) runs before body(400) so 406 stays recoverable, and both give the priority table 404<405<401<415<406<...<400. I confirmed worseHTTPCode toPriority in Router.hs: 404=0,405=1,401=2,415=3,406=4,(default)=5,400=6. Notes 2, 5 agree exactly. No contradiction; the run-order-vs-priority 'inversion' (406 checked before 400 but 400 outranks 406) is intentional and all three notes describe it identically."
      },
      {
        "id": "C2",
        "topic": "Capture parse failure: recoverable Fail vs FailFatal",
        "status": "GENUINE inconsistency between Note 2 and Note 5 (and internally within Note 2).",
        "detail": "Note 5 (errors) rules say: 'Capture/QueryParam parse errors that occur AFTER a route is otherwise matched use FailFatal at several sites'. Note 2 (Delayed) rules say capture parse failure '-> Fail (formatted, default 400 ... ) ... so it can backtrack like a 404-class miss' and its gotchas reiterate 'method/accept/content/capture/host failures are recoverable Fail'. These directly conflict on whether a *capture* parse failure is Fail or FailFatal. The reference resolves it in Note 2's favor for captures specifically (addCapture builds capturesD; the capture-parse failure is a recoverable Fail because a sibling route may parse the same segment differently), while *query-param/header* parse failures are FailFatal. Note 5 over-generalizes 'Capture/QueryParam' into one FailFatal bucket. RESOLUTION FOR THE PORT: capture parse failure = recoverable Fail; required-query-param-missing, query-param parse failure (strict), header parse/missing, and strict body parse = FailFatal. This must be encoded per-extractor with a test, exactly as Note 2's own gotcha warns."
      },
      {
        "id": "C3",
        "topic": "Default ports / Content-Type-absent default location",
        "status": "Minor terminology conflict, not behavioral.",
        "detail": "Note 2 says 'Missing Content-Type defaults to application/octet-stream' as an edge case of the Delayed pipeline. Note 3 says the octet-stream substitution 'lives in the SERVER (getAcceptHeader / ctCheck), not in ContentTypes.hs'. Note 4 (client decode) ALSO defaults a missing *response* Content-Type to application/octet-stream in `decodedAs`. All three are individually correct (server request side and client response side both default to octet-stream), but a reader could wrongly conclude there is one shared default site. There are TWO independent octet-stream defaults (server-side request decode, client-side response decode) plus the Accept→*/* default (server response). The port must implement them as separate, deliberately-duplicated defaults, not a single shared helper, or it will leak the client default into the server."
      },
      {
        "id": "C4",
        "topic": "QueryParam duplicate-key behavior (first vs all)",
        "status": "Underspecified across notes — latent inconsistency.",
        "detail": "Note 1 edge_cases says 'Duplicate query keys for a scalar QueryParam (first vs all?) — Servant's QueryParam takes the matching value; define and test the chosen behavior' (explicitly open). Note 2 edge_cases asserts as fact 'QueryParam takes the FIRST lookup'. These are not contradictory in outcome but Note 1 leaves it open while Note 2 commits to 'first'. The port should fix it as 'first' (matching Note 2 / Haskell `lookup`) and Note 1's open question should be closed to match."
      },
      {
        "id": "C5",
        "topic": "ReqBody Lenient + empty body interaction",
        "status": "Tension between Note 1 and Note 2/3, resolvable.",
        "detail": "Note 1 says 'ReqBody is always Required so empty body still 400s' even under Lenient. Note 2 and Note 3 describe the `noOptionalReqBody` special case where Optional ReqBody + no Content-Type + length 0 succeeds. The conflict is only apparent: ReqBody (the public alias) is always Required, so the noOptionalReqBody branch only fires for the *internal* not-Required path, which the public combinator never exposes. But Note 1 also lists no Optional ReqBody as a gotcha while Notes 2/3 spend an edge case on Optional ReqBody. The port must decide: either expose no Optional ReqBody (Note 1's stance, simpler, drop the noOptionalReqBody branch) OR keep the internal optional path for completeness. Recommend Note 1's stance for the first slice; document the dropped branch."
      },
      {
        "id": "C6",
        "topic": "Fragment merge bias (docs) vs Fragment set-last-wins (links)",
        "status": "Consistent but easy to misread as conflicting.",
        "detail": "Note 5 (links) says Fragment is SET, last-write-wins within a single link fold. Note 6 (docs) says combineFragment keeps the FIRST when MERGING two alternatives' Actions. These are different operations (single-fold overwrite vs cross-alternative merge) and are both correct; flagging because a careless port could implement one merge rule for both."
      }
    ]
  },
  {
    "section": "3. Hardest Rust design problems + concrete recommended approach",
    "problems": [
      {
        "rank": 1,
        "problem": "Type-level handler-signature derivation: turning a right-nested combinator type into a curried/positional handler `A -> B -> ... -> m R` (Haskell `ServerT`/`Client`/`MkLink` associated type families). Rust has no closed type-family that computes an arbitrary-arity function type, and the same API description must yield a server handler shape, a client call shape, and a link-builder arity — all three derived from one type.",
        "recommended_approach": "Do NOT reproduce currying. Use a typed heterogeneous accumulator with a recursive trait that builds a tuple, then hand the tuple to an `async fn(Args) -> Result<R, E>` adapter. Concretely: each extracting combinator implements `trait Extract { type Out; }` and the API fold implements `trait HandlerShape { type Args; type Output; }` where `Args` is grown by tuple-prepend at each `Sub<L,R>` step. Represent the handler as `Handler<Args, Fut>` where the user writes `|a: A, b: B, c: C| async move {...}` and a generated/blanket `impl` for `FnMut(A,B,C) -> Fut` (arity 0..=16 via a macro, the standard axum/`Tuple` pattern) destructures `Args=(A,B,C)` and calls it. This gives positional currying ergonomics without type-level function construction. For the THREE interpretations sharing one description: define the API as a value-carrying combinator tree of marker/struct types (`Path`, `Capture<T>`, `QueryParam<T,P,S>`, `Verb<M,STATUS,Ct,A>`, `Alt<L,R>`) and implement THREE separate traits over the SAME types — `HasServer` (yields the extractor pipeline + `Args`), `HasClient<C>` (yields an `Endpoint{type Args; type Output}` descriptor, Design B from Note 4 — NOT closures), `HasLink` (yields a builder, Design A from Note 5). The `Args` tuple type is computed ONCE by a shared `CombinatorArgs` trait and reused by server and client so they cannot drift. This is the single most important architectural decision; everything else composes on top of `CombinatorArgs`.",
        "why_hard": "Rust cannot put `&str` path literals in type position cleanly (const-generic `&'static str` is limited), so path segments must be carried value-level in a builder while extraction args stay type-level — the hybrid is the crux. The tuple-builder macro caps arity (~16) where Haskell is unbounded; document the cap."
      },
      {
        "rank": 2,
        "problem": "The 4-way modifier ArgShape matrix (Required×Strict/Lenient) for QueryParam/Header vs the distinct 2-way matrix for Capture, with LAST-WINS fold semantics — and keeping it a single shared helper so the four interpretations (server extract, client arg, link arg, docs) don't reimplement it inconsistently (Note 2 gotcha explicitly warns of drift).",
        "recommended_approach": "Encode presence and parsing as marker type params with defaults: `QueryParam<A, P = Optional, S = Strict>`, `Header<A, P = Optional, S = Strict>`, `ReqBody<A, S = Strict>` (P fixed Required, not a param), `Capture<A>` and a distinct `CaptureLenient<A>` (no Optional capture exists). Define ONE trait `trait ArgShape<A> { type Out; }` implemented for the 4 `(P,S)` combinations: `(Required,Strict)=>A`, `(Required,Lenient)=>Result<A,ParseError>`, `(Optional,Strict)=>Option<A>`, `(Optional,Lenient)=>Option<Result<A,ParseError>>`. Capture uses a SEPARATE `trait CaptureShape<A>` with only 2 impls (`Strict=>A`, `Lenient=>Result<A,ParseError>`) because Capture has no Option form. Crucially: do NOT model modifiers as an HList that you fold with last-wins at the type level (Rust can't fold type-level lists ergonomically) — instead make the *builder API* enforce last-wins by having `.optional()`/`.required()`/`.lenient()`/`.strict()` methods replace the type param, so `qp().required().optional()` resolves to Optional by ordinary method chaining, naturally giving last-wins without a type-level fold. The `Out` type then flows into the `CombinatorArgs` tuple from problem #1, shared by all interpretations."
      },
      {
        "rank": 3,
        "problem": "The Fail vs FailFatal recover-vs-commit distinction combined with the fixed-phase pipeline and the non-numeric worseHTTPCode priority (404=0,405=1,401=2,415=3,406=4,other=5,400=6) — getting per-combinator constructor choice right and not letting `?`/early-return semantics escape the choice loop incorrectly (Note 2 gotcha).",
        "recommended_approach": "Model `enum RouteResult<T>{Route(T),Fail(ServerError),FailFatal(ServerError)}` and DELIBERATELY do not implement `Try`/`From` that conflates the two failure kinds. Build the pipeline as a fixed-slot struct with `Vec<BoxCheck>` per `Phase` (derive `Ord` on `Phase` so iteration order is the canonical order, verified against runDelayed). `run_delayed` iterates phases in `Phase` order, short-circuiting on the FIRST Fail OR FailFatal (mirrors RouteResultT bind). The Fail/FailFatal distinction ONLY matters at the `run_choice` boundary: `run_choice` returns immediately on `Route` or `FailFatal`, and folds multiple `Fail`s with `fn priority(u16)->u8{match c{404=>0,405=>1,401=>2,415=>3,406=>4,400=>6,_=>5}}` keeping higher priority, ties keep left (left-biased). Hardcode this exact table (verified in Router.hs) — never sort by status value. Per-combinator constructor assignment (the actually-hard part) is fixed data, encode it as a constant on each extractor and TEST each: capture-parse=Fail, method=Fail, accept=Fail, content-type-unsupported=Fail, host-mismatch=Fail; required-query/header-missing=FailFatal, strict-param/header-parse=FailFatal, strict-body-parse=FailFatal, ALL BasicAuth(401/403)=FailFatal. (This resolves contradiction C2 by making capture a recoverable Fail.) Handler-thrown errors become `Route(err.into_response())`, never Fail/FailFatal."
      },
      {
        "rank": 4,
        "problem": "Replacing Haskell's overlapping/OVERLAPPABLE/OVERLAPPING instance resolution that selects the specialized Verb/MimeRender/HasClient impl by return type (NoContent vs Headers<H,A> vs Union vs plain A), plus the cross-interpretation coherence/orphan problem of implementing many interpretation traits per combinator over a closed set.",
        "recommended_approach": "Rust has no instance specialization (it is unstable), so dispatch on the RETURN TYPE explicitly via distinct marker/wrapper types rather than overlapping blanket impls: `NoContent` (unit struct, blanket `MimeRender<C> for NoContent` returning empty bytes for all C — Rust has no overlap problem here, the OVERLAPPING trick in Haskell is unnecessary), `Headers<H, A>` (wrapper), `Union`/a user `enum` for UVerb, `RawResponse` for Raw. The `Verb`'s response handling becomes a trait `ResponseBody { fn decode/encode }` with one impl per wrapper type — no overlap because the types are disjoint. For the COHERENCE problem: seal the combinator set (`mod sealed { pub trait Sealed {} }`) so the combinator types are a closed world owned by the `servant` crate; this lets the interpretation traits (`HasServer`/`HasClient`/`HasLink`/`HasDocs`) be implemented for every combinator inside the crate without orphan-rule fights, while still letting USERS add new content-type markers and new value types (the open extension points) since those are governed by separate non-sealed traits (`MediaTypeMarker`, `ToSample`, `ToHttpApiData`). Document explicitly: combinator set is closed (sealed), codecs/value-types are open."
      },
      {
        "rank": 5,
        "problem": "Keeping one API description authoritative across server routing, client endpoints, links, and docs so route definitions are never duplicated (the CLAUDE.md central mandate), given the three interpretations have structurally different shapes (extractor pipeline vs Endpoint descriptor vs builder vs value-tree walker).",
        "recommended_approach": "Make the API description a value-level combinator tree (the SAME tree all four interpretations traverse), parameterized by the type-level combinator structs. Each interpretation is a trait implemented over the tree: `IntoRouter` (server, builds the Phase pipeline + inspectable `Router` enum), `IntoEndpoints` (client, Design B descriptors), `IntoLinks` (Design A builders keyed per endpoint), `IntoDocs` (value walker producing `ApiDoc`). All four consume the shared `CombinatorArgs::Args` (problem #1) so the handler/arg ordering is computed once. For docs/links membership safety, derive an `IsElem` sealed marker per endpoint from the SAME tree registration so `safe_link` only compiles for real endpoints. Crucially keep the four interpretations as SEPARATE traits/modules (no god `Api` object — per code-style rules) that share only the description types and `CombinatorArgs`. NamedRoutes (record-of-routes) becomes a `#[derive(Api)]` macro DEFERRED until the trait/data model is proven (per project macro rules); it lowers a struct's fields to a right-nested `Alt` tree in declaration order (field order == precedence, load-bearing)."
      }
    ]
  },
  {
    "section": "4. Recommended dependency-ordered build sequence for the smallest end-to-end slice",
    "guiding_principle": "Each step must be integration-testable against the prior step; do not add a combinator to one interpretation without adding it to all interpretations that the slice touches. Target slice API (subset of ComprehensiveAPI, enough to exercise path, capture, query, body, response, alternatives): `\"users\" :> Capture<u64> :> Get<(Json,), User>` :<|> `\"users\" :> ReqBody<Json, NewUser> :> Verb<POST,201,(Json,),User>`.",
    "sequence": [
      {
        "step": 0,
        "crate_or_module": "workspace scaffold",
        "deliverable": "Cargo workspace with empty `servant`, `servant-server`, `servant-client`, `servant-docs` crates; depend on http, bytes, mime, serde, serde_json, serde_urlencoded, percent-encoding, futures, tower, hyper. No tower/hyper wiring yet.",
        "depends_on": []
      },
      {
        "step": 1,
        "crate_or_module": "servant::api (combinator types) + servant::codec + servant::http_data",
        "deliverable": "Sealed combinator marker types (Path, Sub, Alt, EmptyApi, Capture<A>, CaptureLenient<A>, QueryParam<A,P,S>, Header<A,P,S>, ReqBody<A,S>, Verb<M,STATUS,Ct,A>, NoContentVerb<M>, Headers<H,A>, plus metadata structs). The `ArgShape`/`CaptureShape` matrices (problem #2). `CombinatorArgs` trait computing the `Args` tuple (problem #1). MediaTypeMarker + MimeRender/MimeUnrender + Json/PlainText/FormUrlEncoded/OctetStream markers + ContentTypes-for-tuples (Note 3). ToHttpApiData (to_url_piece/to_query_param/to_header). NoContent unit + blanket MimeRender.",
        "depends_on": [0],
        "tests": "Property tests for ArgShape matrix (all 4 states + last-wins via builder methods); content-type negotiation unit tests (matchAccept quality+specificity+left-bias; matchContent params-subset); round-trip MimeRender/MimeUnrender for Json; percent-encoding of path/query per RFC3986 unreserved set."
      },
      {
        "step": 2,
        "crate_or_module": "servant::error + servant::link + servant::routing (inspectable Router enum + RouteResult)",
        "deliverable": "ServerError struct (status+reason override+body+headers) with err300..err505 constructors (exact reason phrases). RouteResult<T> enum + priority() table (problem #3, hardcoded, verified). Link value model + Escaped/QueryParam variants + ArrayElementStyle + to_uri/to_url_piece. ErrorFormatters struct with Default (body/url/header=>400, not_found=>404 '404 Not Found') + ErrorSource enum. The Router enum (Static/Capture/CaptureAll/Choice/Leaf/Raw) as the shared inspectable description target.",
        "depends_on": [1],
        "tests": "worseHTTPCode/priority golden cases (400 beats all; ties keep left); link escaping golden tests (foo/bar=>foo%2Fbar, test@example.com=>%40); err* reason-phrase byte-exactness; flag renders no '='."
      },
      {
        "step": 3,
        "crate_or_module": "servant-server: Delayed pipeline + extractors + IntoRouter + tower/hyper adapter",
        "deliverable": "Phase enum (ordered, verified == runDelayed), Delayed struct with per-phase Vec<BoxCheck>, run_delayed (short-circuit on Fail/FailFatal), run_choice (left-bias + priority fold). Extractor impls for each combinator in the slice assigning the correct Fail/FailFatal constructor (problem #3). Handler tuple-call adapter (problem #1, arity macro). HEAD-of-GET handling. Context holding ErrorFormatters with default fallback. tower::Service<Request<Body>> edge adapter over hyper; bounded body buffering.",
        "depends_on": [2],
        "tests": "Integration: serve the slice API, assert 200/201, capture parse failure=Fail+400, missing required query=FailFatal+400, wrong method=405, unsupported Accept=406, unsupported Content-Type=415, overlapping-route left-bias, best-error selection (405 vs 415 => 415), trailing slash, percent-encoded captures."
      },
      {
        "step": 4,
        "crate_or_module": "servant-client: ClientRequest/ClientResponse + RunClient trait + Endpoint descriptors (IntoEndpoints) + reqwest/hyper transport",
        "deliverable": "ClientRequest (ordered query Vec, HeaderMap append, redacted Debug for Authorization), ClientResponse, ClientError enum, BaseUrl, RunClient async trait (Option<&[StatusCode]> accept-status). Endpoint{type Args;type Output} descriptors (Design B) + `call()` helper. decode helper mirroring decodedAs (content-type presence/parse => InvalidContentTypeHeader; match => UnsupportedContentType; unrender => DecodeFailure). Per-combinator request building (append_path_encoded precondition, append_query, set_body_bytes with PRIMARY ctype, accept=ALL ctypes).",
        "depends_on": [3],
        "tests": "Round-trip: spin up the step-3 server on a local port, generate typed client from the SAME API description, call both endpoints, assert decoded values match; status-mismatch (201 endpoint hit expecting 200) => FailureResponse not DecodeFailure; missing response Content-Type defaults octet-stream; query order/dup preserved."
      },
      {
        "step": 5,
        "crate_or_module": "servant-docs: ApiDoc value model + IntoDocs walker + markdown renderer + ToSample",
        "deliverable": "Endpoint/EndpointDoc/ResponseDoc/ApiDoc (IndexMap), non-commutative left-biased combine methods, IntoDocs walker over the shared tree (path=>:sym, capture=>DocCapture, params, headers, fragment set, reqbody samples, notes, auth, terminal=>single). ToSample/ToParam/ToCapture traits. markdown()/markdown_with() in a SEPARATE module with the fixed section order (intros, sorted endpoints by Endpoint Ord, then Notes/Auth/Captures/Headers/Params/Fragment/Request/Response/curl).",
        "depends_on": [2, 1],
        "tests": "Golden Markdown for the slice API (pin Endpoint sort order path-then-method); merge of two alternatives sharing a (path,method) key concatenates lists + left status; capture renders :sym; max_samples truncation."
      },
      {
        "step": 6,
        "crate_or_module": "consistency tests (cross-interpretation)",
        "deliverable": "A test crate / integration module asserting the ONE description drives all four: (a) every endpoint in the docs ApiDoc has a matching route in the server Router and a matching client Endpoint; (b) a safe_link for each endpoint produces a path the server actually routes to (round-trip link->request->200); (c) client-generated request against server yields the response the docs sample type describes. This is the proof of the central mandate and the equivalent of a mini-ComprehensiveAPI consistency harness.",
        "depends_on": [3, 4, 5],
        "tests": "Property test: for the slice (and as combinators are added, for ComprehensiveAPI subset), the set of (path,method) keys is identical across Router, Endpoints, and ApiDoc; safe_link output parses+routes; no route defined in more than one place."
      }
    ],
    "critical_ordering_notes": [
      "Step 1's CombinatorArgs/ArgShape is the linchpin (problem #1+#2) — server (3) and client (4) BOTH consume it, so it must stabilize first or the two interpretations drift.",
      "Steps 5 (docs) depends only on steps 1-2, NOT on 3/4, so docs can be built in parallel with server/client once the description+error+link layer exists — but step 6 (consistency) is the gate that proves they share one description and must come last.",
      "Defer until after the slice is proven (per project macro/scope rules): NamedRoutes #[derive(Api)] macro, UVerb/MultiVerb, Stream/SSE server+docs, AuthProtect/BasicAuth Context resolution, WithNamedContext/WithResource/Vault runtime backing, hoistClientMonad (drop entirely — parameterize client over RunClient C instead, per Note 4 gotcha)."
    ]
  }
]
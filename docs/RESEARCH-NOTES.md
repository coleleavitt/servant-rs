# Servant -> Rust research notes (auto-generated from understand workflow)

## Servant API combinator surface (servant/src/Servant/API and Servant/API/*)

**Summary:** Servant's API is a type-level eDSL: an API type is built from combinators connected by `:>` (sub/path) and `:<|>` (left-biased alternative). Each combinator is an empty/phantom data type that carries no runtime data; its only job is to drive *interpretations* (server routing, client generation, links, docs) via type classes (HasServer, HasClient, HasLink, etc.). Combinators that extract from a request add an argument to the handler function; structural and metadata combinators do not. Argument shape is governed by a type-level modifier list folded by FoldRequired/FoldLenient/FoldDescription. The Rust port must reproduce the *developer-facing guarantees* (one description drives every interpretation, extraction wrapping by Required/Optional + Strict/Lenient, left-biased route precedence, content negotiation) without copying the type-family machinery.

### key_types
- `:>` (Sub, infixr 4) — path/combinator chaining; `path :> rest` means rest is reached under that combinator. LHS can be a type-level Symbol (literal path segment) or any combinator type.
- `:<|>` (Alternative, infixr 3) — union of two APIs; FIRST (left) takes precedence on overlap. Server value is `handlerA :<|> handlerB`; it is a real runtime product `data a :<|> b = a :<|> b`.
- `EmptyAPI` — serves nothing; the morally-unit of `:<|>`. Real value `EmptyAPI`.
- Modifiers: `Required` / `Optional` (presence), `Strict` / `Lenient` (parse-failure handling). Type-level lists `'[...]` attached to QueryParam'/Header'/ReqBody'/Capture'.
- `FoldRequired mods = FoldRequired' 'False mods` — folds left-to-right, LAST Required/Optional wins; default False (Optional). `FoldLenient` — default False (Strict). `FoldDescription` — last Description wins, default "".
- `RequiredArgument mods a = If (FoldRequired mods) a (Maybe a)` — used by Capture (no lenient wrapping).
- `RequestArgument mods a` — Required+Strict↦`a`; Required+Lenient↦`Either Text a`; Optional+Strict↦`Maybe a`; Optional+Lenient↦`Maybe (Either Text a)`. Used by QueryParam/Header.
- `Capture' mods sym a` (Capture = Capture' '[]); `CaptureAll sym a` — single vs all-remaining path segments.
- `QueryParam' mods sym a` (QueryParam = '[Optional,Strict]); `QueryParams sym a` (→ [a]); `QueryFlag sym` (→ Bool); `QueryString` (→ raw Query); `DeepQuery sym a` (→ FromDeepQuery a, nested filter[a][b]=c).
- `Header' mods sym a` (Header = '[Optional,Strict]) — request header extraction.
- `ReqBody' mods ctypes a` (ReqBody = '[Required,Strict]) — note ReqBody is ALWAYS Required regardless; only Strict/Lenient vary.
- `Verb method statusCode ctypes a` — the workhorse endpoint. `Get='Verb 'GET 200`, Post/Put/Delete/Patch + dozens of status synonyms (PostCreated=201, GetAccepted=202, *NonAuthoritative=203, etc.).
- `NoContentVerb method` — body-less 204 endpoint; handler returns `NoContent`. GetNoContent/PostNoContent/... = NoContentVerb 'GET etc. (no ctype list, no status param).
- `Headers ls a` / `ResponseHeader sym a` (Header | MissingHeader | UndecodableHeader bs) / `AddHeader` / `addHeader`/`noHeader` — response headers wrapping the response value.
- `Raw` — plug a raw WAI Application (handler is `Tagged Application`); `RawM` — same with monadic context.
- `Description sym`, `Summary sym`, `OperationId sym` — metadata only; no handler arg, no routing effect.
- `Fragment a` — documents URI fragment (for Links); contributes nothing to server handler.
- `Vault` — access request Vault (shared middleware storage) → handler arg `Vault`.
- `IsSecure` → handler arg `IsSecure` (Secure | NotSecure).
- `HttpVersion` → handler arg `HttpVersion`.
- `RemoteHost` → handler arg `SockAddr`.
- `Host sym` — match against the request `Host` header (multi-domain routing); no handler arg.
- `BasicAuth realm userData` → handler arg `usr` (the authenticated user type, resolved via context check); `BasicAuthData{username,password}`.
- `AuthProtect tag` (experimental) — generalized auth; handler arg type determined by a context-provided AuthHandler.
- `NamedRoutes api` — embed a record-of-routes (`api` is a `* -> *` record); `Generic`/`GenericMode (:-)`/`ToServantApi`/`toServant`/`fromServant` convert record ⇆ `:<|>` tree.
- `WithNamedContext name subContext subApi` — server-only; selects a named tagged context for combinators in subApi.
- `WithResource res` → handler arg `res`; a per-request managed/bracketed resource.
- `UVerb method ctypes '[as]` — open union response; handler returns `Union (ResponseTypes)`; `WithStatus k a`, `HasStatus`/`StatusOf`, `inject`, `IsMember`.
- `MultiVerb method reqCtypes '[responses] result` — richer union: `Respond s desc a`, `RespondAs ct s desc a`, `RespondEmpty s desc`, `RespondStreaming s desc framing ct`, `WithHeaders hs ret resp`, `AsUnion`/`AsHeaders`/`AsConstructor`, `UnrenderResult` (StatusMismatch|UnrenderError|UnrenderSuccess).
- `Stream method status framing ctype a` (StreamGet/StreamPost=200); `StreamBody' mods framing ctype a` (StreamBody='[]); framing strategies `NoFraming`/`NewlineFraming`/`NetstringFraming`; `SourceIO=SourceT IO`, `ToSourceIO`/`FromSourceIO`.
- `ServerSentEvents' method status kind a` (ServerSentEvents = 'GET 200); `EventKind = RawEvent | JsonEvent`.
- Content types: `JSON`, `PlainText`, `FormUrlEncoded`, `OctetStream`, `EventStream`; classes `Accept`(contentType), `MimeRender`, `MimeUnrender`, `AllCTRender`/`AllCTUnrender` for ctype lists; `NoContent`.

### rules
- Modifier fold is LEFT-TO-RIGHT with LAST-WINS: `FoldRequired' acc (Required:ms)` recurses with acc='True; Optional sets 'False; any other mod leaves acc untouched. So `'[Required, Optional]` => Optional, `'[Optional, Required]` => Required. Same for FoldLenient (Lenient/Strict) and FoldDescription (last Description string). Rust must NOT use first-wins or set-membership.
- Defaults: QueryParam and Header default to `'[Optional, Strict]`. ReqBody defaults to `'[Required, Strict]` and is ALWAYS Required (the Required modifier is fixed; only Strict↔Lenient is meaningful). Capture defaults to `'[]` (=> Required, Strict; Capture has no Optional notion — a path segment is either there for the route to match or the route doesn't match).
- Argument wrapping for QueryParam/Header (RequestArgument): Required+Strict ↦ `a`; Required+Lenient ↦ `Either Text a`; Optional+Strict ↦ `Maybe a`; Optional+Lenient ↦ `Maybe (Either Text a)`. Lenient means parse errors are surfaced as `Left text` instead of failing the request; Strict means a parse failure rejects the request (400). Required means absence rejects the request; Optional means absence yields Nothing.
- Capture/CaptureAll use RequiredArgument (NOT RequestArgument): Required ↦ `a`, and the only modifier that wraps is via `Capture' '[Lenient]` producing `Either Text a` in the comprehensive API. There is no Optional capture; a missing/failed-strict capture causes the route to not match / 400.
- QueryFlag is always `Bool`, never wrapped; absent or `?flag` or `?flag=true`/`?flag=1` => True, otherwise False.
- QueryParams yields `[a]` (never Maybe/Either); collects all matching keys; supports both `k=v1&k=v2` and `k[]=v1&k[]=v2`.
- `:<|>` is LEFT-BIASED and infixr 3; `:>` is infixr 4 (binds tighter). Route resolution tries the left alternative first; on overlap the left handler wins. The Rust router must preserve this ordering and best-error selection semantics.
- Each request-reading combinator contributes exactly one argument to the handler, in the SAME left-to-right order the combinators appear in the type. Structural (`:>` path literal), metadata (Description/Summary/OperationId/Fragment), and Host contribute NO handler argument. Verb determines the return type. Example: `"x" :> Capture "id" Int :> QueryParam "q" Text :> Header "h" Int :> ReqBody '[JSON] Body :> Post '[JSON] Res` => handler `Int -> Maybe Text -> Maybe Int -> Body -> m Res`.
- Path literals are type-level Symbols on the LHS of `:>`; they match exact, percent-decoded path segments and contribute no argument.
- Verb's statusCode (a type-level Nat) is the SUCCESS status set on the response (200 default, plus the documented synonyms 201/202/203/205/206...). NoContentVerb is fixed 204 with empty body. UVerb/MultiVerb statuses are per-response-alternative via HasStatus/StatusOf / Respond's Nat.
- Response headers (`Headers ls a`): the response value is wrapped; `addHeader` PREPENDS to the existing header list (order matters for the type-level list and for output order). `ResponseHeader` distinguishes Header a / MissingHeader / UndecodableHeader bs; MissingHeader emits no header line.
- Content negotiation: ReqBody chooses a decoder by matching the request `Content-Type` against the combinator's ctype list (AllCTUnrender); Verb chooses an encoder by matching the request `Accept` header against the ctype list (AllCTRender), defaulting to the first listed ctype when Accept is absent. Unsupported Accept => 406; unsupported request Content-Type => 415; body parse failure => 400.
- UVerb/MultiVerb response parsing (client side) uses UnrenderResult ordering: StatusMismatch (status didn't match this alternative) is distinct from UnrenderError (status matched, body failed) and UnrenderSuccess; MonadPlus picks the first matching/most-informative alternative. The Rust port must preserve: try alternatives, prefer status match, surface body errors only when status matched.
- Streaming framing: NoFraming passes chunks through (best for binary/OctetStream); NewlineFraming appends '\n' per frame and splits on newline (assumes payloads contain none, e.g. JSON); NetstringFraming encodes `len:payload,`. The framing strategy and content type are independent type parameters.
- NamedRoutes/Generic: a record `Routes mode` with fields `mode :- <endpoint>` is isomorphic (via GHC.Generics product) to a right-nested `:<|>` tree, in field declaration order. `genericApi` produces the Proxy; `toServant`/`fromServant` convert handler records ⇆ the `:<|>` value. Field order == alternative order == precedence.
- WithNamedContext and BasicAuth/AuthProtect are server-context dependent: the auth result type and the named context are supplied out-of-band (Context), not from the wire. Auth failures must be modeled distinctly from parse failures (401/403 vs 400).

### edge_cases
- Modifier last-wins ordering: `'[Required, Optional]` must resolve Optional and `'[Optional, Required]` Required; test both orders and empty list (=> Optional, Strict). Duplicate/conflicting modifiers must not error.
- QueryParam Optional+Lenient produces `Maybe (Either Text a)`: absent => Nothing, present+parse-ok => Just (Right v), present+parse-fail => Just (Left err). Test all four states; ensure Lenient never 400s while Strict does.
- QueryFlag truthiness: `?flag` (no value), `?flag=true`, `?flag=1` => True; `?flag=false`, `?flag=0`, `?flag=anything-else` => False; absent => False. Test each literal and casing.
- QueryParams collects multiple values and supports both `k=v1&k=v2` and `k[]=v1&k[]=v2`; empty => []. DeepQuery nested `filter[a][b]=c` and bare `filter=a` (=> ([], Just a)); malformed brackets.
- Capture parse failure under Strict vs Lenient: Strict failing capture should make the route fail-to-match/400; Lenient yields `Either Text a`. CaptureAll with zero remaining segments => empty list; with trailing slash; with percent-encoded segments.
- Trailing slash handling and empty path segments (`/a//b`) must be tested for both static path match and CaptureAll.
- Percent-encoding/UTF-8 in path segments, query keys/values, and header values — decode intentionally; test reserved chars and malformed (`%zz`) encodings.
- Duplicate query keys for a scalar QueryParam (first vs all?) — Servant's QueryParam takes the matching value; define and test the chosen behavior.
- Content negotiation edge cases: missing Accept (=> first ctype), `Accept: */*`, `Accept: application/*`, unsupported (=> 406), malformed Accept header; request Content-Type missing/unsupported (=> 415); empty body for Required ReqBody (=> 400) vs Optional.
- ReqBody Lenient: body present but unparseable should still call handler with `Left err` (does not 400); ReqBody is always Required so empty body still 400s.
- Response headers: addHeader PREPEND order vs output order; MissingHeader emits nothing; UndecodableHeader passes raw bytes through; duplicate header names.
- Verb status code: 204 NoContentVerb must emit empty body even if handler type would otherwise serialize; test that body is truly absent. Non-200 success synonyms (201,202,203,205,206) set the right status.
- UVerb/MultiVerb: two responses with the SAME status code but different bodies (client must try body parse, not just status); WithStatus overriding NoContent's default 204; status-mismatch vs body-error precedence.
- MultiVerb AsUnion conventions: Bool pair (False => first/failure response), Maybe pair (Nothing => first response) — order-sensitivity; reordering union members is a compile error in Haskell, must be a clear error in Rust.
- Streaming framing round-trips: NewlineFraming assumes no embedded newlines (JSON) — test payload containing newline breaks framing; NetstringFraming length prefix off-by-one and empty string `0:,`; NoFraming with binary chunks.
- Raw route path stripping: when `:>`-prefixed, the inner Application sees a stripped pathInfo; Raw must NOT bypass path-traversal protection (security rule). Host combinator matching wrong/missing Host header.
- Alternative left-bias with overlapping routes (same path, different method, or identical) — left handler wins; best-error selection when no branch matches (404 vs 405 vs 406/415 vs 400).

### gotchas
- Combinators are EMPTY phantom types in Haskell (no runtime data); the entire behavior is in type-class instances (HasServer/HasClient/HasLink/HasDocs). Rust has no such open type-class dispatch, so the 'one description, many interpretations' guarantee must be re-created with traits implemented per combinator across multiple interpretation traits — expect a coherence/orphan challenge; use sealed traits and a closed combinator set.
- FoldRequired/FoldLenient/FoldDescription are LAST-WINS folds, not first-wins and not boolean-OR. A naive Rust `contains(Required)` over a list would be wrong for `'[Required, Optional]`.
- ReqBody' is documented as ALWAYS Required even though it takes a mods list — the Required modifier is fixed; only Strict/Lenient is meaningful. Don't expose an Optional ReqBody.
- Capture uses `RequiredArgument` (only Required/Optional fold, wrapping in Maybe) while QueryParam/Header use `RequestArgument` (full 4-way Required×Lenient matrix). They are DIFFERENT helper type families — Capture in practice only varies Strict/Lenient (via Capture' '[Lenient]) and is never Optional. Two distinct ArgShape rules in Rust.
- Modifier order in the handler argument list mirrors COMBINATOR order in the type, left-to-right. The whole point is positional currying; Rust must assemble the argument tuple in exactly that order. Easy to get backwards when folding right-nested `:>`.
- `:>` LHS being a type-level Symbol (string literal path) vs a combinator type is the same syntactic position in Haskell. In Rust, distinguish 'literal path segment' from 'combinator' explicitly (e.g. a `Path` wrapper) because Rust can't put a `&str` where a type is expected without const generics, and const-generic `&'static str` support is limited — likely carry segments in a value-level builder rather than purely type-level.
- `Headers ls a` and `addHeader` PREPEND to the type-level header list; the head of the list is the most-recently-added header. Output ordering and the type accumulation both depend on this; don't reverse.
- UVerb/MultiVerb response selection depends on `UnrenderResult`'s MonadPlus: StatusMismatch is the 'empty'/skip case, UnrenderError is preserved-but-overridable, UnrenderSuccess short-circuits. This is NOT simple try-each-and-take-first-Ok; status-mismatched alternatives are skipped before body errors are considered.
- NoContentVerb has NO content-type list and NO status parameter (always 204) — it is a separate type from Verb, not `Verb _ 204 _ NoContent`. Two distinct Rust types.
- WithNamedContext, BasicAuth, AuthProtect, WithResource are SERVER-ONLY / context-dependent: their handler argument types are supplied by the server Context (out of band), not parsed from the wire. The API type names a tag/realm; the actual handler/user type is resolved at server-construction time. This breaks the 'argument type is fully determined by the API type' assumption and needs a Context abstraction in servant-server.
- MultiVerb's `Respond` content type is chosen dynamically by Accept (from MultiVerb's ctype list), whereas `RespondAs` HARDCODES the response content type independent of Accept — two different negotiation paths in one combinator family.
- Generic/NamedRoutes relies on GHC.Generics to flatten a record into a right-nested `:<|>` tree in field-declaration order; Rust will need a derive macro (later, per project rules) to do the same, and the field order == route precedence is load-bearing.
- `Verb` statusCode and `Stream`/`ServerSentEvents'`/`UVerb` statuses are type-level Nats validated against a closed `KnownStatus` instance set (Status.hs lists ~50 specific codes). Unknown codes are handled by `statusFromNat`/`toEnum` for `Verb` but `KnownStatus` is a closed whitelist for UVerb — replicate as a const u16 with a known-status lookup but allow arbitrary codes where Servant does.
- EventStream / ServerSentEvents and Stream both produce SourceIO; SSE has its own EventKind (RawEvent vs JsonEvent) distinct from generic streaming framing — don't conflate SSE with Stream's framing strategies.
- Description/Summary/OperationId/Fragment/Host are 'silent' for the server handler (no argument) but ARE meaningful to Links (Fragment) and Docs/OpenAPI (Description/Summary/OperationId) and routing (Host). A Rust no-op extractor must still surface these to the docs/links interpretations.

### rust_mapping
Model the API description as a *type-level builder of typed tuples* using a sealed trait, not phantom strings. Core trait: `trait Api { type Handler; }` where each combinator is a generic struct that prepends/wraps its contribution onto the inner `Handler`. Sketch:\n\n```rust\n// Structural\npub struct Path<const SEG: &'static str, Inner>(PhantomData<Inner>); // or Segment(&str) carried in a builder\npub struct Sub<L, R>(...);            // L :> R\npub struct Alt<L, R>(L, R);           // L :<|> R, left-biased; real runtime product\npub struct EmptyApi;\n\n// Modifier marker types + sealed traits\npub trait Presence { } pub struct Required; pub struct Optional; // impl Presence\npub trait Parsing  { } pub struct Strict;   pub struct Lenient;  // impl Parsing\n// Fold via a GAT/assoc-type that computes the wrapped argument type:\npub trait ArgShape<A> { type Out; }\n// Required+Strict => A; Required+Lenient => Result<A, ParseError>;\n// Optional+Strict => Option<A>; Optional+Lenient => Option<Result<A, ParseError>>\n```\n\nBecause Rust lacks Haskell's type-level Symbol/Nat folding, do NOT try to encode mods as a HList; instead make each combinator generic over two type params `P: Presence, S: Parsing` with sensible defaults via builder methods, and compute the argument type through a single `ArgShape` trait impl matrix (4 impls). E.g. `QueryParam<A, P = Optional, S = Strict>`, `Header<A, P = Optional, S = Strict>`, `ReqBody<A, S = Strict>` (P fixed Required), `Capture<A>` / `CaptureLenient<A>`, `CaptureAll<A>`.\n\nEach extracting combinator implements an `Extract` trait that yields its `ArgShape::Out`:\n```rust\ntrait FromRequestPart { type Out; fn extract(req: &RequestParts, st: &mut PathState) -> Result<Self::Out, RouteError>; }\n```\nHandler shape: build the handler argument tuple by accumulating each combinator's `Out` left-to-right; the final `Verb` fixes the return type. Represent the assembled handler as a Tower `Service` or an `async fn(Args...) -> Result<Resp, Error>` adapted via a generated trait. Use the existing ecosystem: `http::Request/Response`, `bytes::Bytes`, `mime::Mime` + `mime`/`headers` crate for Accept/Content-Type negotiation, `serde`/`serde_json` for JSON/Form codecs, `serde_urlencoded` for query/form, `tower::Service` for Raw and the runtime, `hyper` for the server.\n\nContent types: `trait Accept { fn content_type() -> Mime; fn matches(&Mime) -> bool; }`, `trait MimeRender<A> { fn render(&A) -> Bytes; }`, `trait MimeUnrender<A> { fn unrender(&Bytes) -> Result<A, CodecError>; }`. Ctype lists become tuples `(Json, PlainText)` implementing `AllCtRender`/`AllCtUnrender` that iterate in order, defaulting to the first for absent Accept, returning 406/415 otherwise.\n\nVerb: `struct Verb<M: Method, const STATUS: u16, Ct, A>`; provide `Get<Ct,A> = Verb<GET,200,..>` aliases. `NoContentVerb<M>` => unit body, fixed 204. Response headers: `struct WithHeaders<H, A>` wrapping the value, where `H` is a tuple of `ResponseHeader<NAME, V>` enum `{ Present(V), Missing, Undecodable(Bytes) }`; `add_header` prepends.\n\nAlternatives & routing: build an inspectable `Router` enum (`Alt(Box<Router>, Box<Router>)`, `Path(segment, Box<Router>)`, `Capture`, `CaptureAll`, `Leaf{method,status,..}`, `Raw`, `Host`) from the type via a `IntoRouter` interpretation, preserving left-bias and best-error selection (collect candidate errors, return the most specific: 404 < 405 < 415/406 < 400 < auth).\n\nNamedRoutes: a `#[derive(Api)]`-style derive (later) or a hand-written trait `Routes` whose fields map to a tuple of endpoints in declaration order; provide `to_alt()/from_alt()` equivalents. UVerb/MultiVerb: model union responses as a Rust `enum` with `#[derive]`-generated `AsUnion`-equivalent; per-variant `HasStatus` via `const STATUS: u16`; client parse uses an `UnrenderOutcome { StatusMismatch, BodyError(String), Ok(T) }` enum mirroring `UnrenderResult` with the same precedence. Streaming: `SourceIo = futures::Stream<Item=Result<Bytes,Err>>`; framing as a `trait FramingEncode/FramingDecode` with `NoFraming/NewlineFraming/NetstringFraming` unit structs. Metadata combinators (Description/Summary/OperationId/Fragment) carry `&'static str` and feed only the docs/links interpretations — they implement a no-op `FromRequestPart` (Out = ()) and contribute nothing to the handler tuple.

---

## server-side request extraction and the Delayed pipeline (servant-server: Delayed.hs, DelayedIO.hs, RouteResult.hs, Internal.hs HasServer instances, Router.hs choice/error selection)

**Summary:** Servant decouples *when* a check runs from *what error it reports* by storing each kind of check in a fixed slot of a `Delayed` record and running them in a hardcoded order inside `runDelayed`, regardless of the order combinators were composed. Each check yields a `RouteResult` that is either `Route a` (success), `Fail e` (recoverable; routing backtracks to sibling routes), or `FailFatal e` (commit; stop trying siblings). The fixed run order (captures, method, auth, accept, content-type, query params, headers, body, then the handler) plus a per-status priority table guarantees a stable, documented precedence of HTTP errors: 404 < 405 < 401 < 415 < 406 < 400. A Rust port must reproduce both the fixed slot ordering and the Fail/FailFatal recover-vs-commit distinction, because they are the entire reason routing produces deterministic, sensible status codes across overlapping routes.

### key_types
- RouteResult a = Fail ServerError | FailFatal ServerError | Route a — the per-route outcome; Fail is recoverable (sibling routes still tried), FailFatal commits, Route succeeds. Monad short-circuits on both Fail and FailFatal.
- RouteResultT m a — monad transformer wrapping m (RouteResult a); its bind short-circuits on Fail/FailFatal so the first failing check aborts the rest of runDelayed.
- DelayedIO a = ReaderT Request (ResourceT (RouteResultT IO)) a — a check computation: reads the Request, can do IO/resource allocation, and yields a RouteResult. delayedFail=Fail, delayedFailFatal=FailFatal, liftRouteResult lifts a pure RouteResult, withRequest gives access to the Request.
- Delayed env c — GADT with one slot per check kind: capturesD (env->DelayedIO captures), methodD (DelayedIO ()), authD (DelayedIO auth), acceptD (DelayedIO ()), contentD (DelayedIO contentType), paramsD (DelayedIO params), headersD (DelayedIO headers), bodyD (contentType->DelayedIO body), plus serverD that takes all extracted values + Request and returns RouteResult c (the partially-applied handler).
- emptyDelayed :: RouteResult a -> Delayed env a — base case: all checks are pure (), handler ignores inputs and returns the given result.
- Router' env a — routing tree: StaticRouter (Map segment subrouter) [leaf], CaptureRouter, CaptureAllRouter, RawRouter, Choice (left-biased). leafRouter wraps a single runAction call.
- ServerError { errHTTPCode::Int, errReasonPhrase::String, errHeaders, errBody } — the structured error; err400/401/404/405/406/415 are the canonical ones produced by the pipeline.
- Handler a / runAction — runs the Delayed, then if Route, runs the handler (Handler ExceptT ServerError IO); a thrown ServerError becomes Route(responseServerError err) (handler errors are NOT Fail/FailFatal — they always render as a response, never backtrack).

### rules
- FIXED RUN ORDER (runDelayed), independent of composition order: 1) capturesD env, 2) methodD, 3) authD, 4) acceptD, 5) contentD, 6) paramsD, 7) headersD, 8) bodyD content, 9) serverD (handler). The comment in runDelayed is explicit that params must run AFTER content-type check but BEFORE body parsing.
- Composition appends WITHIN a slot but never reorders slots: addCapture appends to capturesD (env->...), addMethodCheck does methodD <* new, addAuthCheck does (,) <$> authD <*> new, addAcceptCheck does acceptD *> new, addBodyCheck appends to BOTH contentD ((,)<$>contentD<*>newCT) and bodyD (runs old bodyD then new), addParameterCheck appends to paramsD, addHeaderCheck appends to headersD. passToServer adds a pure (never-failing) input derived from Request. So multiple captures/params/headers run left-to-right in source order, but a body combinator written before a query-param combinator still has its content check before params and its body parse after params.
- STATUS / CONSTRUCTOR per failure: capture parse fail -> Fail (formatted, default 400 via urlParseErrorFormatter) BUT it occurs in capture slot so it can backtrack like a 404-class miss; method mismatch -> Fail err405 (recoverable); accept (Accept header unservable) -> Fail err406 (recoverable — deliberately made recoverable by running it BEFORE body); content-type unhandled -> Fail err415; required query param missing -> FailFatal (400); query param parse error -> FailFatal (400); QueryParams parse error -> FailFatal (400); required header missing or header parse error -> FailFatal (400); Host mismatch/missing -> Fail (400, recoverable); body parse error (strict) -> FailFatal (400); body content-type unhandled -> Fail err415 (recoverable, in contentD slot).
- BasicAuth: BadPassword -> FailFatal err401 + WWW-Authenticate: Basic realm=...; NoSuchUser -> FailFatal err401 + challenge header; Unauthorized -> FailFatal err403; Authorized usr -> Route. All auth failures are FATAL (commit) — once a route matched far enough to attempt auth, sibling routes are not tried.
- HEAD handling: allowedMethodHead lets a GET endpoint match a HEAD request; on success the response body is emptied (bdy = "" if HEAD). allowedMethod = method matches OR (endpoint is GET and request is HEAD).
- ERROR SELECTION among sibling routes (runChoice): try routes left-to-right; a FailFatal or Route stops immediately and is returned. Among multiple Fail results, pick the one with the WORSE (higher-priority) HTTP code using the table: 404->0, 405->1, 401->2, 415->3, 406->4, (other)->5, 400->6. Higher priority number wins. Note 401 here is for the (rare) case auth produced a recoverable 401; the BasicAuth path makes 401 fatal so it commits.
- Capture/StaticRouter routing also injects Fail at the tree level: no matching static segment -> Fail (notFoundFormatter, 404); empty path or [""] (trailing slash) at a CaptureRouter -> Fail 404; CaptureAllRouter strips a leading empty segment to handle trailing slash. Trailing slash on a StaticRouter leaf ([] or [""]) both reach the leaf set.
- runDelayed MUST be called at most once per request: the body is a side-effecting one-shot stream and resource allocation (ResourceT) happens during the run; re-running breaks effect ordering and resource lifetime.
- Optional/lenient modifiers (unfoldRequestArgument): Required+missing -> error (FailFatal); Optional+missing -> Nothing (no failure); Required+present+lenient -> pass Either through (no failure); Required+present+strict -> error on parse fail; Optional+present+lenient -> Just(Either) (no failure); Optional+present+strict -> error on parse fail (FailFatal). Lenient turns parse failures into Either values handed to the handler instead of producing a RouteResult failure.
- Optional ReqBody special case: if not required AND no Content-Type header AND body length is KnownLength 0, the content-type check succeeds with an ignored decoder (noOptionalReqBody) rather than 415.
- Handler-thrown ServerError is terminal but NOT a routing failure: runAction converts Left err from the handler into Route(responseServerError err). It is never Fail/FailFatal, so it never causes sibling backtracking — the matched route owns the response once the handler runs.

### edge_cases
- Trailing slash: a request path ending in `/` yields a final empty segment; StaticRouter treats `[]` and `[""]` identically at a leaf, CaptureRouter rejects `[""]` as 404, CaptureAllRouter strips a leading empty segment. Port must normalize/split path identically and test `/foo` vs `/foo/`.
- Empty path segment in the middle (`//`) — produces an empty capture/static segment; must be tested against capture parsing and static matching.
- Duplicate query keys: QueryParam takes the first lookup; QueryParams collects all values under both `name` and `name[]`; QueryFlag treats present-without-value, `true`, `1`, and empty-string as true, everything else false. Must replicate the `[]` suffix and the truthy set exactly.
- Percent-encoding / UTF-8 in captures, query keys/values, and headers — decode before parse; malformed percent-encoding must be a controlled parse failure, not a panic.
- Missing Accept header defaults to `*/*` (servable by anything). Missing Content-Type defaults to `application/octet-stream`. Wildcard and malformed Accept/Content-Type values must be tested (415 vs 406 selection).
- Optional empty body: Optional ReqBody + no Content-Type + body length 0 must succeed (Nothing/ignored), NOT 415.
- HEAD against a GET endpoint must match and return an empty body with the same status/headers; HEAD against non-GET must 405.
- Lenient modifier: parse failures for captures/params/headers/body are handed to the handler as `Either`/`Result` instead of becoming a RouteResult failure — the failure path is suppressed entirely.
- Required-but-missing query param or header is FailFatal(400) and must NOT backtrack to a sibling route, unlike a method/accept/content mismatch which is recoverable Fail.
- Body must be read at most once (one-shot stream); concurrent/duplicate runDelayed or reading the body in two body checks must be prevented (bound and single-consume).
- Best-error selection: two sibling routes failing with e.g. 405 and 415 must yield 415 (worse priority); a 400 always beats 404/405/401/415/406 when all are recoverable Fails; any FailFatal or Route short-circuits selection.

### gotchas
- The slot ordering in `runDelayed` is the load-bearing semantic, NOT the combinator composition order. addAcceptCheck uses `acceptD *> new` and addMethodCheck uses `methodD <* new`, but every slot still runs at its fixed position. A naive Rust port that runs extractors in declaration order (like axum/tower) will produce different status codes for overlapping routes — you must group checks into phases and run phases in the canonical order.
- Accept (406) is intentionally checked BEFORE the body (400) precisely so it stays recoverable (`Fail`), because the body stream is irreversible — once you read the body you can't backtrack. The code comment notes that morally 400 should beat 406, but to allow streaming they accept the priority inversion. Don't 'fix' this ordering.
- Fail vs FailFatal is the recover-vs-commit distinction, and the choice is per-combinator and sometimes counterintuitive: method/accept/content/capture/host failures are recoverable `Fail`; required query/header missing, strict parse failures, and ALL BasicAuth failures (401/403) are `FailFatal`. A Rust `enum` is easy; getting each combinator's constructor right is the hard part — encode it per extractor, with tests.
- RouteResult is a Monad whose bind short-circuits on BOTH Fail and FailFatal — so within a single route the first failing check aborts the rest. The Fail-vs-FailFatal difference only matters at the `runChoice` boundary between sibling routes. Don't model FailFatal as a Rust panic/early-return that also escapes the choice loop incorrectly.
- Handler errors (thrown ServerError) are converted to `Route(response)`, NOT to Fail/FailFatal. They are terminal responses, not routing failures — the matched route owns the response. A Rust port that turns a handler `Err` into a routing failure would wrongly backtrack to sibling routes.
- The status-priority table is not the numeric order of status codes: it is 404<405<401<415<406<...<400, with 400 deliberately the worst (highest priority). Hardcode the table; do not sort by status code value.
- ResourceT integration: WithResource allocates a resource as a param-phase check (`addParameterCheck allocateResource`) tied to request lifetime; in Rust this maps to a resource/guard owned by the request scope and released after the response, not a global. The allocation participates in the pipeline and runs in the params phase, so its failure timing matters.
- Type-level heterogeneity (the `captures/params/headers/auth/body` existential tuple threaded into `serverD`) does not translate directly — Rust needs either a builder that constructs the handler argument tuple via a typed HList/macro, or a runtime `Vec<Box<dyn Any>>` keyed by insertion order. Either way you must preserve left-to-right intra-phase ordering so handler arguments line up.
- QueryParams uses FailFatal for parse errors but QueryParam-missing-required is also FailFatal while QueryParam-optional-missing is success — the required/lenient/optional matrix (unfoldRequestArgument) must be ported as a single shared helper, not reimplemented per combinator, to avoid drift.
- Capture parse failure goes through urlParseErrorFormatter (default 400) but lives in the Capture phase and is a recoverable `Fail`; despite being a 400 it is reported with low priority context because it can backtrack — so the same 400 can come from a recoverable capture-miss or a fatal body-parse, and they behave differently at choice time.

### rust_mapping
Model the per-route outcome as `enum RouteResult<T> { Fail(ServerError), FailFatal(ServerError), Route(T) }` with `?`-friendly helpers but NOT a blanket `From`/`Try` that conflates the two failure kinds. Replace `DelayedIO` with an async check function type: `type Check<'r, T> = Pin<Box<dyn Future<Output = RouteResult<T>> + Send + 'r>>` produced from `&RequestParts` (+ a body handle). Provide `fn delayed_fail(e) -> RouteResult` and `fn delayed_fail_fatal(e)`.

Rather than a Haskell-style heterogeneous GADT, use a fixed-slot pipeline struct keyed by check kind and run it in the canonical order. Concretely:

```
pub trait Extractor: Send + Sync {
    type Output: Send;
    // each impl declares which phase it runs in (see Phase below)
    fn phase(&self) -> Phase;
    fn extract<'r>(&'r self, ctx: &'r RequestCtx<'r>) -> Check<'r, Self::Output>;
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase { Capture, Method, Auth, Accept, Content, Param, Header, Body }
```

The router builds a `Delayed` that owns ordered `Vec`s per phase plus the boxed handler invoker:

```
pub struct Delayed {
    captures: Vec<BoxCheck<()>>,   // each pushes a captured value into a slot
    method:   Vec<BoxCheck<()>>,
    auth:     Vec<BoxCheck<()>>,
    accept:   Vec<BoxCheck<()>>,
    content:  Vec<BoxCheck<()>>,   // 415 here, before params
    params:   Vec<BoxCheck<()>>,
    headers:  Vec<BoxCheck<()>>,
    body:     Vec<BoxCheck<()>>,   // one-shot, consumes the body stream
    handler:  Box<dyn FnOnce(Extracted, &RequestParts) -> RouteResult<Response> + Send>,
}
```

`run_delayed(&self, ctx) -> RouteResult<Response>` runs phases strictly in the `Phase` discriminant order (Capture..Body), short-circuiting on the first `Fail`/`FailFatal` (mirror `RouteResultT` bind), then invokes the handler. Builder methods mirror Servant exactly and only ever push onto the matching phase vec: `add_capture`, `add_method_check`, `add_auth_check`, `add_accept_check`, `add_body_check` (pushes BOTH a `content` check and a `body` check), `add_param_check`, `add_header_check`, `pass_to_server` (an infallible derivation, modeled as a `params`-phase check that never fails). Extracted values flow via a typed accumulator (a tuple-building heterogeneous list, or a `TypeMap`/index-keyed `Vec<Box<dyn Any>>`) consumed by the handler closure — preserve left-to-right source order within a phase.

Routing tree: `enum Router { Static(HashMap<String, Router>, Vec<Leaf>), Capture(Router), CaptureAll(Router), Raw(RawHandler), Choice(Box<Router>, Box<Router>) }`. `run_choice` tries leaves/branches left-to-right: return immediately on `Route` or `FailFatal`; accumulate `Fail` and pick the worse code via:

```
fn priority(code: u16) -> u8 { match code {404=>0,405=>1,401=>2,415=>3,406=>4,400=>6,_=>5} }
```

Use `http::StatusCode`, `http::HeaderMap`, `mime::Mime` for the accept/content negotiation, `bytes::Bytes` for the buffered body, and a `tower::Service<Request<Body>>` adapter at the edge. `ServerError` = `struct ServerError { status: StatusCode, reason: String, headers: HeaderMap, body: Bytes }`. Error formatting (capture/param/header/body parse messages, 404/notFound) goes through pluggable `ErrorFormatters` passed in the routing context, not hardcoded strings. Handler-thrown errors return `RouteResult::Route(error.into_response())` from the handler invoker so they never backtrack — keep the type distinction between "handler returned an error response" and "extraction failed".

---

## content negotiation and codecs (Servant.API.ContentTypes)

**Summary:** This subsystem defines the mapping between Rust/Haskell values and wire bytes for a set of MIME content types, and the algorithms that select a content type given an HTTP Accept header (response serialization) or Content-Type header (request deserialization). An `Accept` typeclass names the media type(s) a content-type token represents; `MimeRender`/`MimeUnrender` serialize/deserialize a specific value type for a specific content type; and the `AllCT*`/`AllMime*` families collapse a type-level list of content types into runtime negotiation over the Accept/Content-Type headers using the http-media library (matchAccept/mapAcceptMedia for responses, matchContent/mapContentMedia for requests). Built-in types are JSON, PlainText (text/plain;charset=utf-8), FormUrlEncoded, OctetStream, and EventStream, plus a `NoContent` marker for empty bodies.

### key_types
- Accept ctype: class providing `contentType :: MediaType` and/or `contentTypes :: NonEmpty MediaType`; the media type(s) a phantom content-type token denotes. Minimal: one of the two. `contentType` defaults to head of `contentTypes`; `contentTypes` defaults to a singleton of `contentType`.
- MimeRender ctype a: `mimeRender :: a -> lazy ByteString`; serialize value `a` to bytes for content type `ctype`. Total (no failure).
- MimeUnrender ctype a: `mimeUnrender :: ByteString -> Either String a`; deserialize. Also `mimeUnrenderWithType :: MediaType -> ByteString -> Either String a` given the ACTUAL matched media type (used rarely to branch on subtype/params). Minimal: one of the two; each defaults via the other (mimeUnrender calls mimeUnrenderWithType with the type's own contentType).
- AllMime (list :: [Type]): `allMime :: [MediaType]` — flatten the type-level list to the full ordered list of all media types (expanding each ctype's `contentTypes` NonEmpty). Empty list -> []. Order: left-to-right, each ctype's contentTypes inlined in order.
- AllMimeRender list a: `allMimeRender :: a -> [(MediaType, ByteString)]` — for each media type in the list (in AllMime order), the (mediaType, serialized-body) pair. Each ctype's body is computed ONCE and paired with each of that ctype's media types.
- AllMimeUnrender list a: `allMimeUnrender :: [(MediaType, ByteString -> Either String a)]` — for each media type, a decoder closure built via `mimeUnrenderWithType` so the matched media type is threaded in.
- AllCTRender list a: `handleAcceptH :: AcceptHeader -> a -> Maybe (ByteString, ByteString)` — full RESPONSE negotiation; returns (rendered Content-Type header bytes, body bytes) or Nothing if no acceptable type. There is a `TypeError` instance for `AllCTRender '[] ()` telling users to use NoContent.
- AllCTUnrender list a: `canHandleCTypeH :: ByteString -> Maybe (ByteString -> Either String a)` (pick decoder by Content-Type header) and `handleCTypeH :: ByteString -> ByteString -> Maybe (Either String a)` (apply decoder to body); Nothing means unsupported content type.
- AcceptHeader: newtype over strict ByteString wrapping the raw Accept header value.
- NoContent: unit-like marker (single nullary constructor) for empty response bodies; has special AllMimeRender instances that render empty bytes for every listed content type.
- EventStream / EventStreamChunk: SSE content type (text/event-stream) and a newtype chunk wrapper whose MimeUnrender is identity-into-chunk.
- canHandleAcceptH: `AllMime list => AcceptHeader -> Bool` — boolean acceptability precheck (matchAccept over allMime), used by the server's recoverable acceptCheck.

### rules
- Canonical media-type tokens (EXACT, must be byte-preserved): JSON = `application/json` (no params); FormUrlEncoded = `application/x-www-form-urlencoded`; PlainText = `text/plain; charset=utf-8` (charset param IS part of the type and is emitted in the response Content-Type); OctetStream = `application/octet-stream`; EventStream = `text/event-stream`.
- Content-type ORDER is significant and left-biased: `allMime`/`allMimeRender`/`allMimeUnrender` preserve the order of the type-level list, expanding each ctype's `contentTypes` NonEmpty in order. The FIRST media type in the list is the default/canonical one when the Accept header is `*/*` or absent.
- RESPONSE negotiation (handleAcceptH) algorithm: (1) compute `amrs = allMimeRender list val` => ordered [(MediaType, body)]; (2) attach the rendered header bytes: lkup = [(mediaType, (renderHeader mediaType, body))]; (3) `M.mapAcceptMedia lkup acceptHeaderBytes` returns the value associated with the BEST matching media type, or Nothing. The returned Content-Type header is `M.renderHeader matchedMediaType` (the SERVER's canonical media type, including params like charset=utf-8 — NOT the client's Accept token).
- matchAccept / mapAcceptMedia semantics (http-media): parse Accept into a list of media ranges each with a quality value q (default q=1.0; q range [0,1], 3 decimal places). For each server-offered media type, find the most specific matching range; its q is the type's score. Reject any type whose best-matching range has q=0. Among acceptable types, choose the one with the HIGHEST q; ties broken by the order the server LISTED them (left-biased, i.e. earlier server type wins on equal quality). Matching: a range `type/subtype;params` matches an offered type if type matches (or range type is `*`), subtype matches (or range subtype is `*`), and every range parameter is present-and-equal on the offered type. `*/*` matches everything; `type/*` matches any subtype of that type. Specificity for choosing WHICH range applies to a given offered type: more specific range wins (full type+subtype+params > type/subtype > type/* > */*).
- Absent Accept header: the server substitutes `*/*` (getAcceptHeader = lookup Accept, default `*/*`). So a request with no Accept always matches and yields the FIRST server content type.
- REQUEST negotiation (canHandleCTypeH/handleCTypeH): `M.mapContentMedia (allMimeUnrender list) contentTypeHeaderBytes` selects the decoder whose media type matches the Content-Type. matchContent semantics: the Content-Type is a single concrete media type (NOT a quality list); a candidate matches if `*` wildcards in the CANDIDATE match, or exact type/subtype match, and candidate params are a subset of the Content-Type params. Returns the decoder for the first matching candidate (server list order) or Nothing.
- Absent Content-Type header on a request body: the server defaults it to `application/octet-stream` before calling canHandleCTypeH (RFC 2616 7.2.1). So a body with no Content-Type is decoded as octet-stream IF octet-stream is in the accepted list, else 415.
- SERVER ordering of checks for a method endpoint: methodCheck (405) is added first, then acceptCheck (406) — acceptCheck runs BEFORE the body is read so it is recoverable/backtrackable (can fall through to a sibling route); after the handler runs, handleAcceptH is called again and a Nothing there is a FATAL 406 (should be unreachable since acceptCheck already passed).
- SERVER ordering for ReqBody: the Content-Type check (ctCheck) runs first and a failure is `err415` (Unsupported Media Type); only if a decoder is found is the body read and decoded; a decode failure on a REQUIRED body is `err400` via the configured body-parser error formatter (delayedFailFatal — not backtrackable since the body was consumed).
- HEAD requests: when the matched method is GET and the request method is HEAD (allowedMethodHead), the negotiated body is replaced with empty bytes but the negotiated Content-Type header is still set.
- NoContent special-casing: there is intentionally NO `MimeRender ct NoContent` instance; instead AllMimeRender has overlapping instances that, for `NoContent`, pair EVERY listed media type with empty body bytes (`""`). This lets `Get '[JSON] NoContent` negotiate a Content-Type but emit no body. (The server also has a dedicated noContentRouter for 204-style responses that emits no Content-Type and empty body.)
- JSON decode uses aeson `eitherDecode` which is lenient (accepts top-level non-object/array JSON values); `eitherDecodeLenient` is a deprecated alias for `eitherDecode`.
- PlainText decoding is strict UTF-8 (`decodeUtf8'`) and returns the decode error as the `Left String`; FormUrlEncoded round-trip law (`unrender . render == Right`) only holds when no field value is empty.
- The empty list `AllCTRender '[]` is a compile-time TypeError instructing use of NoContent; i.e. an endpoint must declare at least one content type unless it is NoContent.

### edge_cases
- Absent Accept header: server substitutes `*/*` => first listed content type chosen. Test None vs explicit `*/*` produce identical result.
- Absent Content-Type on a request body: defaults to `application/octet-stream`; decoded as octet-stream if listed, else 415. Test that JSON-only endpoints reject a no-Content-Type body with 415 (unless octet-stream listed).
- Optional ReqBody with no body AND no Content-Type and KnownLength 0: special-cased to succeed (decoder ignored). Port the `noOptionalReqBody` branch.
- Accept with q=0 for the only matching type => 406 (type explicitly rejected even though it matched syntactically).
- Accept quality tiebreak: equal-q matches resolve to the FIRST server-listed type (left-biased). Test `Accept: application/json, text/plain` against server `[PlainText, JSON]` returns PlainText? No — both q=1, server lists PlainText first => PlainText. Document this carefully.
- Wildcards: `*/*`, `application/*`, `text/*` in Accept; and candidate-side wildcards in match_content_type. Test specificity (`text/plain;charset=utf-8` range beats `text/*` beats `*/*`).
- PlainText charset param: the response Content-Type MUST include `; charset=utf-8`; an Accept of `text/plain` (no params) must still match the `text/plain;charset=utf-8` server type (range params subset of offered).
- Content-Type with extra params on request (e.g. `application/json; charset=utf-8`) must match the `application/json` candidate (params subset rule); but a candidate with params requires those params present on the header.
- Malformed Accept header (garbage, empty, just `,`, missing subtype) must not panic — Servant/http-media is lenient; decide to skip unparsable entries. Add tests for empty string, `q=` without value, q>1, negative q, q with >3 decimals.
- Multiple content types mapping to the SAME media type across markers (contentTypes returning overlapping types) — first decoder/renderer in list order wins.
- JSON top-level scalar/lenient decode: `eitherDecode`/serde_json accepts e.g. `42` or `"x"` at top level — preserve lenient behavior.
- Empty content-type list at type level is a compile error in Haskell; in Rust ensure a 0-tuple/empty list either fails to satisfy a `NonEmpty` bound or is rejected (only NoContent endpoints may have empty body semantics).
- NoContent with multiple content types: every listed type pairs with empty body; negotiation still picks one Content-Type per Accept. Distinguish from true 204 noContentRouter (no Content-Type header at all).
- HEAD request to a GET route: body emptied, Content-Type preserved; Content-Length semantics handled by adapter.
- Required body decode failure => 400 via configurable error formatter, FATAL (no backtrack). Unsupported content type => 415, also after body not yet read. Don't conflate the two status codes.
- Duplicate/comma-folded Accept headers across multiple header lines — http joins them; ensure parser handles a single combined value and/or multiple HeaderValue entries.
- UTF-8 decode failure in PlainText unrender returns the error string, not a panic.

### gotchas
- Haskell uses type-level lists `'[JSON, PlainText]` resolved by typeclass instance selection (AllMime/AllMimeRender built by structural recursion with OVERLAPPABLE/OVERLAPPING instances). Rust has no overlapping instances; implement `ContentTypes<T>` for tuples via a macro and rely on ordinary trait impls. The NoContent OVERLAPPING trick (no `MimeRender ct NoContent` instance, special AllMimeRender instances instead) is unnecessary in Rust — a single blanket `MimeRender<C> for NoContent` works.
- `mimeRender` is total (returns ByteString, never fails); only `mimeUnrender` returns `Either`. Don't make MimeRender fallible to match Servant's contract — keep render infallible (or document the deviation).
- Two-method MimeUnrender (`mimeUnrender` vs `mimeUnrenderWithType`) with mutual defaults: the matched media type is threaded through `allMimeUnrender` so decoders can branch on the actual subtype/params. Most impls ignore it; preserve the hook (a `fn(&Mime, &[u8])` decoder) but default to ignoring the Mime.
- `contentType`/`contentTypes` mutual-default with MINIMAL pragma: a marker must provide at least one. In Rust, providing default bodies for both methods that call each other causes infinite recursion if neither is overridden — make `MediaTypeMarker` a sealed/required-method situation, or split into a required `content_types()` (NonEmpty) with a provided `content_type()` = first, so exactly one is required (prefer requiring `content_types`).
- The response Content-Type emitted is the SERVER's canonical media type (`renderHeader matchedMediaType`), INCLUDING params like `charset=utf-8`, NOT the client's Accept token. Don't echo the Accept value.
- matchAccept (response) uses quality + specificity + left-bias; matchContent (request) is a different algorithm — it has NO quality values and treats the candidate list entries as PATTERNS matched against the single concrete Content-Type (params subset). Implement two distinct functions; do not reuse one for both.
- The Accept-default-`*/*` and Content-Type-default-`application/octet-stream` substitutions live in the SERVER (getAcceptHeader / ctCheck), not in ContentTypes.hs. Keep them in servant-server, but the negotiation functions must handle the substituted values consistently.
- Ordering of server checks is load-bearing: acceptCheck is run BEFORE the body is read specifically so it is RECOVERABLE (can backtrack to a sibling route) — the source comment explains it has flip-flopped historically. If you read the body first you must make 406 fatal. Preserve check order: method -> accept -> content-type -> body.
- The double 406 path: acceptCheck pre-validates (recoverable err406), then handleAcceptH after the handler returns Nothing => FATAL err406 ('unreachable'). Mirror with an internal invariant; treat post-handler negotiation failure as a 500-ish/fatal-406 server bug, not a normal client error.
- `AllCTRender '[] ()` is a custom compile-time TypeError nudging toward NoContent; replicate as a helpful trait-bound error or doc, not a runtime panic.
- serde_json is approximately as lenient as aeson `eitherDecode` for top-level values, but error message FORMAT differs — golden/round-trip tests comparing exact Haskell error strings will not match; assert on success/failure + status, not exact message text.
- FormUrlEncoded round-trip law caveat (empty field values break it) is documented in Haskell; serde_urlencoded behaves slightly differently — verify and document any deviation.
- lazy ByteString vs strict: Haskell uses lazy ByteString for bodies and strict for headers, converting with toStrict/fromStrict. In Rust use `bytes::Bytes` uniformly; the rendered Content-Type goes into an `http::HeaderValue` (validate it is a legal header value).
- EventStream/SSE MimeUnrender is identity-into-chunk and ties into the streaming framing subsystem (FromSourceIO/FramingUnrender) — out of scope for plain negotiation but the marker must exist so `text/event-stream` participates in matching.

### rust_mapping
Use the `mime` crate for media types and the `http` crate's headers; do NOT pull http-media. Core traits (object-unsafe, generic; sealed `Accept` for built-ins is fine since users add their own):

```rust
use bytes::Bytes;
use mime::Mime;

/// Names the media type(s) a content-type marker denotes. Mirrors `Accept`.
pub trait MediaTypeMarker {
    /// Canonical media type (first / default). Used as response Content-Type.
    fn content_type() -> Mime { Self::content_types().swap_remove(0) }
    /// All media types this marker matches, in priority order. Non-empty.
    fn content_types() -> Vec<Mime> { vec![Self::content_type()] }
}

/// Serialize a value into bytes for marker `C`. Total (infallible by Servant law);
/// allow Result for robustness but built-ins never fail.
pub trait MimeRender<C: MediaTypeMarker> {
    fn mime_render(&self) -> Bytes;            // matches `mimeRender :: a -> ByteString`
}

/// Deserialize a value from bytes for marker `C`.
pub trait MimeUnrender<C: MediaTypeMarker>: Sized {
    fn mime_unrender(body: &[u8]) -> Result<Self, String>;
    /// Variant threading the actually-matched media type (cf. mimeUnrenderWithType).
    fn mime_unrender_with_type(matched: &Mime, body: &[u8]) -> Result<Self, String> {
        let _ = matched; Self::mime_unrender(body)
    }
}
```

Built-in markers as zero-sized types implementing `MediaTypeMarker`:
- `Json` -> `mime::APPLICATION_JSON`; `MimeRender<Json> for T: Serialize` via serde_json::to_vec; `MimeUnrender<Json> for T: DeserializeOwned` via serde_json (serde_json is already lenient about top-level scalars like aeson).
- `PlainText` -> `text/plain; charset=utf-8` (`mime::TEXT_PLAIN_UTF_8`); impls for `String`/`&str` (render = utf8 bytes; unrender via `String::from_utf8` mapping error to String).
- `FormUrlEncoded` -> `application/x-www-form-urlencoded`; impls for `T: Serialize`/`DeserializeOwned` via `serde_urlencoded`.
- `OctetStream` -> `mime::APPLICATION_OCTET_STREAM`; impls for `Bytes`/`Vec<u8>` as identity.
- `EventStream` -> `text/event-stream`; chunk newtype.

The type-level list (`'[JSON, PlainText]`) maps to a tuple-based HList or, more idiomatically, to a runtime ordered registry built by a `ContentTypeList` trait implemented for tuples:

```rust
/// Mirrors AllMime + AllMimeRender + AllMimeUnrender for a value type `T`.
pub trait ContentTypes<T> {
    /// AllMime: ordered, expanded list of media types (preserves declaration order).
    fn all_media_types() -> Vec<Mime>;
    /// AllMimeRender: (media_type, body) pairs; body computed once per marker.
    fn all_render(value: &T) -> Vec<(Mime, Bytes)> where T: ?Sized;
    /// AllMimeUnrender: (media_type, decoder) where decoder threads matched type.
    fn all_unrender() -> Vec<(Mime, fn(&Mime, &[u8]) -> Result<T, String>)>;
}
// impl for (C1,), (C1, C2), (C1, C2, C3), ... up to N via a macro.
```

Negotiation lives in `servant`/`servant-server` runtime as free functions over the `http::HeaderMap` (not on a god object):

```rust
pub enum NegotiationError { NotAcceptable /*406*/, UnsupportedMediaType /*415*/ }

/// RESPONSE: mirror handleAcceptH. Returns chosen Content-Type + body.
/// Absent Accept => treat as `*/*` (caller substitutes), yielding first listed type.
pub fn negotiate_response(
    accept: Option<&http::HeaderValue>,
    candidates: &[(Mime, Bytes)],   // from ContentTypes::all_render, in server order
) -> Option<(Mime, Bytes)>;          // None => 406

/// REQUEST: mirror canHandleCTypeH/handleCTypeH.
/// Absent Content-Type => default to application/octet-stream before matching.
pub fn match_content_type<'a, D>(
    content_type: Option<&http::HeaderValue>,
    candidates: &'a [(Mime, D)],
) -> Option<&'a (Mime, D)>;           // None => 415
```

`negotiate_response` reimplements matchAccept/mapAcceptMedia: parse the Accept header into `(media_range: Mime-ish, q: f32)` entries (q default 1.0, ignore/clamp out-of-range, q=0 => excluded), reject malformed entries leniently, then for each server candidate find its most-specific matching range, score by that range's q, drop q==0, pick max q with EARLIEST server index as tiebreak (stable). A small `AcceptHeader` parser type should handle wildcards `*/*`, `type/*`, params subset-matching, and `q=` extraction; prefer the `accept-header`/`mediatype`-style parsing crate or hand-roll a structured parser (no ad-hoc string ops per security rules). `match_content_type` uses subset/wildcard matching with the candidate as pattern and the header as the concrete type, first-match-wins.

`NoContent` is a unit struct; provide blanket `MimeRender<C> for NoContent { fn mime_render(&self) -> Bytes { Bytes::new() } }` for ALL markers (Rust has no overlapping-instance problem here), and a separate `no_content_response()` path in the server for true 204 responses (no Content-Type, empty body). The server wires `negotiate_response`/`match_content_type` into the routing pipeline in this fixed order: method (405) -> accept (406, recoverable/backtracking) -> for bodies content-type (415) -> body decode (400 via error formatter, fatal). HEAD-of-GET clears the body but keeps the negotiated Content-Type.

---

## Typed client generation (servant-client-core: HasClient / Request / Response / RunClient / BaseUrl / ClientError)

**Summary:** HasClient is a type class indexed by a monad `m` and an API type `api` that computes an associated `Client m api` type and folds a `Request` builder left-to-right across the combinator chain. Each combinator either (a) adds a curried function argument to the client (Capture, CaptureAll, Header, QueryParam/Params/Flag/String/DeepQuery, ReqBody, StreamBody, AuthProtect, BasicAuth, Raw method) by mutating the partial Request and recursing, (b) is transparent (path literal appends a segment; metadata combinators like Summary/Description/HttpVersion/Vault/IsSecure/RemoteHost are no-ops), or (c) is a terminal Verb that finalizes the request (sets method + Accept), runs it through RunClient, validates status and Content-Type, and decodes the body to `m a`. `:<|>` produces a pair of clients. The whole thing is transport-agnostic: HasClient builds an abstract `Request` and decodes an abstract `Response`; the actual HTTP is done by a `RunClient m` instance (servant-client supplies `ClientM` over http-client + a `BaseUrl`/`ClientEnv`).

### key_types
- HasClient m api: class with `type Client m api`, `clientWithRoute :: Proxy m -> Proxy api -> Request -> Client m api`, and `hoistClientMonad` to swap the effect monad
- Client m api: associated type family computing the generated client shape (function args + return) per combinator
- RunClient m: transport trait — `runRequestAcceptStatus :: Maybe [Status] -> Request -> m Response` and `throwClientError :: ClientError -> m a`; `runRequest = runRequestAcceptStatus Nothing`
- RunStreamingClient m: `withStreamingRequest :: Request -> (StreamingResponse -> IO a) -> m a` for Stream
- RequestF body path: builder record {requestPath, requestQueryString :: Seq QueryItem, requestBody :: Maybe (body, MediaType), requestAccept :: Seq MediaType, requestHeaders :: Seq Header, requestHttpVersion, requestMethod}; `Request = RequestF RequestBody Builder`
- RequestBody: LBS | strict BS | streaming SourceIO
- ResponseF a: {responseStatusCode :: Status, responseHeaders :: Seq Header, responseHttpVersion, responseBody :: a}; `Response = ResponseF LBS`, `StreamingResponse = ResponseF (SourceIO BS)`
- ClientError: FailureResponse | DecodeFailure | UnsupportedContentType | InvalidContentTypeHeader | ConnectionError
- BaseUrl: {scheme :: Http|Https, host, port :: Int, path}; default ports 80/443; trailing slash stripped on parse
- AuthenticatedRequest a = (AuthClientData a, AuthClientData a -> Request -> Request) — caller supplies a request-mutator
- EmptyClient: unit client for EmptyAPI; AsClientT m / NamedRoutes: record-of-clients via Generic
- defaultRequest: GET, empty path/query/headers/accept, no body, HTTP/1.1

### rules
- Request is built incrementally and LEFT-BIASED in source order: each `:>` combinator mutates the Request then recurses into the sublayout, so path segments, query items, and headers accumulate in declaration order. Order MUST be preserved (requestQueryString and requestHeaders are ordered Seq, appended with |>).
- appendToPath: `requestPath = requestPath <> "/" <> p`. The leading separator is always added, so the final path always starts with '/'. CRITICAL: appendToPath assumes p is ALREADY percent-encoded — Capture/path-literal callers pre-encode via toEncodedUrlPiece BEFORE calling appendToPath. Do not double-encode.
- Path literal (`(path :: Symbol) :> api`): Client unchanged; appends `toEncodedUrlPiece (symbolVal)` to path.
- Capture' mods cap a :> api: Client = `a -> Client m api`; appends `toEncodedUrlPiece val` (percent-encoded) to path.
- CaptureAll cap a :> api: Client = `[a] -> Client m api`; folds each encoded element into the path with appendToPath (one segment each, in list order).
- QueryParam' mods sym a :> api: Client = `RequiredArgument mods a -> Client m api`. Optional (default) => arg is `Maybe a`, Nothing adds nothing; required => arg is `a`. Value encoded via encodeQueryParamValue (urlEncode with queryEncode=True over toQueryParam) and appended as (name, Just value).
- QueryParams sym a :> api: Client = `[a] -> Client m api`; appends one (sym, Just encodedValue) per list element; empty list adds nothing.
- QueryFlag sym :> api: Client = `Bool -> Client m api`; True appends (sym, Nothing) value-less param, False adds nothing.
- QueryString :> api: Client = `Query -> Client m api`; REPLACES the entire query string via setQueryString (not append).
- DeepQuery sym a :> api: Client = `a -> Client m api`; expands ToDeepQuery into multiple bracketed params and appends each (value UTF-8 encoded).
- Header' mods sym a :> api: Client = `RequiredArgument mods a -> Client m api`; addHeader name (toHeader val). Optional Nothing adds no header. Header value encoded via ToHttpApiData toHeader.
- Host sym :> api: transparent to Client type; adds a literal `Host` header.
- ReqBody' mods (ct ': cts) a :> api: Client = `a -> Client m api`; sets body to `mimeRender ct a` with media type = `contentType ct` (the FIRST/primary content type of ct), via setRequestBodyLBS. Content-Type list MUST be non-empty (enforced by ct ': cts).
- StreamBody' mods framing ctype a :> api: Client = `a -> Client m api`; sets a streaming RequestBodySource using framingRender + mimeRender, media type = contentType ctype.
- Verb method status (ct ': cts) a (OVERLAPPABLE general case): Client = `m a`. Sets requestMethod = method, requestAccept = ALL media types of ct (contentTypes ct), calls runRequestAcceptStatus (Just [status]) — i.e. only THIS declared status is accepted as success — then `decodedAs ct`.
- Verb ... NoContent (OVERLAPPING): Client = `m NoContent`; sets method, runs with accept-status (Just [status]), ignores body, returns NoContent. No Accept header set.
- NoContentVerb method: Client = `m NoContent`; sets method, uses runRequest (Nothing => default success = 2xx), returns NoContent.
- Verb ... (Headers ls a): Client = `m (Headers ls a)`; decodes body via ct then builds the typed response-header record from responseHeaders via BuildHeadersTo. Headers ls NoContent variant skips body decode.
- decodedAs (the response decode helper): (1) read Content-Type header — missing => default `application/octet-stream`; unparseable => throw InvalidContentTypeHeader. (2) The response Content-Type MUST `matches` one of the acceptable content types (contentTypes ct), else throw UnsupportedContentType. (3) mimeUnrender ct body; Left => throw DecodeFailure, Right => value. ORDER: content-type presence/parse, then content-type match, then decode.
- Status acceptance: Verb passes (Just [declaredStatus]) so a response with a different status code is a FailureResponse (constructed by the backend), NOT decoded. Only the exact declared status is decoded. Default (runRequest/Nothing) treats 2xx (statusIsSuccessful) as success.
- UVerb / MultiVerb: Client = `m (Union as)` / `m r`; accept = ALL mime types of the content-type list; accept-status = the set of all declared statuses; after the response, content-type must match an accepted type (else UnsupportedContentType) then parsers are tried in declared order, matching on status first (StatusMismatch/ClientStatusMismatch) then content (DecodeFailure on no parse).
- Stream method ...: Client = `m a` (RunStreamingClient); sets accept = [contentType ct] (single), method; consumes the streaming response body via framingUnrender + fromSourceIO.
- AuthProtect tag :> api: Client = `AuthenticatedRequest (AuthProtect tag) -> Client m api`; applies the user-supplied `func val` to the Request. BasicAuth realm usr :> api: Client = `BasicAuthData -> Client m api`; adds `Authorization: Basic base64(user:pass)` header.
- Raw / RawM: Client = `Method -> m Response`; sets the method from the argument and returns the raw Response (no decode, no status check beyond default).
- `:<|>` (alternatives): Client m (a :<|> b) = Client m a :<|> Client m b — a PAIR of clients; clientWithRoute threads the SAME accumulated Request into both branches independently.
- EmptyAPI => Client = EmptyClient (unit). NamedRoutes api => Client = api (AsClientT m), a record whose fields are the per-route clients (built by converting to the generic :<|>/:> representation and back).
- Backend success/failure (servant-client ClientM): on a non-accepted status the backend builds FailureResponse by stripping the request body to () and resolving requestPath Builder to (BaseUrl, encoded-path-bytes); connection-level exceptions become ConnectionError. The Verb-level code never throws FailureResponse itself — that is the RunClient instance's job based on the Maybe [Status] argument.
- Request finalization to wire (defaultMakeClientRequest): method/host/port/path from BaseUrl + requestPath; Accept header rendered from requestAccept (omitted if empty); Content-Type from requestBody's media type (omitted if no body, and body defaults to empty BS); user headers EXCLUDE any manually-set Accept/Content-Type (those are owned by requestAccept/requestBody); query string built WITHOUT re-encoding keys beyond urlEncode True on key and raw value; secure flag from scheme.

### edge_cases
- Missing Content-Type response header => treated as `application/octet-stream` (NOT an error); only an UNPARSEABLE Content-Type yields InvalidContentTypeHeader.
- Content-Type match uses media-type `matches` (wildcard/parameter aware), not string equality — e.g. server `application/json; charset=utf-8` must match accepted `application/json`. Use a real media-type matcher (mime + a matches predicate), not ==.
- Empty Accept => backend OMITS the Accept header entirely (do not send `Accept:` empty).
- Verb accepts ONLY the single declared status as success; a 200 endpoint receiving 201 becomes FailureResponse, not a decode. Test status mismatch != decode failure.
- NoContentVerb uses default 2xx success (Nothing), while Verb...NoContent uses the exact declared status — two different acceptance rules for 'no content'.
- appendToPath always prepends '/', so empty path-literal segments still create '/' separators; trailing/leading slash behavior must match. Path pieces are pre-encoded — double-encoding (re-percent-encoding) is a bug.
- Query param value encoding: urlEncode with query-component rules over the UTF-8 of toQueryParam. Reserved chars, spaces (+ vs %20), and '&'/'=' inside values must be encoded. QueryFlag and QueryString add value-less / verbatim items respectively.
- Duplicate query keys are allowed and ORDER-preserving (QueryParams emits repeated keys; appendToQueryString uses |>). Do not dedupe or reorder into a map.
- Duplicate / multiple headers: requestHeaders is an ordered Seq allowing repeats; map to http::HeaderMap with append (not insert) to keep multiples.
- Backend strips/overrides manually-added `Accept` and `Content-Type` headers: requestAccept and requestBody own those. A user Header named Accept/Content-Type is dropped at wire time — port must replicate or document the divergence.
- Optional (Maybe) QueryParam/Header with Nothing adds NOTHING; required ones always add. The Required/Optional distinction changes the arg type (Option<T> vs T).
- ReqBody content-type list must be non-empty (type-enforced in Haskell via ct ': cts). The request media type used is the PRIMARY content type, but Accept for the response is the full list.
- BaseUrl parsing strips a single trailing slash; default ports 80 (http) / 443 (https) are normalized and omitted when showing the URL. Eq ignores a leading '/' difference in path.
- FailureResponse stores the request with its body erased to () and the path resolved against BaseUrl — so error reporting includes BaseUrl + encoded path but NOT the request body.
- ConnectionError wraps transport exceptions distinctly from any HTTP-level error; equality only compares exception type, so don't rely on message equality.
- Empty request body: backend sends an empty byte body and NO Content-Type header (None), even though the wire body is present-but-empty.
- Authorization header is redacted in the Request's Show/Debug instance — the Rust Debug impl for ClientRequest must redact Authorization (and per security rules, cookies/JWT/API keys).
- UVerb/MultiVerb: empty response body is decoded as the unit/() body variant before trying parsers; parsers are tried in declared order and the FIRST status+content match wins (left-biased).

### gotchas
- Haskell's `type Client m api` is a CLOSED-WORLD associated type family resolved at compile time; Rust has no equivalent for arbitrary curried `a -> b -> ... -> m r` shapes. Either box closures (Design A, loses zero-cost + leaks impl Trait in assoc types until TAIT stabilizes) or generate concrete endpoint structs / use an `Endpoint { type Args; type Output }` descriptor (Design B). Do NOT try to mechanically reproduce the curried function type.
- `hoistClientMonad` (natural transformation `forall x. m x -> m' x` mapped over the whole client) has no clean stable-Rust analog (no higher-rank type-level fn over an effect). In Rust, parameterize the generated client over the `RunClient` impl `C` directly; 'hoisting' becomes constructing the client with a different `C`. Drop hoist as a first-class op.
- RunClient is a Monad in Haskell carrying ClientEnv via Reader + ExceptT; in Rust make it an async trait whose `&self` owns BaseUrl/middleware/cookie-jar. `runRequest = runRequestAcceptStatus Nothing` — keep the Option<&[StatusCode]> distinction; it is load-bearing for success classification.
- The `Maybe [Status]` accept-status argument is the ONLY place success/failure status is decided; HasClient combinators never inspect status codes for failure — they delegate to the transport. Keep this separation: combinators say WHICH statuses are OK; the RunClient impl turns a non-OK status into FailureResponse.
- OVERLAPPING/OVERLAPPABLE instance resolution picks the most specific Verb instance (NoContent vs Headers vs plain). Rust has no instance overlap; you must dispatch on the return TYPE explicitly (e.g. blanket `Decode for T`, specialized impls for NoContent / Headers<H,T> / Union / Response). Specialization is unstable, so use distinct marker types or an enum of return shapes rather than overlapping impls.
- The `cts' ~ (ct ': cts)` 'Non-Empty Content Types' trick (forcing the first content type to exist while staying overridable) maps to a Rust newtype/trait guaranteeing a primary media type, e.g. `trait ContentTypeList { fn primary() -> Mime; fn all() -> Vec<Mime>; }` with a `NonEmpty` invariant — not directly expressible as a list constraint.
- `toEncodedUrlPiece` already percent-encodes and appendToPath assumes encoding — the encoding responsibility lives in the ToHttpApiData layer, NOT appendToPath. In Rust keep `to_url_piece` returning encoded bytes and make `append_path_encoded` document the precondition, or invert it (encode inside append) but be consistent to avoid double-encoding.
- Query string is stored as `Seq QueryItem` (ordered, dup-allowing, value = Maybe ByteString) — this is NOT a map. Using a HashMap/BTreeMap in Rust would lose order and duplicates required by QueryParams/QueryString. Use Vec<(String, Option<String>)>.
- `setQueryString` REPLACES the whole query while every other query combinator APPENDS — easy to get wrong if you model query mutation as a single 'add' op.
- RequestBody has three constructors (LBS/strict BS/streaming SourceIO); the streaming variant (StreamBody) needs RunStreamingClient and a chunked popper. Bounded buffering matters (security rule) — the Rust stream body must be a real async stream, not buffered into Bytes.
- AuthProtect's client arg is an opaque (value, Request->Request) pair — extension point with no fixed wire shape. In Rust model as a user-supplied closure/trait object `dyn Fn(&mut ClientRequest)`; do not hardcode a header.
- NamedRoutes/Generic record-of-clients relies on GHC Generics to convert between a record and the `:<|>`/`:>` tree. In Rust this is the natural target (a struct of methods) but requires a derive macro to generate field <-> endpoint mapping; defer the macro until the trait/data model is proven (per project rules).
- `Show`/Debug redaction of Authorization is built into RequestF's Show instance — porting Debug derive blindly would leak secrets. Hand-write Debug to redact.
- Status acceptance for streaming and the `TODO: honour the accept-status argument` in the Free-monad RunClient instance means the abstract `ClientF` interpreter ignores accept-status; only concrete backends enforce it. Document that the abstract test backend may differ from the real transport on status handling.

### rust_mapping
CORE TRAITS (crate `servant-client-core`):

```rust
/// Transport-agnostic abstract request the combinators build up.
#[derive(Clone)]
pub struct ClientRequest {
    pub method: http::Method,
    pub path: String,              // always percent-encoded segments, leading '/'
    pub query: Vec<(String, Option<String>)>, // ordered, value pre-encoded or raw? see gotchas
    pub headers: http::HeaderMap,  // ordered; allows duplicates
    pub accept: Vec<mime::Mime>,   // ordered, primary first
    pub body: Option<RequestBody>, // (bytes/stream, media type)
    pub version: http::Version,
}
pub enum RequestBody { Bytes(bytes::Bytes), Stream(BoxStream<'static, Result<Bytes, std::io::Error>>) , MediaTyped { data: ..., media_type: mime::Mime } }
impl ClientRequest {
    pub fn default_get() -> Self { /* GET, "", empty */ }
    pub fn append_path_encoded(&mut self, seg_already_encoded: &str); // pushes "/" + seg
    pub fn append_query(&mut self, name: &str, value: Option<String>);
    pub fn set_query(&mut self, q: Vec<(String, Option<String>)>);    // replaces
    pub fn add_header(&mut self, name: http::HeaderName, value: http::HeaderValue);
    pub fn set_body_bytes(&mut self, data: bytes::Bytes, media: mime::Mime);
    pub fn set_body_stream(&mut self, s: ..., media: mime::Mime);
}

pub struct ClientResponse {
    pub status: http::StatusCode,
    pub headers: http::HeaderMap,
    pub version: http::Version,
    pub body: bytes::Bytes,
}

/// The transport. Object-unsafe due to async; use async_trait or RPITIT.
pub trait RunClient {
    type Error: From<ClientError>; // or just `ClientError`
    /// `accept_status = None` => success is 2xx; Some(set) => only those statuses are success.
    async fn run_request(
        &self,
        accept_status: Option<&[http::StatusCode]>,
        req: ClientRequest,
    ) -> Result<ClientResponse, ClientError>;
}
pub trait RunStreamingClient: RunClient {
    async fn with_streaming_request<R>(&self, req: ClientRequest,
        f: impl FnOnce(StreamingResponse) -> R) -> Result<R, ClientError>;
}

pub enum ClientError {
    FailureResponse { base_url: BaseUrl, path: String, response: ClientResponse },
    DecodeFailure { message: String, response: ClientResponse },
    UnsupportedContentType { media_type: mime::Mime, response: ClientResponse },
    InvalidContentTypeHeader { response: ClientResponse },
    ConnectionError(Box<dyn std::error::Error + Send + Sync>),
}

pub struct BaseUrl { pub scheme: Scheme, pub host: String, pub port: u16, pub path: String }
pub enum Scheme { Http, Https } // default ports 80/443
```

HasClient — the central trait. Because Rust has no closed type families, model it as a trait with a GAT-free associated type plus a runtime that owns the partial request and the transport handle:

```rust
pub trait HasClient<C: RunClient> {
    /// The generated client value for this API fragment.
    type Client;
    /// Build the client by capturing the transport `client` and the
    /// accumulated partial request.
    fn client_with_route(client: C, req: ClientRequest) -> Self::Client;
}
```

The idiomatic Rust shape of `Client` differs per combinator. Two viable designs:

DESIGN A (closure-returning, mirrors Haskell curried args): each arg-adding combinator's `Client` is `impl FnOnce(Arg) -> <Inner as HasClient>::Client`. Clean for single endpoints but composes awkwardly and leaks `impl Trait` in associated types (needs TAIT / boxing). Maps `a -> Client m api` directly.

DESIGN B (RECOMMENDED — endpoint structs + a fluent request-builder, no type-family): represent each endpoint as a concrete generated struct (or, for hand-written APIs, an `Endpoint<Req, Resp>` descriptor) implementing an `Operation` trait:

```rust
pub trait Endpoint {
    type Args;          // tuple of captures/queries/headers/body, in declaration order
    type Output;        // decoded return (T, NoContent, Headers<H,T>, raw Response, Union...)
    const METHOD: http::Method;
    /// Build the request from typed args (does append_path/query/header/body in order).
    fn build_request(args: &Self::Args, base: ClientRequest) -> ClientRequest;
    /// The statuses to accept as success (Some) or None for 2xx.
    fn accept_status() -> Option<Vec<http::StatusCode>>;
    /// Decode a ClientResponse into Output (or transport-return for Raw/Stream).
    fn decode(resp: ClientResponse) -> Result<Self::Output, ClientError>;
}

pub async fn call<C: RunClient, E: Endpoint>(
    client: &C, args: E::Args,
) -> Result<E::Output, ClientError> {
    let mut req = ClientRequest::default_get();
    req.method = E::METHOD;
    req = E::build_request(&args, req);
    let resp = client.run_request(E::accept_status().as_deref(), req).await?;
    E::decode(resp)
}
```

`:<|>` => a generated struct with one field per route, each a callable method. `NamedRoutes` => a record struct of sub-clients (the natural Rust shape — Servant's `AsClientT`).

PER-COMBINATOR MAPPING:
- Path literal: `build_request` calls `append_path_encoded(percent_encode(seg))`. No arg.
- Capture<T: ToHttpApiData>: one arg, `append_path_encoded(t.to_url_piece())` (already encoded).
- CaptureAll<T>: arg `Vec<T>`, fold encoded segments.
- QueryParam<sym, T>: arg `Option<T>` (optional) or `T` (required); `Some/value` => `append_query(sym, Some(encode_query_value(v)))`.
- QueryParams<sym, T>: arg `Vec<T>` -> one append per element.
- QueryFlag<sym>: arg `bool` -> true => `append_query(sym, None)`.
- QueryString: arg `Vec<(String,Option<String>)>` -> `set_query` (replace).
- DeepQuery<sym, T: ToDeepQuery>: arg `T` -> expand to bracketed params.
- Header<sym, T>: arg `Option<T>`/`T` -> `add_header`.
- ReqBody<Ct, T>: arg `T` -> `set_body_bytes(Ct::render(&t), Ct::primary_media_type())`.
- StreamBody: arg stream -> `set_body_stream`.
- Verb<M, Status, Cts, T>: METHOD=M; `accept_status = Some([Status])`; accept = Cts::all_media_types(); decode = content-type-check + UnsupportedContentType + mimeUnrender + DecodeFailure.
- NoContentVerb: accept_status = None (2xx), Output = NoContent.
- Headers<H, T> return: decode body then build `Headers { value, headers: H::from_header_map(resp.headers) }`.
- UVerb/MultiVerb: accept_status = all declared statuses; Output = an enum (Rust's natural Union); decode tries variants in order, status-match first then content.
- Raw/RawM: arg `http::Method`, Output = ClientResponse, no decode.
- AuthProtect: arg an `AuthenticatedRequest` = a value + `Fn(&mut ClientRequest)` closure (or a trait `AddAuth { fn apply(&self, &mut ClientRequest); }`).
- BasicAuth: arg `BasicAuthData`, sets `Authorization: Basic <b64>`.
- Metadata (Summary/Description/HttpVersion/Vault/IsSecure/RemoteHost/Fragment/WithNamedContext/WithResource): transparent, no arg, no request change (Host adds a Host header; HttpVersion is a no-op for the client).

The `decode` shared helper mirrors `decodedAs`:
```rust
fn check_content_type(resp: &ClientResponse) -> Result<mime::Mime, ClientError> {
    match resp.headers.get(http::header::CONTENT_TYPE) {
        None => Ok("application/octet-stream".parse().unwrap()),
        Some(v) => v.to_str().ok().and_then(|s| s.parse().ok())
            .ok_or_else(|| ClientError::InvalidContentTypeHeader { response: resp.clone() }),
    }
}
```
then `media_type matches one of Cts::all()` else UnsupportedContentType, then deserialize else DecodeFailure.

`ToHttpApiData` trait supplies `to_url_piece`, `to_query_param`, `to_header` (separate methods because Servant distinguishes path/query/header encoding). Codec traits `MimeRender`/`MimeUnrender` keyed by a content-type marker type with `primary_media_type()` and `all_media_types()` (ordered).

---

## links-and-errors

**Summary:** Two cooperating subsystems. (1) Safe links (`Servant/Links.hs`): a `HasLink` type class folds an API type down a linear path, each combinator either contributing a path segment, a query param, a fragment, or a captured argument, producing a `Link` value guaranteed by the type-level `IsElem`/`MkLink` machinery to belong to the API. `safeLink api endpoint ...` returns a function whose arity equals the number of Captures/QueryParams/Fragments on the endpoint. (2) Errors (`ServerError.hs` + `ErrorFormatter.hs`): a flat `ServerError` record (HTTP code, reason phrase, lazy body, header list) with ~38 prebuilt constructors `err300..err505`; pluggable `ErrorFormatters` (4 hooks) live in the server Context and are invoked at each parse/not-found site. The router's `RouteResult` distinguishes non-fatal `Fail` (retry sibling routes; only 404/405/401/415/406/400 allowed) from `FailFatal` (stop), and `worseHTTPCode` picks the "best" error among failed alternatives.

### key_types
- HasLink endpoint: type class with associated type family `MkLink endpoint a` (the curried builder function type) and method `toLink :: (Link -> a) -> Proxy endpoint -> Link -> MkLink endpoint a`
- Link: record `{ _segments :: [Escaped], _queryParams :: [Param], _fragment :: Maybe String }` — the only constructible safe-link value
- Escaped: newtype wrapper over an already-percent-escaped String segment (built via `escaped = Escaped . escape`)
- Param: enum `SingleParam name value | ArrayElemParam name value | FlagParam name` — three query-param shapes
- LinkArrayElementStyle: `LinkArrayElementBracket` (foo[]=1) vs `LinkArrayElementPlain` (foo=1) — controls array query rendering
- MkLink (associated type): per-combinator, encodes how many/which args the link builder takes (Capture -> `v ->`, QueryParam -> `Maybe v ->` or `v ->`, QueryParams -> `[v] ->`, QueryFlag -> `Bool ->`, Fragment -> `v ->`, Verb -> terminal `a`)
- IsElem endpoint api: type-level membership check (in Servant.API.TypeLevel) that makes `safeLink` only typecheck if endpoint is part of api — the safety guarantee
- ServerError: record `{ errHTTPCode :: Int, errReasonPhrase :: String, errBody :: LBS.ByteString, errHeaders :: [HTTP.Header] }`, an Exception; rendered by `responseServerError` via `mkStatus errHTTPCode (pack errReasonPhrase)`
- ErrorFormatter = `TypeRep -> Request -> String -> ServerError` — formats body/url/header parse errors, given the failing combinator's type, full request, and message
- NotFoundErrorFormatter = `Request -> ServerError` — formats 404 (no TypeRep, no message)
- ErrorFormatters: record of 4 hooks { bodyParserErrorFormatter, urlParseErrorFormatter, headerParseErrorFormatter, notFoundErrorFormatter }; stored in server Context
- RouteResult a = `Fail ServerError | FailFatal !ServerError | Route a` — the routing-tree match outcome
- DefaultErrorFormatters = `'[ErrorFormatters]`; `mkContextWithErrorFormatter` appends defaults to a user Context so the hooks are always retrievable

### rules
- LINK: A Link is built left-to-right by folding combinators; each `:>` step prepends nothing but recurses, with the prefix combinator mutating the accumulator Link before recursing to `sub`.
- LINK: A literal path symbol (`sym :> sub`) appends one escaped segment: `addSegment (escaped seg)` where seg = the type-level symbol. Segments are joined with '/' at render time.
- LINK: `Capture' mods sym v :> sub` consumes one argument `v`, appends `escaped (toUrlPiece v)` as a segment. `CaptureAll sym v :> sub` consumes `[v]` and appends each `toUrlPiece` element as its own escaped segment (left fold, order preserved).
- LINK: `QueryParam' mods sym v :> sub`: if required (FoldRequired mods = True) the builder arg is `v` and the param is always added; if optional the arg is `Maybe v` and the param is added only when `Just` (Nothing => no param). Added as `SingleParam k (toQueryParam v)`.
- LINK: `QueryParams sym v :> sub` consumes `[v]`, adds one `ArrayElemParam k (toQueryParam v)` per element (left fold, order preserved).
- LINK: `QueryFlag sym :> sub` consumes a `Bool`: True adds `FlagParam k`, False adds nothing.
- LINK: `Fragment v :> sub` consumes `v` and SETS (overwrites, not appends) the fragment to `Just (toQueryParam v as String)`. Only one fragment per link; last write wins.
- LINK: `DeepQuery sym record :> sub` consumes a `record`, calls `toDeepQuery` to get `[([Text], Maybe Text)]`, and for each builds key `sym[k1][k2]...` via `generateDeepParam` and adds a `SingleParam`. NOTE: in the link code the deep value is escaped twice (escape applied inside mkSingleParam then again at render) — a known quirk to NOT replicate.
- LINK: Pass-through combinators that contribute NOTHING to the link (recurse to sub unchanged): Header', Vault, Description, Summary, OperationId, HttpVersion, IsSecure, RemoteHost, BasicAuth, AuthProtect, WithNamedContext, WithResource, ReqBody', StreamBody'. Their MkLink == MkLink sub.
- LINK: Terminal combinators end the fold and just apply the `toA` finalizer to the accumulated Link: Verb, NoContentVerb, Raw, RawM, Stream, UVerb, MultiVerb. `MkLink (Verb ...) r = r`.
- LINK: `:<|>` (alternatives) produces a PRODUCT of builders: `MkLink (a :<|> b) r = MkLink a r :<|> MkLink b r`; `allLinks` returns the whole product so every endpoint gets its own builder sharing the empty starting Link.
- LINK: ESCAPING uses `escapeURIString isUnreserved`, i.e. only RFC3986 unreserved characters (ALPHA / DIGIT / '-' / '.' / '_' / '~') pass through unescaped; everything else (including '/', '@', '%', space, reserved chars) is percent-encoded. So a literal segment 'foo/bar' becomes 'foo%2Fbar' and a capture 'test@example.com' becomes 'test%40example.com'.
- LINK: Param keys are also escaped at render time (`escape k`). For SingleParam: `escape k <> '=' <> escape value`. For ArrayElemParam: `escape k <> style <> escape value` where style is '[]=' (bracket) or '=' (plain). For FlagParam: just `escape k` (no '='). Query string is `?` + params joined by `&` (empty string if no params). Fragment renders as `#` + `escape fragment`.
- LINK: `toUrlPiece (Link)` = `uriPath ++ uriQuery ++ uriFragment` of the relative URI (no scheme, no authority). `linkURI` defaults array style to Bracket; `linkURI'` lets the caller choose Bracket/Plain.
- LINK: SAFETY INVARIANT: `Link` has no public smart constructor other than going through `safeLink`/`allLinks`, and these require `IsElem endpoint api`. Custom HasLink instances must only use `addSegment`/`addQueryParam`/`addFragment` to preserve escaping + membership guarantees.
- ERROR: `ServerError` is a plain record, not an enum of statuses — code+phrase are arbitrary Ints/Strings, body is a (lazy) byte buffer, headers are an ordered list. Rendering builds an HTTP status from the raw int and the reason-phrase bytes; arbitrary codes are allowed.
- ERROR: The standard constructors set is exactly: err300 Multiple Choices, err301 Moved Permanently, err302 Found, err303 See Other, err304 Not Modified, err305 Use Proxy, err307 Temporary Redirect; err400 Bad Request, err401 Unauthorized, err402 Payment Required, err403 Forbidden, err404 Not Found, err405 Method Not Allowed, err406 Not Acceptable, err407 Proxy Authentication Required, err409 Conflict, err410 Gone, err411 Length Required, err412 Precondition Failed, err413 Request Entity Too Large, err414 Request-URI Too Large, err415 Unsupported Media Type, err416 Request range not satisfiable, err417 Expectation Failed, err418 I'm a teapot, err422 Unprocessable Entity, err429 Too Many Requests; err500 Internal Server Error, err501 Not Implemented, err502 Bad Gateway, err503 Service Unavailable, err504 Gateway Time-out, err505 HTTP Version not supported. NOTE gaps: there is NO err306, err308, err408, err419-421, err423-428, err431, err451; all default constructors ship with empty body and empty headers, intended to be customized via record update (e.g. `err400 { errBody = ... }`).
- ERROR: Reason phrases are exact strings from the source (e.g. err418 = "I'm a teapot", err416 = "Request range not satisfiable", err414 = "Request-URI Too Large", err304 reason "Not Modified"); a port must reproduce these verbatim for golden/wire compatibility.
- ERROR: Four formatter hooks and their trigger sites: bodyParserErrorFormatter -> ReqBody parse failure; urlParseErrorFormatter -> Capture / QueryParam / QueryParams / QueryFlag / Fragment / DeepQuery / url-segment parse failures; headerParseErrorFormatter -> Header' and Host-header parse failures; notFoundErrorFormatter -> path did not match any route. The first three have signature `TypeRep -> Request -> String -> ServerError`; not-found is `Request -> ServerError`.
- ERROR: Defaults: body/url/header formatters all return `err400 { errBody = pack message }` (HTTP 400 with the raw error text as body). The not-found default ignores the request and returns `err404 { errBody = "404 Not Found" }`. So a missing/invalid Capture or QueryParam yields 400, not 404, by default.
- ERROR: The formatters are looked up from the server Context via `getContextEntry (mkContextWithErrorFormatter context)`; `mkContextWithErrorFormatter` appends `defaultErrorFormatters` so a lookup always succeeds even if the user supplied none. Overriding is done with record update on `defaultErrorFormatters` and inserting `ErrorFormatters` into the Context.
- ERROR: RouteResult semantics: `Fail` means 'this branch did not match, keep trying siblings' and must carry only a retryable status (404/405/401/415/406/400). `FailFatal` means 'this branch matched enough to commit; stop trying siblings and return this error' (e.g. a body that parsed-failed on an otherwise-matched route, marked fatal so a sibling can't accidentally serve it). `Route a` is success. Monad/Applicative short-circuit on either failure.
- ERROR: When matching alternatives (`runChoice`), it tries routers left-to-right; on a `Fail` it continues to the next and combines via `highestPri`; a `FailFatal` or `Route` stops immediately. `highestPri (Fail e1) (Fail e2)` keeps the one with the WORSE (higher-priority) code per `worseHTTPCode`; `highestPri (Fail _) y = y` (any non-Fail beats a Fail); empty router list => `Fail (notFoundErrorFormatter request)`.
- ERROR: `worseHTTPCode` priority (higher number = preferred/'worse', i.e. surfaced to the client): 404 -> 0 (lowest), 405 -> 1, 401 -> 2, 415 -> 3, 406 -> 4, default/other -> 5, 400 -> 6 (highest). So among competing branch failures, a 400 (bad request) beats a 406 beats a 415 beats a 401 beats a 405 beats a 404, and any non-listed code (priority 5) outranks all of those except 400. This ordering is left-biased on ties (keeps e1 when codes equal).
- ERROR: `delayedFail` produces a non-fatal `Fail` (lets routing try siblings); `delayedFailFatal` produces `FailFatal` (commits). Capture/QueryParam parse errors that occur AFTER a route is otherwise matched use FailFatal at several sites (e.g. required-query-param parse failure, header parse failure), while not-found / method-mismatch / accept-mismatch use Fail so routing can recover.

### edge_cases
- Literal path segment containing '/' (type-level symbol 'foo/bar') must be percent-encoded to 'foo%2Fbar', NOT split into two segments — test segment escaping.
- Capture value with reserved chars, e.g. 'test@example.com' -> 'test%40example.com'; space, '+', '%', non-ASCII UTF-8 all percent-encoded byte-wise after UTF-8 encoding.
- Only RFC3986 unreserved survive escaping: verify '-', '.', '_', '~' pass through but ':', '/', '?', '#', '[', ']', '@', '!', '$', '&', "'", '(', ')', '*', '+', ',', ';', '=' are encoded.
- Optional QueryParam = None contributes nothing (no '?' at all if it's the only param); required QueryParam always contributes even with an 'empty' value.
- QueryFlag false contributes nothing; true contributes a key with no '=' (FlagParam render).
- QueryParams [] => no params; QueryParams with multiple values => repeated key, order preserved; array style Bracket vs Plain differ (x[]=1 vs x=1) — golden test both.
- Empty link (bare Verb endpoint) => path '', empty query, no fragment; to_url_piece of '"hello" :> Get' => 'hello'.
- Multiple Fragment combinators or Fragment set then sub-fragment: last set wins (fragment is overwrite, not append) — though normally only one.
- DeepQuery double-escaping quirk in the Haskell link code (value escaped in mkSingleParam AND again at render) — decide intentionally NOT to double-encode in Rust and document the deviation; test deep key generation 'filter[a][b][c]=d'.
- Trailing/empty path segments: an empty literal segment should still render as an empty path component joined by '/' — confirm against router's trailing-slash handling.
- ServerError with arbitrary/non-standard HTTP code (e.g. 299, 599) and custom reason phrase must round-trip; reason phrases with non-ASCII or punctuation ("I'm a teapot") must be byte-preserved.
- ServerError with multiple headers of the same name (e.g. several Set-Cookie / WWW-Authenticate) must preserve order and duplicates — use HeaderMap::append, not insert.
- Empty body default (all err* ship empty body) vs default not-found body literally '404 Not Found'.
- Missing/invalid Capture or required QueryParam returns 400 (not 404) by DEFAULT formatter — assert this, plus that overriding the urlParse formatter changes it.
- Best-error selection ties: two sibling 404s keep the left/first one; a 400 from one branch must win over a 406/415/405/404 from another; an unlisted code (priority 5) must beat 404/405/401/415/406 but lose to 400.
- Fail vs FailFatal: a body parse failure on an otherwise-matched route must be FailFatal (sibling routes must NOT be allowed to serve it); a method/accept/path mismatch must be Fail (sibling can recover).
- Not-found formatter receives only the request (no message/type); ensure the Rust hook signature drops the message and source for parity.
- Malformed Accept header path: getAcceptHeader defaults missing Accept to '*/*' — content negotiation 406 interacts with error priority (406 has priority 4).

### gotchas
- `MkLink` is a type FAMILY producing a curried function whose arity depends on the endpoint's Captures/QueryParams/Fragments. Rust has no variadic currying, so this does NOT translate to a single generic signature — use a builder (Design A) or per-arg GAT chaining (Design B). Do not try to mirror `MkLink` as one associated fn type directly.
- `safeLink`'s safety comes from the `IsElem endpoint api` type family PLUS `Link` having private fields. Both halves are needed: in Rust, sealing the membership trait without sealing Link construction (or vice versa) loses the guarantee.
- Servant's `Param` is rendered structurally, not via a generic urlencode of the whole query string — keys, single values, array values, and flags each have distinct render rules (notably flags have NO '='). A naive `serde_urlencoded` pass will get flags and array-bracket style wrong.
- The DeepQuery link path escapes the value TWICE (once in `mkSingleParam`, once at render). This is almost certainly a latent bug; do not faithfully port it — document the intentional single-encode deviation.
- `ServerError` is a record with a free-form Int code + String reason, NOT a closed status enum. Modeling it as a Rust enum-over-StatusCode would break arbitrary codes and the record-update idiom (`err400 { errBody = .. }`); keep it a struct with a `reason: Option<String>` override because `http::StatusCode` only stores canonical phrases.
- `errReasonPhrase` is part of the wire status line and several phrases are non-canonical or unusual ("Request range not satisfiable" lowercase, "I'm a teapot", "Gateway Time-out" with hyphen). `http`'s canonical_reason() will NOT reproduce these — store and emit the explicit phrase.
- `worseHTTPCode` priority is NOT numeric ordering: it is a hand-tuned lookup where 400 (priority 6) is the most-surfaced and 404 (0) the least, with all 'other' codes at 5 between 406 and 400. Porting it as 'higher status code wins' is wrong.
- `Fail` is documented to only ever carry 404/405/406 (router-level) plus the formatter codes (401/415/400) — but this is a soft invariant, not enforced by types. The Rust enum should keep the `Fail`/`FailFatal` distinction as the load-bearing semantic, not the specific codes.
- The `TypeRep` passed to formatters identifies WHICH combinator failed so messages can differ per combinator; replace with a typed `ErrorSource` enum rather than a stringly type name to stay idiomatic and avoid relying on Rust's (unstable/limited) type-name reflection.
- Error formatters live in the server `Context` and are auto-augmented with defaults (`mkContextWithErrorFormatter`) so lookup is total. A Rust port must guarantee a default is always present (e.g. `ErrorFormatters::default()` baked into the server builder) rather than making the hooks `Option`, to match the 'lookup never fails' behavior.
- Pass-through link combinators (Header, Auth, ReqBody, IsSecure, Vault, Description, Summary, etc.) intentionally contribute nothing to the URL even though they affect server/client behavior — the link fold must skip them, so the link layer and the extraction layer share the API description but interpret combinators differently. Don't assume one combinator => one URL effect.
- `allLinks` returns a nested product (`:<|>`) of builders mirroring the API tree; nested APIs interleave capture args awkwardly (the doc warns 'nested APIs don't work well'). A Rust port should return a per-endpoint keyed collection / generated record (analogous to `allFieldLinks` with named-routes generics) rather than a positional tuple tree.

### rust_mapping
LINKS — model a `Link` as an owned, render-agnostic value plus a builder fold.

```rust
// servant::link
#[derive(Clone, Debug, Default)]
pub struct Link {
    segments: Vec<Escaped>,          // already-percent-encoded path segments
    query: Vec<QueryParam>,          // ordered, allows dup keys
    fragment: Option<String>,        // last-write-wins, escaped at render
}
#[derive(Clone, Debug)] struct Escaped(String);
#[derive(Clone, Debug)]
pub enum QueryParam { Single { key: String, value: String }, ArrayElem { key: String, value: String }, Flag { key: String } }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ArrayElementStyle { #[default] Bracket, Plain } // Bracket => key[]=v ; Plain => key=v

impl Link {
    pub fn push_segment(&mut self, raw: &str) { self.segments.push(Escaped(percent_encode_unreserved(raw))); }
    pub fn add_query(&mut self, p: QueryParam) { self.query.push(p); }
    pub fn set_fragment(&mut self, f: Option<&str>) { self.fragment = f.map(str::to_owned); }
    pub fn segments(&self) -> impl Iterator<Item=&str> { self.segments.iter().map(|s| s.0.as_str()) }
    pub fn to_uri(&self, style: ArrayElementStyle) -> http::Uri { /* relative path?query#frag */ }
    pub fn to_url_piece(&self) -> String { /* path + query + fragment, like ToHttpApiData */ }
}
```

Use the `percent-encoding` crate with a custom set that encodes everything EXCEPT RFC3986 unreserved (`A-Za-z0-9-._~`) to match `escapeURIString isUnreserved`. Render query as `?` + `&`-joined; Single => `enc(k)=enc(v)`, ArrayElem => `enc(k)` + style(`[]=` | `=`) + `enc(v)`, Flag => `enc(k)`. Fragment => `#enc(frag)`. Re-encode keys at render time exactly as Haskell does.

For the combinator fold, replace the type-family `MkLink`/`HasLink` with a sealed trait whose associated type captures the *remaining builder*. Since Rust lacks variadic currying, prefer a builder-with-typestate OR a per-endpoint typed wrapper. Two viable designs:

Design A (object-driven, recommended first): each combinator implements
```rust
pub trait LinkSegment { fn extend(&self, link: &mut Link); }
```
and the typed API description (the same `Endpoint`/route trie used by server+client) exposes `fn link(&self) -> LinkBuilder<Captures, Queries>`. The builder requires the right captures and offers optional `.query(name, v)`, `.query_param_required(...)`, `.flag(name, bool)`, `.fragment(v)`, `.query_params(name, iter)` methods that map 1:1 to the Param variants, finishing with `.build() -> Link`. Path-vs-query distinction is encoded by which builder methods exist for that endpoint.

Design B (typed currying via GAT): mirror `MkLink` with
```rust
pub trait HasLink { type Builder; fn to_link(start: Link) -> Self::Builder; }
```
where `Capture` sets `Builder = ApplyCapture<V, SubBuilder>` (a struct with a `fn apply(self, v: V) -> Sub::Builder`). This is closer to Servant but heavier; defer until Design A's data model is proven.

SAFETY: expose only `Link` constructors that go through an `Endpoint` token proven to belong to an `Api` — e.g. `fn safe_link<E: Endpoint + IsElem<A>>(api: &A, ep: &E) -> E::Builder`. Implement `IsElem` as a sealed marker trait auto-derived by the API-description macro/registration so external types cannot forge membership (the Rust analog of Servant's `IsElem` type family + the `Link` having no public field constructor).

ERRORS — keep `ServerError` a struct (record), NOT an enum of statuses, to preserve arbitrary-code support and record-update ergonomics.

```rust
// servant::error
#[derive(Clone, Debug)]
pub struct ServerError {
    pub status: http::StatusCode,         // replaces errHTTPCode + errReasonPhrase
    pub reason: Option<String>,           // override phrase if non-canonical (e.g. "Request range not satisfiable")
    pub body: bytes::Bytes,
    pub headers: http::HeaderMap,         // ordered insert preserved
}
impl ServerError {
    pub fn with_body(mut self, b: impl Into<Bytes>) -> Self { self.body = b.into(); self }
    pub fn with_header(mut self, k: HeaderName, v: HeaderValue) -> Self { self.headers.append(k, v); self }
    pub fn into_response(self) -> http::Response<bytes::Bytes> { /* status + reason + headers + body */ }
}
```

Provide the constructor set as `pub const fn`/associated fns matching names and exact reason phrases: `ServerError::err300() .. err505()` (or a `mod err { pub fn err404() -> ServerError ... }`). Encode the exact phrases (including "I'm a teapot", "Request-URI Too Large", "Gateway Time-out") so wire output matches. Implement `std::error::Error` (use thiserror) so handlers can `return Err(ServerError::err404().with_body(...))`.

Routing outcome:
```rust
pub enum RouteResult<T> { Route(T), Fail(ServerError), FailFatal(ServerError) }
```
`Fail` = try siblings; `FailFatal` = stop. Implement the best-error pick:
```rust
fn route_priority(code: u16) -> u8 { match code {404=>0,405=>1,401=>2,415=>3,406=>4,400=>6,_=>5} }
// in choose(): on two Fails keep higher priority (ties keep left/first); any non-Fail beats a Fail.
```

Formatter hooks as a struct of boxed fns held in the server `Context` (not a god object — a focused `ErrorFormatters` value):
```rust
pub type ErrorFormatter = Arc<dyn Fn(&ErrorSource, &http::request::Parts, &str) -> ServerError + Send + Sync>;
pub type NotFoundFormatter = Arc<dyn Fn(&http::request::Parts) -> ServerError + Send + Sync>;
pub struct ErrorFormatters { pub body: ErrorFormatter, pub url: ErrorFormatter, pub header: ErrorFormatter, pub not_found: NotFoundFormatter }
impl Default for ErrorFormatters { /* body/url/header => 400 with msg body; not_found => 404 "404 Not Found" */ }
```
Replace Haskell's `TypeRep` (which says which combinator failed) with a small `enum ErrorSource { Capture{name}, QueryParam{name}, ReqBody, Header{name}, CaptureAll{name}, Fragment, DeepQuery{name} }` — typed, not stringly. Each extractor calls the matching hook (`url` for path/query, `body` for ReqBody, `header` for headers) producing `RouteResult::Fail` for retryable failures and `RouteResult::FailFatal` once the route is committed (mirror the delayedFail vs delayedFailFatal split). Resolve formatters from `Context` with a default fallback equivalent to `mkContextWithErrorFormatter` so lookups never fail.

---

## servant-docs documentation model (HasDocs / API / Endpoint / Action) + the ComprehensiveAPI coverage target

**Summary:** servant-docs reflects an API type into a value-level `API` document = a list of intro paragraphs plus a HashMap keyed by `Endpoint` (path segments + HTTP method) mapping to an `Action` (captures, headers, query params, fragment, request body samples + media types, response, auth info, notes). The `HasDocs` type class threads a `(Endpoint, Action)` accumulator left-to-right through the combinator tree: path/capture combinators mutate the Endpoint, parameter/header/body/note combinators append to the Action, and terminal Verb/Stream/Raw combinators emit a single-endpoint API via `single`. A separate `markdown`/`markdownWith` renderer turns the `API` into a Markdown string with a fixed section ordering. The same data model is the natural basis for an OpenAPI emitter later: Endpoint becomes path+method, Action's params/captures/body/response become the OpenAPI Operation. ComprehensiveAPI is the master list of every combinator that must produce a docs (and server/client) instance.

### key_types
- Endpoint = { path: [String], method: HTTP.Method } — hashable/ord key into the API map; default is GET "/"
- API = { apiIntros: [DocIntro], apiEndpoints: HashMap Endpoint Action } — a Monoid; mappend unions endpoint maps with combineAction and concatenates intros
- Action = { authInfo: [DocAuthentication], captures: [DocCapture], headers: [HTTP.Header], params: [DocQueryParam], fragment: Maybe DocFragment, notes: [DocNote], mxParams: [(String,[DocQueryParam])], rqtypes: [MediaType], rqbody: [(Text,MediaType,ByteString)], response: Response } — per-endpoint accumulator
- Response = { respStatus: Int, respTypes: [MediaType], respBody: [(Text,MediaType,ByteString)], respHeaders: [HTTP.Header] } — defResponse is 200 with empty everything
- DocCapture = { capSymbol, capDesc }; DocQueryParam = { paramName, paramValues, paramDesc, paramKind } with ParamKind = Normal|List|Flag; DocFragment = { fragSymbol, fragDesc }
- DocNote = { noteTitle, noteBody:[String] }; DocIntro = { introTitle, introBody:[String] } (Ord by title); DocAuthentication = { authIntro, authDataRequired }
- HasDocs api { docsFor :: Proxy api -> (Endpoint, Action) -> DocOptions -> API } — the central reflection class
- ToSample a { toSamples :: Proxy a -> [(Text,a)] } — supplies example values; Generic default via GToSample; toSample = first sample; noSamples/singleSample/samples helpers
- ToParam t { toParam -> DocQueryParam }, ToCapture c { toCapture -> DocCapture }, ToFragment t { toFragment -> DocFragment }, ToAuthInfo a { toAuthInfo -> DocAuthentication } — user-supplied metadata classes
- AllHeaderSamples ls — builds sample response headers from a Headers '[Header ...] type-level list
- DocOptions = { maxSamples: Int=5 }; RenderingOptions = { requestExamples, responseExamples: ShowContentTypes(AllContentTypes|FirstContentType), notesHeading: Maybe String, renderCurlBasePath: Maybe String }
- ExtraInfo api = newtype over HashMap Endpoint Action — phantom-typed user overrides constrained by IsIn to a real endpoint

### rules
- docsFor threads a (Endpoint, Action) accumulator top-down/left-to-right starting from (defEndpoint=GET "/", defAction). Each combinator either mutates the Endpoint, appends to the Action, or terminally emits the API with `single`.
- Path literal (path :> api): append the path segment to Endpoint.path (`path <>~ [seg]`). Recurse with same Action.
- Capture' '[] sym a / CaptureAll sym a: append `:sym` to Endpoint.path AND append a DocCapture (from ToCapture) to Action.captures (snoc, order preserved). CaptureAll renders the same `:sym` path form.
- Capture' (Description descr ': mods): special instance builds DocCapture sym descr directly (description is the capture doc); other Capture' modifiers (Lenient, Strict, Required) are transparently peeled off one mod at a time via the overlappable `(mod ': mods)` instance — they do NOT change docs output.
- Verb method status (ct ': cts) a (terminal, no headers): emit `single endpoint' action'` where endpoint.method := reflected method, response.respStatus := status (type-level Nat), response.respTypes := allMime of content types, response.respBody := take maxSamples of sampleByteStrings (ToSample × AllMimeRender across all cts).
- Verb ... (Headers ls a) (terminal, OVERLAPPING): same as Verb but additionally response.respHeaders := allHeaderToSample ls (one (name, sample-or-placeholder) per Header in the list).
- NoContentVerb method: emit single endpoint with method set, response.respStatus := 204, empty respTypes/respBody/respHeaders.
- Stream method status framing ct a (terminal): emit single endpoint, method set, respStatus := status, respTypes := allMime [ct]; NO response body samples (streaming bodies are not sampled). Framing strategy is currently NOT documented (upstream TODO).
- Header' mods sym a :> api: append (CI name, sample-or-"<no header sample provided>") to Action.headers; recurse. Lenient/Required modifiers do not change docs.
- QueryParam' mods sym a :> api: append toParam (DocQueryParam, kind Normal) to Action.params. QueryParams sym a -> kind List. QueryFlag sym -> kind Flag. Modifiers (Required/Lenient) don't change docs structurally; they're carried in the ToParam instance the user writes.
- Fragment a :> api: SET (not append) Action.fragment := Just (toFragment). At most one fragment per endpoint; combineFragment keeps the FIRST when merging.
- ReqBody' mods (ct ': cts) a :> api: SET Action.rqbody := take maxSamples (sampleByteStrings) and Action.rqtypes := allMime. StreamBody' mods framing ctype a: set Action.rqtypes := contentTypes ctype, no samples.
- Description desc / Summary desc / OperationId oid (each `:> api`): append a DocNote to Action.notes. Description/Summary use the symbol as the note title with empty body; OperationId uses title "OperationId: <oid>". All recurse with unchanged Endpoint.
- BasicAuth realm usr :> api: append toAuthInfo (DocAuthentication) to Action.authInfo; recurse.
- Transparent/no-op combinators (carry no docs, just recurse with unchanged Endpoint+Action): RemoteHost, IsSecure, HttpVersion, Vault, WithResource res, WithNamedContext name ctx (the named-context wrapper itself is invisible — only the inner API documents).
- a :<|> b: docsFor a (ep,action) <> docsFor b (ep,action) — both branches see the SAME inherited accumulator; results are merged with the API Monoid (HashMap unionWith combineAction). Alternative routes that share an Endpoint key collapse into one merged Action.
- EmptyAPI: produces emptyAPI (no endpoints). NamedRoutes api: documents as its generic ToServantApi expansion.
- Raw: emit `single endpoint action` as-is (whatever method/path/action accumulated). Raw is a catch-all; no body/response synthesis.
- API Monoid merge: HM.unionWith combineAction. combineAction is non-commutative, LEFT-biased: concatenates list fields (authInfo, captures, headers, params, notes, mxParams, rqtypes, rqbody) and combineFragment (first wins), and combineResponse for the response. combineResponse takes status from the LEFT and concatenates respTypes/respBody/respHeaders. Status merging is intentionally NOT a monoid (would break laws).
- docs = docsWithOptions defaultDocOptions. docsWithOptions p = docsFor p (defEndpoint, defAction). docsWithIntros prepends intros. docsWith opts intros extra: generate base API, prepend intros, then HM.unionWith (flip combineAction) endpoints — note the FLIP: user ExtraInfo is merged as the LEFT argument so user-supplied response status/fragment win over generated ones.
- extraInfo p action: runs docsFor on a single endpoint then OVERWRITES every endpoint's Action with the supplied action (traversed .~ action); IsIn + HasLink constraints guarantee the proxy points at exactly one real endpoint in the API.
- markdownWith rendering order is FIXED and load-bearing: first all intros (sorted? intros kept in insertion order, but DocIntro Ord is by title), then `sort . HM.toList` of endpoints (sorted by Endpoint = (path, method) Ord). For each endpoint, sections in this exact order: H2 `## METHOD /path`, blank, Notes, Authentication, Captures, Headers, Params, Fragment, Request, Response, optional Sample Request (curl).
- Endpoint sort order: derived Ord on Endpoint = lexicographic on (path :: [String], method :: ByteString). Path components compared element-wise as strings; method as raw bytes. This determines stable doc ordering and MUST be reproduced.
- showPath: [] -> "/"; otherwise concat of ("/" ++ seg) for each segment (no trailing slash unless a segment is empty).
- Markdown param rendering branches on ParamKind: Values line shown when values non-empty OR kind /= Flag; List kind adds the `[]` list-forwarding note; Flag kind adds the no-value note.
- Response body markdown: [] -> "- No response body"; single ("", type, body) -> "- Response body as below." + fenced code; multiple -> formatBodies grouped by (label, body) via groupWith, media types intersperse-joined, code fence language chosen by markdownForType (html/xml/json/javascript/css else empty).
- sampleByteStrings encodes EVERY sample across EVERY content type (cartesian) preserving the (label) text; formatBodies assumes identical (label, body) pairs are adjacent (relies on groupWith over a stable list) — sample/render ordering is load-bearing for grouping.
- GToSample generic sampling: U1 -> single unit sample; V1 -> no samples; product (:*:) -> cartesian product of sub-samples joining labels with ", "; sum (:+:) -> interleave (U.+++) of left/right samples; K1 delegates to ToSample of the field; this yields multiple labeled samples for ADTs.

### edge_cases
- Endpoint sort stability: ordering is lexicographic on (Vec<String> path, Method bytes). A Rust port must derive Ord on Endpoint with path first, method second, and method compared as bytes (http::Method has its own Ord — verify it matches byte order, or normalize to str). Golden Markdown tests must pin this ordering.
- Path segment with empty string: showPath would emit `//` (a literal `/` between empty segs) — handle empty path components from path combinators or percent-decoded captures intentionally.
- Capture renders as `:sym` in Markdown but must become `{sym}` for OpenAPI — the path representation diverges between interpretations; store raw symbol, not the rendered form.
- Capture' with Description modifier vs other modifiers: the Description mod produces the capture's description; Lenient/Required/Strict mods are stripped without affecting docs. Port must not let a Lenient flag leak into docs output.
- Header with no ToSample: value becomes the literal string `<no header sample provided>` for both request-sensitive headers and response header samples. Preserve this exact placeholder for golden tests.
- Fragment is SET not appended, and on merge the FIRST fragment wins (combineFragment). Two alternatives that both set a fragment on the same endpoint key keep the left one.
- Multiple samples / multiple content types produce a cartesian set of (label, mime, body) entries; formatBodies groups by (label, body) and ASSUMES equal entries are adjacent. The Rust sampling must emit in an order where identical label+body pairs are contiguous, or grouping breaks.
- maxSamples truncation (default 5) applies to BOTH request body samples and response body samples via `take`. Empty/zero samples must yield `- No response body` / no Request section.
- NoContent verb: status forced to 204 with all response collections emptied even if a content-type list was present in the type — do not infer a body.
- Stream/StreamBody: no body samples are generated and framing strategy is not documented (upstream TODO). A faithful port reproduces the gap unless intentionally extending it (document the deviation).
- Raw endpoint: emits whatever (Endpoint, Action) was accumulated with no response synthesis; method/path come from preceding combinators (defaults to GET / if none).
- EmptyAPI contributes zero endpoints; an API that is only EmptyAPI renders to just the intros.
- docsWith uses `flip combineAction` for user ExtraInfo, so user-supplied response status and fragment override generated ones — opposite bias from normal alternative merging. Easy to get backwards.
- Alternative branches that resolve to the SAME (path, method) key are MERGED into one Action (lists concatenated, response status from left). This is the only place two distinct routes collapse; tests must cover duplicate-endpoint merge.
- QueryParams list params render with a `[]` suffix note and the param name is shown without brackets; QueryFlag renders a no-value note and may legitimately have empty values list.
- WithNamedContext is invisible to docs — only the wrapped inner API documents; the context name/type contributes nothing.
- Code-fence language selection (markdownForType) only special-cases html/xml/json/javascript/css; everything else gets an empty language tag. Golden tests should cover an octet-stream/plain body.
- DocIntro Ord is by title only, but intros are concatenated in insertion order in markdownWith (no sort applied to intros in the renderer) — do not sort intros even though Ord exists.

### gotchas
- No Proxy/type-class-on-types in Rust: the Haskell `HasDocs (combinator :> api)` instance chain that threads `(Endpoint, Action)` at the type level must be reimplemented either as a value-level combinator-tree walker or as a trait fold over zero-sized marker types. The value-tree approach aligns with CLAUDE.md's 'one API description drives server/client/docs' mandate and avoids re-encoding type families.
- The API/Action/Response merge operations are deliberately NON-MONOIDAL for status (status is left-biased, not combined). Do NOT implement `std::ops::Add`/`Monoid`-like blanket logic that would symmetrize status. Keep explicit `combine` methods so the left-bias is visible.
- Haskell uses an unordered HashMap then sorts at render time. A Rust HashMap is also unordered; to keep deterministic golden output, sort an explicit Vec at render time OR use IndexMap and sort on render. Insertion order of HashMap must never leak into output.
- ToSample's Generic instance does a CARTESIAN PRODUCT across record fields and an INTERLEAVE across sum constructors, generating potentially many labeled samples. A serde-derive-based Rust equivalent has no automatic multi-sample story — implementors will usually hand-write samples; do not try to mechanically port GToSample's combinatorics. Provide `single_sample`/`samples` helpers and let max_samples cap it.
- `AllMimeRender`/`AllMimeUnrender` may disagree or be undefined independently; servant-docs renders examples using the RENDER side only. In Rust, the codec used to produce a doc sample need not be the same instance that decodes a request body — but the port should reuse the shared codec registry to stay consistent, and document any divergence.
- Headers are stored as `[HTTP.Header]` = (CI ByteString, ByteString) pairs and rendered through CI.original. Rust `http::HeaderName`/`HeaderValue` already normalize case; preserve the original-casing behavior only where it matters for the 'sensitive to header X' note (uses original casing).
- `extraInfo` relies on the IsIn/HasLink type-level proof that a proxy references exactly one existing endpoint, then blanket-overwrites every endpoint Action with the supplied one. There is no runtime equivalent of that compile-time guarantee in a value model — provide an explicit `add_extra(endpoint_key, action)` keyed by the concrete Endpoint and validate membership at runtime, returning a Result.
- Markdown rendering hardcodes indentation (e.g. `     - **Values**`), heading levels (## / ### / #### when notesHeading is set), and a curl block format. These are load-bearing for golden tests; keep them in a single renderer module and pin with golden files rather than scattering format strings.
- ParamKind drives three different branches in param rendering (Values line shown unless Flag-with-no-values; List adds the [] note; Flag adds the no-value note). Model ParamKind as an enum and branch explicitly; do not infer kind from emptiness of values.
- Stream's docs status/type come from type-level Nat/Accept but body is omitted; if the Rust port adds streaming body samples it is a deliberate semantic EXTENSION beyond upstream and must be documented per CLAUDE.md.
- OperationId note uses the literal prefix `OperationId: ` in its title while Summary/Description use the bare symbol; when later lowering to OpenAPI these must map to distinct fields (operationId vs summary vs description), so do NOT round-trip notes back into OpenAPI by string-parsing the prefix — keep typed metadata or tag the note source.

### rust_mapping
Core value model (in `servant-docs`, mirroring the Action/Endpoint split, no god object):

```rust
// Endpoint key: path + method.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Endpoint { pub path: Vec<String>, pub method: http::Method }
impl Default for Endpoint { fn default() -> Self { Self { path: vec![], method: http::Method::GET } } }
impl Endpoint { pub fn show_path(&self) -> String { if self.path.is_empty() { "/".into() } else { self.path.iter().map(|s| format!("/{s}")).collect() } } }

pub enum ParamKind { Normal, List, Flag }
pub struct DocCapture { pub symbol: String, pub desc: String }
pub struct DocQueryParam { pub name: String, pub values: Vec<String>, pub desc: String, pub kind: ParamKind }
pub struct DocFragment { pub symbol: String, pub desc: String }
pub struct DocNote { pub title: String, pub body: Vec<String> }
pub struct DocIntro { pub title: String, pub body: Vec<String> } // Ord by title
pub struct DocAuthentication { pub intro: String, pub data_required: String }

pub struct SampleBody { pub label: String, pub media_type: mime::Mime, pub body: bytes::Bytes }
pub struct ResponseDoc { pub status: http::StatusCode, pub types: Vec<mime::Mime>, pub body: Vec<SampleBody>, pub headers: Vec<(http::HeaderName, http::HeaderValue)> }
impl Default for ResponseDoc { fn default() -> Self { Self { status: http::StatusCode::OK, types: vec![], body: vec![], headers: vec![] } } }

#[derive(Default)]
pub struct EndpointDoc {            // == Haskell Action
  pub auth_info: Vec<DocAuthentication>,
  pub captures: Vec<DocCapture>,
  pub headers: Vec<(http::HeaderName, http::HeaderValue)>,
  pub params: Vec<DocQueryParam>,
  pub fragment: Option<DocFragment>,
  pub notes: Vec<DocNote>,
  pub req_types: Vec<mime::Mime>,
  pub req_body: Vec<SampleBody>,
  pub response: ResponseDoc,
}

pub struct ApiDoc { pub intros: Vec<DocIntro>, pub endpoints: indexmap::IndexMap<Endpoint, EndpointDoc> }
```

Merge semantics as explicit functions (NOT std `Add`/`Extend`, because they are non-commutative and left-biased):

```rust
impl EndpointDoc { pub fn combine(self, rhs: EndpointDoc) -> EndpointDoc { /* concat list fields; fragment = self.fragment.or(rhs.fragment); response.combine(...) */ } }
impl ResponseDoc { pub fn combine(self, rhs: ResponseDoc) -> ResponseDoc { ResponseDoc { status: self.status /* LEFT wins */, types: [self.types, rhs.types].concat(), body: ..., headers: ... } } }
impl ApiDoc { pub fn merge(mut self, rhs: ApiDoc) -> ApiDoc { /* extend intros; for each rhs endpoint, entry.combine */ } }
```

The reflection class. Because Rust has no `Proxy`/type-class-on-types, model the API as a value-level combinator tree (the same tree the router/client traverse). Use a `HasDocs` trait on the combinator marker types that takes the threaded accumulator by mutation, plus `DocsContext`:

```rust
pub struct DocsContext<'a> { pub opts: &'a DocOptions }
pub struct DocOptions { pub max_samples: usize } // default 5

pub trait HasDocs {
    /// Fold this combinator's contribution into `(ep, action)`; terminal combinators flush into `out`.
    fn docs_for(ep: &Endpoint, action: &EndpointDoc, ctx: &DocsContext) -> ApiDoc;
}
```

Idiomatic alternative that fits servant-rs's value-driven model better than a pure type trait: keep an `Api` description enum/struct (the same one server/client use) and implement a `to_docs(&self, ep, action, ctx) -> ApiDoc` walker. Combinators map directly:
- `Path(seg) :> rest` -> push seg, recurse.
- `Capture { symbol, to_capture } :> rest` -> push `:symbol`, action.captures.push(to_capture), recurse. `CaptureAll` identical path form.
- `QueryParam/Params/Flag` -> action.params.push(DocQueryParam{kind: Normal|List|Flag}).
- `Header` -> action.headers.push((name, sample_or_placeholder)).
- `Fragment` -> action.fragment = Some(..) (set, not append).
- `ReqBody{cts, sample}` -> action.req_types = all_mime(cts); action.req_body = take(max_samples, sample_byte_strings).
- `Description/Summary/OperationId` -> action.notes.push(DocNote{..}).
- `BasicAuth` -> action.auth_info.push(..).
- `RemoteHost|IsSecure|HttpVersion|Vault|WithResource|WithNamedContext` -> recurse unchanged.
- `Alt(a, b)` -> a.to_docs(ep, action).merge(b.to_docs(ep, action)).  // both see same accumulator
- `EmptyApi` -> ApiDoc::default().
- Terminal `Verb { method, status, cts, sample } / Stream / NoContent / Raw` -> ApiDoc::single(ep.with(method,status,...), action).

Sample/metadata supplier traits, the Rust analogue of ToSample/ToParam/ToCapture/etc. Use serde for serialization rather than per-content-type `AllMimeRender`:

```rust
pub trait ToSample: Sized { fn to_samples() -> Vec<(String, Self)>; } // (label, value)
// helpers: single_sample(x), no_samples(), samples(vec)
pub trait ToParam { fn to_param() -> DocQueryParam; }
pub trait ToCapture { fn to_capture() -> DocCapture; }
pub trait ToFragment { fn to_fragment() -> DocFragment; }
pub trait ToAuthInfo { fn to_auth_info() -> DocAuthentication; }
```

`sample_byte_strings` becomes: for each (label, value) sample, for each supported `mime::Mime`, render via the codec registry (serde_json etc.), preserving label; collect `Vec<SampleBody>`. Truncate to `max_samples`. Header samples: one entry per declared response header, value = encoded sample or a `<no header sample provided>` placeholder.

Renderer is a SEPARATE module (`servant_docs::markdown`), never on the data type:

```rust
pub struct RenderingOptions { pub request_examples: ShowContentTypes, pub response_examples: ShowContentTypes, pub notes_heading: Option<String>, pub render_curl_base_path: Option<String> }
pub enum ShowContentTypes { AllContentTypes, FirstContentType }
pub fn markdown(api: &ApiDoc) -> String; // = markdown_with(default)
pub fn markdown_with(opts: &RenderingOptions, api: &ApiDoc) -> String;
```
Render order MUST be: intros, then endpoints sorted by `Endpoint` Ord (path Vec<String> then method) — sort a `Vec<(&Endpoint,&EndpointDoc)>`. Per endpoint: `## METHOD /path`, then Notes, Authentication, Captures, Headers, Params, Fragment, Request, Response, optional curl — in exactly this sequence.

OpenAPI bridge (later, `servant-openapi`): the SAME `ApiDoc` is the lowering source. Endpoint.path (with `:sym` -> `{sym}`) + method -> OpenAPI Path Item / Operation; captures+params -> `parameters[]` (path/query, with `kind` distinguishing array vs flag/boolean); headers -> header parameters; req_types+req_body -> `requestBody.content`; response (status, types, body, headers) -> `responses`; notes/Summary/Description/OperationId -> Operation.summary/description/operationId; auth_info -> securitySchemes refs. Provide a separate trait `ToSchema` (serde-aware) feeding both the JSON sample (ToSample) and the OpenAPI Schema, so docs and spec stay derived from one API description. Keep `HasDocs` (Markdown) and `HasOpenApi` as two interpretations over the same combinator tree, mirroring servant-docs vs servant-swagger.

---

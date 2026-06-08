//! The typed API description: combinator types plus the traits that compute a
//! handler's argument list ([`HasArgs`]) and an endpoint's response shape
//! ([`Endpoint`]) from that description.
//!
//! Mirrors Servant's combinators (`:>`, `:<|>`, `Capture`, `QueryParam`,
//! `Header`, `ReqBody`, `Verb`, …). Two deliberate Rust differences:
//!
//! - The structure is encoded in the **type** while runtime strings (path
//!   segments, capture/parameter/header names) live in the **value**, because
//!   Rust cannot put `&'static str` literals in type position on stable.
//! - Combinators nest forward through their `next` field: `path("a",
//!   capture::<u64>("id", get::<(Json,), T>()))` is Servant's
//!   `"a" :> Capture "id" u64 :> Get '[JSON] T`.
//!
//! The combinator set is **sealed** (a closed world owned by this crate) so the
//! interpretation traits can be implemented for every combinator without
//! orphan-rule friction; codecs and value types stay open for extension.

use std::marker::PhantomData;
use std::net::SocketAddr;
use std::sync::Arc;

use http::{Extensions, Method, StatusCode, Version};

use crate::content::NoContent;
use crate::hlist::{HCons, HList, HNil};
use crate::method::MethodMarker;
use crate::modifiers::{ArgShape, CaptureShape, Lenient, Optional, Required, Strict};

mod sealed {
    pub trait Sealed {}
}

// ---------------------------------------------------------------------------
// Combinator types
// ---------------------------------------------------------------------------

/// A terminal endpoint: HTTP `method`, numeric `STATUS`, response content-type
/// list `CTypes`, and response value `A`. (`Get '[JSON] A` etc.)
pub struct Verb<M, const STATUS: u16, CTypes, A> {
    _marker: PhantomData<fn() -> (M, CTypes, A)>,
}

/// A body-less terminal endpoint, always responding `204 No Content`.
pub struct NoContentVerb<M> {
    _marker: PhantomData<fn() -> M>,
}

/// A terminal endpoint returning a *union* response (`UVerb method ctypes
/// '[..]`). The handler returns `Resp` (a [`crate::uverb::Union2`] etc. of
/// [`crate::uverb::WithStatus`] arms); the active arm's status and body are sent.
pub struct UVerb<M, CTypes, Resp> {
    _marker: PhantomData<fn() -> (M, CTypes, Resp)>,
}

/// A streaming terminal endpoint (`Stream method status framing ctype a`). The
/// handler returns a [`crate::stream::SourceStream<T>`]; each item is rendered as
/// `CType` and delimited by the `Framing` strategy. Server + docs only.
pub struct StreamVerb<M, const STATUS: u16, Framing, CType, T> {
    _marker: PhantomData<fn() -> (M, Framing, CType, T)>,
}

/// A terminal endpoint whose response carries extra headers. The handler returns
/// [`crate::response::Headers<A>`]; the body is content-negotiated over `CTypes`
/// (like [`Verb`]) and the headers are attached to the response. A distinct type
/// from [`Verb`] so its response-rendering does not collide with the plain path.
pub struct VerbWithHeaders<M, const STATUS: u16, CTypes, A> {
    _marker: PhantomData<fn() -> (M, CTypes, A)>,
}

/// A static path segment (`"users" :>`).
pub struct Path<Next> {
    /// The (already-decoded) literal segment.
    pub segment: String,
    /// The rest of the API under this segment.
    pub next: Next,
}

/// Capture a single path segment, parsed as `A` (`Capture "id" A :>`). `S`
/// selects strict vs lenient parsing.
pub struct Capture<A, S, Next> {
    /// Documentation/link name of the captured variable.
    pub name: String,
    /// The rest of the API.
    pub next: Next,
    _marker: PhantomData<fn() -> (A, S)>,
}

/// Capture all remaining path segments as `Vec<A>` (`CaptureAll "rest" A :>`).
pub struct CaptureAll<A, Next> {
    /// Documentation/link name.
    pub name: String,
    /// The rest of the API.
    pub next: Next,
    _marker: PhantomData<fn() -> A>,
}

/// A query parameter parsed as `A` (`QueryParam' mods "k" A :>`). `P` is
/// presence (default [`Optional`]), `S` is strictness (default [`Strict`]).
pub struct QueryParam<A, P, S, Next> {
    /// The query key.
    pub name: String,
    /// The rest of the API.
    pub next: Next,
    _marker: PhantomData<fn() -> (A, P, S)>,
}

/// Collect every value for a repeated query key as `Vec<A>` (`QueryParams`).
pub struct QueryParams<A, Next> {
    /// The query key.
    pub name: String,
    /// The rest of the API.
    pub next: Next,
    _marker: PhantomData<fn() -> A>,
}

/// A boolean query flag (`QueryFlag "k" :>`), `true` when the key is present.
pub struct QueryFlag<Next> {
    /// The query key.
    pub name: String,
    /// The rest of the API.
    pub next: Next,
}

/// A request header parsed as `A` (`Header' mods "H" A :>`). Same modifier
/// defaults as [`QueryParam`].
pub struct Header<A, P, S, Next> {
    /// The header name.
    pub name: String,
    /// The rest of the API.
    pub next: Next,
    _marker: PhantomData<fn() -> (A, P, S)>,
}

/// A request body of content types `CTypes` decoded as `A`
/// (`ReqBody' mods '[CTypes] A :>`). Presence is always required; `S` selects
/// strict vs lenient parsing.
pub struct ReqBody<CTypes, A, S, Next> {
    /// The rest of the API.
    pub next: Next,
    _marker: PhantomData<fn() -> (CTypes, A, S)>,
}

/// A left-biased union of two APIs (`a :<|> b`). On overlap the left wins.
pub struct Alt<L, R> {
    /// The higher-precedence (left) API.
    pub left: L,
    /// The lower-precedence (right) API.
    pub right: R,
}

/// An API that serves nothing — the unit of [`Alt`].
#[derive(Debug, Clone, Copy, Default)]
pub struct EmptyApi;

/// A documentation note attached to the API below it (no routing effect).
pub struct Description<Next> {
    /// The description text.
    pub text: String,
    /// The rest of the API.
    pub next: Next,
}

/// A short summary attached to the API below it (no routing effect).
pub struct Summary<Next> {
    /// The summary text.
    pub text: String,
    /// The rest of the API.
    pub next: Next,
}

/// Access the request's `http::Extensions` (the analogue of Servant's `Vault`):
/// per-request values set by middleware. Server-only; the handler receives
/// `Arc<http::Extensions>`.
pub struct Vault<Next> {
    /// The rest of the API.
    pub next: Next,
}

/// Allocate a per-request resource `R` from the server `Context` and pass it to
/// the handler (`WithResource res`). Server-only.
pub struct WithResource<R, Next> {
    /// The rest of the API.
    pub next: Next,
    _marker: PhantomData<fn() -> R>,
}

/// Protect the API below with HTTP Basic authentication. The server reads the
/// `Authorization` header, runs the `Context`-supplied check, and passes the
/// authenticated user `Usr` to the handler (or rejects with `401`). Server +
/// docs + links only (see [`crate::auth`]).
pub struct BasicAuth<Usr, Next> {
    /// The authentication realm (sent in `WWW-Authenticate`).
    pub realm: String,
    /// The rest of the API.
    pub next: Next,
    _marker: PhantomData<fn() -> Usr>,
}

/// Generalized authentication (`AuthProtect tag`): the server runs a
/// `Context`-supplied check over the request headers and passes the resolved
/// user `Usr` to the handler, or returns the check's error. Server-only.
pub struct AuthProtect<Usr, Next> {
    /// The rest of the API.
    pub next: Next,
    _marker: PhantomData<fn() -> Usr>,
}

/// Give the handler whether the connection is secure (TLS) — `IsSecure :>`.
/// Handler argument: `bool`. Server-only.
pub struct IsSecure<Next> {
    /// The rest of the API.
    pub next: Next,
}

/// Give the handler the request's HTTP version (`HttpVersion :>`). Handler
/// argument: [`http::Version`]. Server-only.
pub struct HttpVersion<Next> {
    /// The rest of the API.
    pub next: Next,
}

/// Give the handler the peer socket address (`RemoteHost :>`). Handler argument:
/// `Option<SocketAddr>` (`None` when unknown, e.g. in-process). Server-only.
pub struct RemoteHost<Next> {
    /// The rest of the API.
    pub next: Next,
}

/// Run the API below against a *named* sub-context selected from the parent
/// [`Context`](../../servant_server/context/struct.Context.html)
/// (`WithNamedContext name subctx :>`). Server-only; affects which context
/// inner `BasicAuth`/`AuthProtect`/`WithResource` combinators see.
pub struct WithNamedContext<Name, Next> {
    /// The rest of the API.
    pub next: Next,
    _marker: PhantomData<fn() -> Name>,
}

// ---------------------------------------------------------------------------
// Sealing
// ---------------------------------------------------------------------------

macro_rules! seal {
    ( $( $ty:ty ),* $(,)? ) => {
        $( impl sealed::Sealed for $ty {} )*
    };
}
seal!(EmptyApi);
impl<M, const STATUS: u16, CTypes, A> sealed::Sealed for Verb<M, STATUS, CTypes, A> {}
impl<M, const STATUS: u16, CTypes, A> sealed::Sealed for VerbWithHeaders<M, STATUS, CTypes, A> {}
impl<M, CTypes, Resp> sealed::Sealed for UVerb<M, CTypes, Resp> {}
impl<M, const STATUS: u16, Framing, CType, T> sealed::Sealed
    for StreamVerb<M, STATUS, Framing, CType, T>
{
}
impl<M> sealed::Sealed for NoContentVerb<M> {}
impl<Next> sealed::Sealed for Path<Next> {}
impl<A, S, Next> sealed::Sealed for Capture<A, S, Next> {}
impl<A, Next> sealed::Sealed for CaptureAll<A, Next> {}
impl<A, P, S, Next> sealed::Sealed for QueryParam<A, P, S, Next> {}
impl<A, Next> sealed::Sealed for QueryParams<A, Next> {}
impl<Next> sealed::Sealed for QueryFlag<Next> {}
impl<A, P, S, Next> sealed::Sealed for Header<A, P, S, Next> {}
impl<CTypes, A, S, Next> sealed::Sealed for ReqBody<CTypes, A, S, Next> {}
impl<L, R> sealed::Sealed for Alt<L, R> {}
impl<Next> sealed::Sealed for Description<Next> {}
impl<Next> sealed::Sealed for Summary<Next> {}
impl<Next> sealed::Sealed for Vault<Next> {}
impl<R, Next> sealed::Sealed for WithResource<R, Next> {}
impl<Usr, Next> sealed::Sealed for BasicAuth<Usr, Next> {}
impl<Usr, Next> sealed::Sealed for AuthProtect<Usr, Next> {}
impl<Next> sealed::Sealed for IsSecure<Next> {}
impl<Next> sealed::Sealed for HttpVersion<Next> {}
impl<Next> sealed::Sealed for RemoteHost<Next> {}
impl<Name, Next> sealed::Sealed for WithNamedContext<Name, Next> {}

// ---------------------------------------------------------------------------
// HasArgs — the handler argument list, accumulated as an HList
// ---------------------------------------------------------------------------

/// Computes the heterogeneous argument list a handler for this API fragment
/// must accept. Each value-extracting combinator prepends exactly one element
/// (its [`crate::modifiers::ArgShape`]-wrapped type) in left-to-right order;
/// structural and metadata combinators prepend nothing.
///
/// Sealed: only this crate's combinators implement it.
pub trait HasArgs: sealed::Sealed {
    /// The accumulated argument list.
    type Args: HList;
}

impl<M, const STATUS: u16, CTypes, A> HasArgs for Verb<M, STATUS, CTypes, A> {
    type Args = HNil;
}
impl<M> HasArgs for NoContentVerb<M> {
    type Args = HNil;
}
impl<M, const STATUS: u16, CTypes, A> HasArgs for VerbWithHeaders<M, STATUS, CTypes, A> {
    type Args = HNil;
}
impl<M, CTypes, Resp> HasArgs for UVerb<M, CTypes, Resp> {
    type Args = HNil;
}
impl<M, const STATUS: u16, Framing, CType, T> HasArgs for StreamVerb<M, STATUS, Framing, CType, T> {
    type Args = HNil;
}
impl<Next: HasArgs> HasArgs for Path<Next> {
    type Args = Next::Args;
}
impl<A, S, Next> HasArgs for Capture<A, S, Next>
where
    S: CaptureShape<A>,
    Next: HasArgs,
{
    type Args = HCons<<S as CaptureShape<A>>::Out, Next::Args>;
}
impl<A, Next: HasArgs> HasArgs for CaptureAll<A, Next> {
    type Args = HCons<Vec<A>, Next::Args>;
}
impl<A, P, S, Next> HasArgs for QueryParam<A, P, S, Next>
where
    (P, S): ArgShape<A>,
    Next: HasArgs,
{
    type Args = HCons<<(P, S) as ArgShape<A>>::Out, Next::Args>;
}
impl<A, Next: HasArgs> HasArgs for QueryParams<A, Next> {
    type Args = HCons<Vec<A>, Next::Args>;
}
impl<Next: HasArgs> HasArgs for QueryFlag<Next> {
    type Args = HCons<bool, Next::Args>;
}
impl<A, P, S, Next> HasArgs for Header<A, P, S, Next>
where
    (P, S): ArgShape<A>,
    Next: HasArgs,
{
    type Args = HCons<<(P, S) as ArgShape<A>>::Out, Next::Args>;
}
impl<CTypes, A, S, Next> HasArgs for ReqBody<CTypes, A, S, Next>
where
    (Required, S): ArgShape<A>,
    Next: HasArgs,
{
    type Args = HCons<<(Required, S) as ArgShape<A>>::Out, Next::Args>;
}
impl<Next: HasArgs> HasArgs for Description<Next> {
    type Args = Next::Args;
}
impl<Next: HasArgs> HasArgs for Summary<Next> {
    type Args = Next::Args;
}
impl<Next: HasArgs> HasArgs for Vault<Next> {
    type Args = HCons<Arc<Extensions>, Next::Args>;
}
impl<R, Next: HasArgs> HasArgs for WithResource<R, Next> {
    type Args = HCons<R, Next::Args>;
}
impl<Usr, Next: HasArgs> HasArgs for BasicAuth<Usr, Next> {
    type Args = HCons<Usr, Next::Args>;
}
impl<Usr, Next: HasArgs> HasArgs for AuthProtect<Usr, Next> {
    type Args = HCons<Usr, Next::Args>;
}
impl<Next: HasArgs> HasArgs for IsSecure<Next> {
    type Args = HCons<bool, Next::Args>;
}
impl<Next: HasArgs> HasArgs for HttpVersion<Next> {
    type Args = HCons<Version, Next::Args>;
}
impl<Next: HasArgs> HasArgs for RemoteHost<Next> {
    type Args = HCons<Option<SocketAddr>, Next::Args>;
}
impl<Name, Next: HasArgs> HasArgs for WithNamedContext<Name, Next> {
    type Args = Next::Args;
}

// ---------------------------------------------------------------------------
// Endpoint — terminal response info, recursed from the verb
// ---------------------------------------------------------------------------

/// A single endpoint chain (path/extractors ending in a verb). Exposes the
/// response value type, the response content-type list, and the runtime
/// method/status. (Not implemented for [`Alt`], which is a *set* of endpoints.)
pub trait Endpoint: HasArgs {
    /// The response value the handler returns.
    type Output;
    /// The response content-type list used for `Accept` negotiation.
    type ResponseTypes;
    /// The HTTP method.
    fn method(&self) -> Method;
    /// The HTTP success status.
    fn status(&self) -> StatusCode;
}

impl<M: MethodMarker, const STATUS: u16, CTypes, A> Endpoint for Verb<M, STATUS, CTypes, A> {
    type Output = A;
    type ResponseTypes = CTypes;
    fn method(&self) -> Method {
        M::method()
    }
    fn status(&self) -> StatusCode {
        StatusCode::from_u16(STATUS).expect("Verb STATUS must be a valid HTTP status code")
    }
}

impl<M: MethodMarker, const STATUS: u16, CTypes, A> Endpoint
    for VerbWithHeaders<M, STATUS, CTypes, A>
{
    type Output = crate::response::Headers<A>;
    type ResponseTypes = CTypes;
    fn method(&self) -> Method {
        M::method()
    }
    fn status(&self) -> StatusCode {
        StatusCode::from_u16(STATUS)
            .expect("VerbWithHeaders STATUS must be a valid HTTP status code")
    }
}

impl<M: MethodMarker, const STATUS: u16, Framing, CType, T> Endpoint
    for StreamVerb<M, STATUS, Framing, CType, T>
{
    type Output = crate::stream::SourceStream<T>;
    type ResponseTypes = (CType,);
    fn method(&self) -> Method {
        M::method()
    }
    fn status(&self) -> StatusCode {
        StatusCode::from_u16(STATUS).expect("StreamVerb STATUS must be a valid HTTP status code")
    }
}

impl<M: MethodMarker, CTypes, Resp> Endpoint for UVerb<M, CTypes, Resp> {
    type Output = Resp;
    type ResponseTypes = CTypes;
    fn method(&self) -> Method {
        M::method()
    }
    fn status(&self) -> StatusCode {
        // A union has no single status; the active arm supplies it at render
        // time. 200 is the nominal status used for docs/layout.
        StatusCode::OK
    }
}

impl<M: MethodMarker> Endpoint for NoContentVerb<M> {
    type Output = NoContent;
    /// No content negotiation occurs for a 204 response.
    type ResponseTypes = ();
    fn method(&self) -> Method {
        M::method()
    }
    fn status(&self) -> StatusCode {
        StatusCode::NO_CONTENT
    }
}

macro_rules! forward_endpoint {
    ($ty:ident < $($g:ident),+ >) => {
        impl<$($g),+, Next> Endpoint for $ty<$($g),+, Next>
        where
            Self: HasArgs,
            Next: Endpoint,
        {
            type Output = Next::Output;
            type ResponseTypes = Next::ResponseTypes;
            fn method(&self) -> Method { self.next.method() }
            fn status(&self) -> StatusCode { self.next.status() }
        }
    };
}

impl<Next> Endpoint for Path<Next>
where
    Self: HasArgs,
    Next: Endpoint,
{
    type Output = Next::Output;
    type ResponseTypes = Next::ResponseTypes;
    fn method(&self) -> Method {
        self.next.method()
    }
    fn status(&self) -> StatusCode {
        self.next.status()
    }
}
forward_endpoint!(Capture<A, S>);
forward_endpoint!(CaptureAll<A>);
forward_endpoint!(QueryParam<A, P, S>);
forward_endpoint!(QueryParams<A>);
forward_endpoint!(Header<A, P, S>);
forward_endpoint!(ReqBody<CTypes, A, S>);

// Combinators whose only type parameter is `Next` (the macro requires ≥1 extra
// generic, so these are written out).
macro_rules! forward_endpoint_unary {
    ($ty:ident) => {
        impl<Next> Endpoint for $ty<Next>
        where
            Self: HasArgs,
            Next: Endpoint,
        {
            type Output = Next::Output;
            type ResponseTypes = Next::ResponseTypes;
            fn method(&self) -> Method {
                self.next.method()
            }
            fn status(&self) -> StatusCode {
                self.next.status()
            }
        }
    };
}
forward_endpoint_unary!(QueryFlag);
forward_endpoint_unary!(Description);
forward_endpoint_unary!(Summary);
forward_endpoint_unary!(Vault);
forward_endpoint!(WithResource<R>);
forward_endpoint!(BasicAuth<Usr>);
forward_endpoint!(AuthProtect<Usr>);
forward_endpoint_unary!(IsSecure);
forward_endpoint_unary!(HttpVersion);
forward_endpoint_unary!(RemoteHost);
forward_endpoint!(WithNamedContext<Name>);

// ---------------------------------------------------------------------------
// Constructors (the forward-nesting builder surface)
// ---------------------------------------------------------------------------

/// `"segment" :>` — a static path component.
pub fn path<Next>(segment: impl Into<String>, next: Next) -> Path<Next> {
    Path {
        segment: segment.into(),
        next,
    }
}

/// `Capture "name" A :>` — a strictly-parsed single-segment capture.
pub fn capture<A, Next>(name: impl Into<String>, next: Next) -> Capture<A, Strict, Next> {
    Capture {
        name: name.into(),
        next,
        _marker: PhantomData,
    }
}

/// `Capture' '[Lenient] "name" A :>` — a leniently-parsed capture (parse
/// failures are surfaced as `Err` instead of failing the route).
pub fn capture_lenient<A, Next>(name: impl Into<String>, next: Next) -> Capture<A, Lenient, Next> {
    Capture {
        name: name.into(),
        next,
        _marker: PhantomData,
    }
}

/// `CaptureAll "name" A :>` — all remaining segments as `Vec<A>`.
pub fn capture_all<A, Next>(name: impl Into<String>, next: Next) -> CaptureAll<A, Next> {
    CaptureAll {
        name: name.into(),
        next,
        _marker: PhantomData,
    }
}

/// `QueryParam "name" A :>` — optional, strict by default.
pub fn query_param<A, Next>(
    name: impl Into<String>,
    next: Next,
) -> QueryParam<A, Optional, Strict, Next> {
    QueryParam {
        name: name.into(),
        next,
        _marker: PhantomData,
    }
}

/// `QueryParams "name" A :>` — every value for a repeated key as `Vec<A>`.
pub fn query_params<A, Next>(name: impl Into<String>, next: Next) -> QueryParams<A, Next> {
    QueryParams {
        name: name.into(),
        next,
        _marker: PhantomData,
    }
}

/// `QueryFlag "name" :>` — a boolean flag.
pub fn query_flag<Next>(name: impl Into<String>, next: Next) -> QueryFlag<Next> {
    QueryFlag {
        name: name.into(),
        next,
    }
}

/// `Header "name" A :>` — optional, strict by default.
pub fn header<A, Next>(name: impl Into<String>, next: Next) -> Header<A, Optional, Strict, Next> {
    Header {
        name: name.into(),
        next,
        _marker: PhantomData,
    }
}

/// `ReqBody '[CTypes] A :>` — required, strict by default.
pub fn req_body<CTypes, A, Next>(next: Next) -> ReqBody<CTypes, A, Strict, Next> {
    ReqBody {
        next,
        _marker: PhantomData,
    }
}

/// `Get '[CTypes] A` — `GET`, status 200.
pub fn get<CTypes, A>() -> Verb<crate::method::Get, 200, CTypes, A> {
    Verb {
        _marker: PhantomData,
    }
}

/// `Post '[CTypes] A` — `POST`, status 200.
pub fn post<CTypes, A>() -> Verb<crate::method::Post, 200, CTypes, A> {
    Verb {
        _marker: PhantomData,
    }
}

/// `Verb 'M STATUS '[CTypes] A` — an arbitrary method/status endpoint.
pub fn verb<M, const STATUS: u16, CTypes, A>() -> Verb<M, STATUS, CTypes, A> {
    Verb {
        _marker: PhantomData,
    }
}

/// `NoContentVerb 'M` — a `204 No Content` endpoint.
pub fn no_content<M>() -> NoContentVerb<M> {
    NoContentVerb {
        _marker: PhantomData,
    }
}

/// `UVerb 'M '[CTypes] '[Resp]` — a union response (e.g. `200 A | 404 B`).
pub fn uverb<M, CTypes, Resp>() -> UVerb<M, CTypes, Resp> {
    UVerb {
        _marker: PhantomData,
    }
}

/// `StreamGet framing ctype a` — a `GET` streaming `T` items framed by `Framing`.
pub fn stream_get<Framing, CType, T>() -> StreamVerb<crate::method::Get, 200, Framing, CType, T> {
    StreamVerb {
        _marker: PhantomData,
    }
}

/// `Stream 'M status framing ctype a` — an arbitrary-method streaming endpoint.
pub fn stream_verb<M, const STATUS: u16, Framing, CType, T>()
-> StreamVerb<M, STATUS, Framing, CType, T> {
    StreamVerb {
        _marker: PhantomData,
    }
}

/// `ServerSentEvents` — a `GET` streaming [`crate::stream::ServerEvent`]s as
/// `text/event-stream`.
pub fn sse_get() -> StreamVerb<
    crate::method::Get,
    200,
    crate::stream::NoFraming,
    crate::content::EventStream,
    crate::stream::ServerEvent,
> {
    StreamVerb {
        _marker: PhantomData,
    }
}

/// `Get '[CTypes] (Headers h A)` — a `GET` returning `A` plus response headers.
pub fn get_with_headers<CTypes, A>() -> VerbWithHeaders<crate::method::Get, 200, CTypes, A> {
    VerbWithHeaders {
        _marker: PhantomData,
    }
}

/// `Verb 'M STATUS '[CTypes] (Headers h A)` — a response with extra headers.
pub fn verb_with_headers<M, const STATUS: u16, CTypes, A>() -> VerbWithHeaders<M, STATUS, CTypes, A>
{
    VerbWithHeaders {
        _marker: PhantomData,
    }
}

/// `a :<|> b` — a left-biased alternative.
pub fn alt<L, R>(left: L, right: R) -> Alt<L, R> {
    Alt { left, right }
}

/// Combine N endpoints into a right-nested [`Alt`] tree (the analogue of
/// Servant's named-routes record): `alt_all![a, b, c]` ≡ `alt(a, alt(b, c))`.
/// Pair it with [`handlers!`] — both nest identically, so the handler tuple
/// always matches the API structure regardless of how many routes there are.
#[macro_export]
macro_rules! alt_all {
    ($single:expr $(,)?) => { $single };
    ($head:expr, $($rest:expr),+ $(,)?) => {
        $crate::api::alt($head, $crate::alt_all!($($rest),+))
    };
}

/// Build a right-nested handler tuple matching an [`alt_all!`] API:
/// `handlers![h1, h2, h3]` ≡ `(h1, (h2, h3))`.
#[macro_export]
macro_rules! handlers {
    ($single:expr $(,)?) => { $single };
    ($head:expr, $($rest:expr),+ $(,)?) => {
        ($head, $crate::handlers!($($rest),+))
    };
}

/// Attach a `Description` to the API below.
pub fn description<Next>(text: impl Into<String>, next: Next) -> Description<Next> {
    Description {
        text: text.into(),
        next,
    }
}

/// Attach a `Summary` to the API below.
pub fn summary<Next>(text: impl Into<String>, next: Next) -> Summary<Next> {
    Summary {
        text: text.into(),
        next,
    }
}

/// `Vault :>` — give the handler access to the request's `http::Extensions`.
pub fn vault<Next>(next: Next) -> Vault<Next> {
    Vault { next }
}

/// `WithResource res :>` — allocate `R` per request from the server context.
pub fn with_resource<R, Next>(next: Next) -> WithResource<R, Next> {
    WithResource {
        next,
        _marker: PhantomData,
    }
}

/// `BasicAuth realm usr :>` — HTTP Basic auth resolving a user of type `Usr`.
pub fn basic_auth<Usr, Next>(realm: impl Into<String>, next: Next) -> BasicAuth<Usr, Next> {
    BasicAuth {
        realm: realm.into(),
        next,
        _marker: PhantomData,
    }
}

/// `AuthProtect usr :>` — generalized auth resolving a user of type `Usr` via a
/// context-supplied check.
pub fn auth_protect<Usr, Next>(next: Next) -> AuthProtect<Usr, Next> {
    AuthProtect {
        next,
        _marker: PhantomData,
    }
}

/// `IsSecure :>` — pass the connection's secure flag (`bool`) to the handler.
pub fn is_secure<Next>(next: Next) -> IsSecure<Next> {
    IsSecure { next }
}

/// `HttpVersion :>` — pass the request's [`http::Version`] to the handler.
pub fn http_version<Next>(next: Next) -> HttpVersion<Next> {
    HttpVersion { next }
}

/// `RemoteHost :>` — pass the peer `Option<SocketAddr>` to the handler.
pub fn remote_host<Next>(next: Next) -> RemoteHost<Next> {
    RemoteHost { next }
}

/// `WithNamedContext name subctx :>` — run the inner API against a named
/// sub-context. `Name` is a user marker type keying the sub-context.
pub fn with_named_context<Name, Next>(next: Next) -> WithNamedContext<Name, Next> {
    WithNamedContext {
        next,
        _marker: PhantomData,
    }
}

// ---------------------------------------------------------------------------
// Modifier builder methods (last-wins via ordinary method chaining)
// ---------------------------------------------------------------------------

impl<A, P, S, Next> QueryParam<A, P, S, Next> {
    /// Make the parameter required (absence rejects the request).
    pub fn required(self) -> QueryParam<A, Required, S, Next> {
        QueryParam {
            name: self.name,
            next: self.next,
            _marker: PhantomData,
        }
    }
    /// Make the parameter optional (absence yields `None`).
    pub fn optional(self) -> QueryParam<A, Optional, S, Next> {
        QueryParam {
            name: self.name,
            next: self.next,
            _marker: PhantomData,
        }
    }
    /// Reject the request on a parse failure (strict).
    pub fn strict(self) -> QueryParam<A, P, Strict, Next> {
        QueryParam {
            name: self.name,
            next: self.next,
            _marker: PhantomData,
        }
    }
    /// Surface parse failures to the handler as `Err` (lenient).
    pub fn lenient(self) -> QueryParam<A, P, Lenient, Next> {
        QueryParam {
            name: self.name,
            next: self.next,
            _marker: PhantomData,
        }
    }
}

impl<A, P, S, Next> Header<A, P, S, Next> {
    /// Make the header required.
    pub fn required(self) -> Header<A, Required, S, Next> {
        Header {
            name: self.name,
            next: self.next,
            _marker: PhantomData,
        }
    }
    /// Make the header optional.
    pub fn optional(self) -> Header<A, Optional, S, Next> {
        Header {
            name: self.name,
            next: self.next,
            _marker: PhantomData,
        }
    }
    /// Reject the request on a parse failure (strict).
    pub fn strict(self) -> Header<A, P, Strict, Next> {
        Header {
            name: self.name,
            next: self.next,
            _marker: PhantomData,
        }
    }
    /// Surface parse failures to the handler as `Err` (lenient).
    pub fn lenient(self) -> Header<A, P, Lenient, Next> {
        Header {
            name: self.name,
            next: self.next,
            _marker: PhantomData,
        }
    }
}

impl<CTypes, A, S, Next> ReqBody<CTypes, A, S, Next> {
    /// Reject the request on a body parse failure (strict).
    pub fn strict(self) -> ReqBody<CTypes, A, Strict, Next> {
        ReqBody {
            next: self.next,
            _marker: PhantomData,
        }
    }
    /// Surface a body parse failure to the handler as `Err` (lenient).
    pub fn lenient(self) -> ReqBody<CTypes, A, Lenient, Next> {
        ReqBody {
            next: self.next,
            _marker: PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::Json;
    use crate::hlist::HList;
    use crate::modifiers::ParseError;

    // A compile-time assertion that two types are equal.
    fn assert_args<E: HasArgs>(_e: &E, _expected: E::Args)
    where
        E::Args: Sized,
    {
    }

    #[test]
    fn arg_list_order_and_shapes() {
        // "users" :> Capture u64 :> QueryParam<String> :> Get
        let api = path(
            "users",
            capture::<u64, _>("id", query_param::<String, _>("q", get::<(Json,), u8>())),
        );
        // Args = HCons<u64, HCons<Option<String>, HNil>>
        assert_args(
            &api,
            HCons {
                head: 7u64,
                tail: HCons {
                    head: Some("x".to_string()),
                    tail: HNil,
                },
            },
        );
        type Api = Path<
            Capture<
                u64,
                Strict,
                QueryParam<String, Optional, Strict, Verb<crate::method::Get, 200, (Json,), u8>>,
            >,
        >;
        assert_eq!(<<Api as HasArgs>::Args as HList>::LEN, 2);
        assert_eq!(api.method(), Method::GET);
        assert_eq!(api.status(), StatusCode::OK);
    }

    #[test]
    fn required_lenient_query_changes_arg_shape() {
        // Last-wins: optional default -> required -> lenient => Result wrapped, required
        let api = path(
            "x",
            query_param::<u32, _>("n", get::<(Json,), u8>())
                .required()
                .lenient(),
        );
        // Args head should be Result<u32, ParseError>
        assert_args(
            &api,
            HCons {
                head: Ok::<u32, ParseError>(1),
                tail: HNil,
            },
        );
    }

    #[test]
    fn capture_lenient_wraps_result() {
        let api = capture_lenient::<u32, _>("id", get::<(Json,), u8>());
        assert_args(
            &api,
            HCons {
                head: Ok::<u32, ParseError>(1),
                tail: HNil,
            },
        );
    }

    #[test]
    fn verb_status_and_method() {
        let e = verb::<crate::method::Post, 201, (Json,), u8>();
        assert_eq!(e.method(), Method::POST);
        assert_eq!(e.status(), StatusCode::CREATED);
        let n = no_content::<crate::method::Delete>();
        assert_eq!(n.status(), StatusCode::NO_CONTENT);
    }
}

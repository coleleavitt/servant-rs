use std::marker::PhantomData;

use super::sealed;

/// A terminal endpoint: HTTP `method`, numeric `STATUS`, response content-type
/// list `CTypes`, and response value `A`. (`Get '[JSON] A` etc.)
pub struct Verb<M, const STATUS: u16, CTypes, A> {
    pub(crate) _marker: PhantomData<fn() -> (M, CTypes, A)>,
}

/// A body-less terminal endpoint, always responding `204 No Content`.
pub struct NoContentVerb<M> {
    pub(crate) _marker: PhantomData<fn() -> M>,
}

/// A terminal endpoint returning a *union* response (`UVerb method ctypes
/// '[..]`). The handler returns `Resp` (a [`crate::uverb::Union2`] etc. of
/// [`crate::uverb::WithStatus`] arms); the active arm's status and body are sent.
pub struct UVerb<M, CTypes, Resp> {
    pub(crate) _marker: PhantomData<fn() -> (M, CTypes, Resp)>,
}

/// A streaming terminal endpoint (`Stream method status framing ctype a`). The
/// handler returns a [`crate::stream::SourceStream<T>`]; each item is rendered as
/// `CType` and delimited by the `Framing` strategy. Server + docs only.
pub struct StreamVerb<M, const STATUS: u16, Framing, CType, T> {
    pub(crate) _marker: PhantomData<fn() -> (M, Framing, CType, T)>,
}

/// A terminal endpoint whose response carries extra headers. The handler returns
/// [`crate::response::Headers<A>`]; the body is content-negotiated over `CTypes`
/// (like [`Verb`]) and the headers are attached to the response. A distinct type
/// from [`Verb`] so its response-rendering does not collide with the plain path.
pub struct VerbWithHeaders<M, const STATUS: u16, CTypes, A> {
    pub(crate) _marker: PhantomData<fn() -> (M, CTypes, A)>,
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
    pub(crate) _marker: PhantomData<fn() -> (A, S)>,
}

/// Capture all remaining path segments as `Vec<A>` (`CaptureAll "rest" A :>`).
pub struct CaptureAll<A, Next> {
    /// Documentation/link name.
    pub name: String,
    /// The rest of the API.
    pub next: Next,
    pub(crate) _marker: PhantomData<fn() -> A>,
}

/// A query parameter parsed as `A` (`QueryParam' mods "k" A :>`). `P` is
/// presence (default [`crate::modifiers::Optional`]), `S` is strictness (default
/// [`crate::modifiers::Strict`]).
pub struct QueryParam<A, P, S, Next> {
    /// The query key.
    pub name: String,
    /// The rest of the API.
    pub next: Next,
    pub(crate) _marker: PhantomData<fn() -> (A, P, S)>,
}

/// Collect every value for a repeated query key as `Vec<A>` (`QueryParams`).
pub struct QueryParams<A, Next> {
    /// The query key.
    pub name: String,
    /// The rest of the API.
    pub next: Next,
    pub(crate) _marker: PhantomData<fn() -> A>,
}

/// A boolean query flag (`QueryFlag "k" :>`), `true` when the key is present.
pub struct QueryFlag<Next> {
    /// The query key.
    pub name: String,
    /// The rest of the API.
    pub next: Next,
}

/// Capture the entire query string as [`crate::query::Query`].
///
/// Unlike [`QueryParam`] and [`QueryParams`], this combinator is for dynamic
/// query shapes. It preserves the decoded ordered pairs and, on server input,
/// the original raw query string.
pub struct QueryString<Next> {
    /// The rest of the API.
    pub next: Next,
}

/// Extract a nested deep-object query parameter as `A`.
pub struct DeepQuery<A, Next> {
    /// The root query key, e.g. `filter` for `filter[name]=...`.
    pub name: String,
    /// The rest of the API.
    pub next: Next,
    pub(crate) _marker: PhantomData<fn() -> A>,
}

/// A request header parsed as `A` (`Header' mods "H" A :>`). Same modifier
/// defaults as [`QueryParam`].
pub struct Header<A, P, S, Next> {
    /// The header name.
    pub name: String,
    /// The rest of the API.
    pub next: Next,
    pub(crate) _marker: PhantomData<fn() -> (A, P, S)>,
}

/// Match the request `Host` header or URI authority before serving the API
/// below it. The combinator contributes no handler argument.
pub struct Host<Next> {
    /// The required host authority, optionally including `:port`.
    pub name: String,
    /// The rest of the API.
    pub next: Next,
}

/// A request body of content types `CTypes` decoded as `A`
/// (`ReqBody' mods '[CTypes] A :>`). Presence is always required; `S` selects
/// strict vs lenient parsing.
pub struct ReqBody<CTypes, A, S, Next> {
    /// The rest of the API.
    pub next: Next,
    pub(crate) _marker: PhantomData<fn() -> (CTypes, A, S)>,
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

/// A stable operation identifier attached to the API below it.
///
/// It has no routing, handler, link, or client effect. Documentation and
/// OpenAPI interpretations surface it as endpoint metadata.
pub struct OperationId<Next> {
    /// The operation identifier.
    pub id: String,
    /// The rest of the API.
    pub next: Next,
}

/// URI fragment metadata for the API below it.
///
/// Server routing and generated clients ignore this combinator because URI
/// fragments are client-side. Safe links consume one value of type `A` and
/// render it after `#`; Markdown docs record `description`.
pub struct Fragment<A, Next> {
    /// Human-readable fragment description for generated docs.
    pub description: String,
    /// The rest of the API.
    pub next: Next,
    pub(crate) _marker: PhantomData<fn() -> A>,
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
    pub(crate) _marker: PhantomData<fn() -> R>,
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
    pub(crate) _marker: PhantomData<fn() -> Usr>,
}

/// Generalized authentication (`AuthProtect tag`): the server runs a
/// `Context`-supplied check over the request headers and passes the resolved
/// user `Usr` to the handler, or returns the check's error. Server-only.
pub struct AuthProtect<Usr, Next> {
    /// The rest of the API.
    pub next: Next,
    pub(crate) _marker: PhantomData<fn() -> Usr>,
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
    pub(crate) _marker: PhantomData<fn() -> Name>,
}

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
impl<Next> sealed::Sealed for QueryString<Next> {}
impl<A, Next> sealed::Sealed for DeepQuery<A, Next> {}
impl<A, P, S, Next> sealed::Sealed for Header<A, P, S, Next> {}
impl<Next> sealed::Sealed for Host<Next> {}
impl<CTypes, A, S, Next> sealed::Sealed for ReqBody<CTypes, A, S, Next> {}
impl<L, R> sealed::Sealed for Alt<L, R> {}
impl<Next> sealed::Sealed for Description<Next> {}
impl<Next> sealed::Sealed for Summary<Next> {}
impl<Next> sealed::Sealed for OperationId<Next> {}
impl<A, Next> sealed::Sealed for Fragment<A, Next> {}
impl<Next> sealed::Sealed for Vault<Next> {}
impl<R, Next> sealed::Sealed for WithResource<R, Next> {}
impl<Usr, Next> sealed::Sealed for BasicAuth<Usr, Next> {}
impl<Usr, Next> sealed::Sealed for AuthProtect<Usr, Next> {}
impl<Next> sealed::Sealed for IsSecure<Next> {}
impl<Next> sealed::Sealed for HttpVersion<Next> {}
impl<Next> sealed::Sealed for RemoteHost<Next> {}
impl<Name, Next> sealed::Sealed for WithNamedContext<Name, Next> {}

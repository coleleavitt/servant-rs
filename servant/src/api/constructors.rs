use std::marker::PhantomData;

use super::combinators::*;
use crate::modifiers::{Lenient, Optional, Strict};

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

/// `QueryString :>` — the full query string as [`crate::query::Query`].
pub fn query_string<Next>(next: Next) -> QueryString<Next> {
    QueryString { next }
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
    crate::stream::EventStreamFraming,
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
/// Pair it with [`crate::handlers!`] — both nest identically, so the handler tuple
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

/// Attach an `OperationId` to the API below.
pub fn operation_id<Next>(id: impl Into<String>, next: Next) -> OperationId<Next> {
    OperationId {
        id: id.into(),
        next,
    }
}

/// Attach URI fragment metadata to the API below.
pub fn fragment<A, Next>(description: impl Into<String>, next: Next) -> Fragment<A, Next> {
    Fragment {
        description: description.into(),
        next,
        _marker: PhantomData,
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

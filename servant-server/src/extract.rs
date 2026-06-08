//! Request extraction: the `ServerChain` trait and its per-combinator impls.
//!
//! Mirrors Servant's `Delayed` pipeline. The leaf (see [`crate::handler`]) runs
//! the phases in Servant's order — captures → method → accept → content-type →
//! (query/header/body) — short-circuiting on the first failure, with the
//! `Fail`/`FailFatal` distinction per combinator:
//!
//! - **`Fail`** (recoverable): capture parse failure, content-type unsupported.
//! - **`FailFatal`** (commit): missing-required/strict-parse of query, header,
//!   or body (all 400).
//!
//! See `docs/DESIGN.md` for the one documented ordering difference: capture
//! parse validation runs at the leaf via [`ServerChain::validate_captures`]
//! (phase 1, before method), and query/header/body extraction follows
//! combinator order rather than strict query→header→body grouping.

use std::sync::Arc;

use base64::Engine;
use bytes::Bytes;
use http::{Extensions, StatusCode};
use mime::Mime;
use servant::api::{
    AuthProtect,
    BasicAuth,
    Capture,
    CaptureAll,
    Description,
    Endpoint,
    Header,
    HttpVersion,
    IsSecure,
    NoContentVerb,
    Path,
    QueryFlag,
    QueryParam,
    QueryParams,
    RemoteHost,
    ReqBody,
    Summary,
    Vault,
    Verb,
    WithNamedContext,
    WithResource,
};
use servant::auth::{BasicAuthData, BasicAuthResult};
use servant::content::{
    AllMime,
    AllMimeRender,
    AllMimeUnrender,
    MediaType,
    MimeRender,
    NoContent,
    media_type_matches,
    negotiate_media_index,
};
use servant::error::ServerError;
use servant::hlist::{HCons, HNil};
use servant::http_data::FromHttpApiData;
use servant::method::MethodMarker;
use servant::modifiers::{ArgError, ArgShape, CaptureShape, ParseError, Required};
use servant::stream::{Framing, SourceStream};
use servant::uverb::UnionResponse;

use crate::request::RequestData;
use crate::result::RouteResult;

/// Mutable cursor over the request while extracting an endpoint's arguments.
pub struct ExtractState<'a> {
    captures: std::vec::IntoIter<String>,
    capture_all: Option<Vec<String>>,
    req: &'a RequestData,
    /// Stack of contexts: the base context at the bottom, named sub-contexts
    /// (from `WithNamedContext`) pushed on top; lookups search top-down.
    contexts: Vec<&'a crate::context::Context>,
}

impl<'a> ExtractState<'a> {
    /// Build extraction state from the collected captures, the request, and the
    /// server context.
    pub fn new(
        captures: Vec<String>,
        capture_all: Option<Vec<String>>,
        req: &'a RequestData,
        ctx: &'a crate::context::Context,
    ) -> Self {
        ExtractState {
            captures: captures.into_iter(),
            capture_all,
            req,
            contexts: vec![ctx],
        }
    }

    fn next_capture(&mut self) -> Option<String> {
        self.captures.next()
    }

    fn take_capture_all(&mut self) -> Vec<String> {
        self.capture_all.take().unwrap_or_default()
    }

    /// Look up a context entry, searching pushed named sub-contexts first.
    fn lookup_ctx<T: std::any::Any + Send + Sync>(&self) -> Option<&'a T> {
        self.contexts
            .iter()
            .rev()
            .copied()
            .find_map(|c| c.get::<T>())
    }

    fn push_ctx(&mut self, ctx: &'a crate::context::Context) {
        self.contexts.push(ctx);
    }

    fn pop_ctx(&mut self) {
        self.contexts.pop();
    }
}

fn bad_request(msg: impl Into<String>) -> ServerError {
    ServerError::err400().with_body(msg.into())
}

/// Server-side interpretation of an endpoint chain: capture validation,
/// content-type checking, response negotiation/rendering, and argument
/// extraction. Implemented for every endpoint combinator, recursing to the verb.
pub trait ServerChain: Endpoint {
    /// Phase 1: parse every capture (strict failures are recoverable `Fail`);
    /// `idx` walks the collected single-segment captures in order.
    fn validate_captures(
        &self,
        caps: &[String],
        idx: &mut usize,
        capture_all: &Option<Vec<String>>,
    ) -> RouteResult<()>;

    /// The request body's accepted media types, if this chain has a `ReqBody`
    /// (for the phase-5 415 check). `None` means no body is expected.
    fn request_content_types(&self) -> Option<Vec<Mime>>;

    /// Phase 4: 406 check — is any response content type acceptable?
    fn accept_check(&self, accept: Option<&str>) -> RouteResult<()>;

    /// Render the handler's output, negotiating the response content type.
    fn render(&self, accept: Option<&str>, value: Self::Output) -> Rendered;

    /// Phases 1/6/7/8: extract the full argument list in combinator order.
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args>;
}

/// A fully rendered response: status, optional `Content-Type`, body (buffered
/// or streaming), and any extra response headers (from `VerbWithHeaders`).
pub type Rendered = (
    StatusCode,
    Option<Mime>,
    crate::response::ResponseBody,
    Vec<(http::HeaderName, http::HeaderValue)>,
);

// --- Terminal: Verb ---

impl<M, const STATUS: u16, CTypes, A> ServerChain for Verb<M, STATUS, CTypes, A>
where
    M: MethodMarker,
    CTypes: AllMime + AllMimeRender<A>,
{
    fn validate_captures(
        &self,
        _: &[String],
        _: &mut usize,
        _: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        RouteResult::Route(())
    }
    fn request_content_types(&self) -> Option<Vec<Mime>> {
        None
    }
    fn accept_check(&self, accept: Option<&str>) -> RouteResult<()> {
        let media = CTypes::all_media_types();
        match negotiate_media_index(accept, &media) {
            Some(_) => RouteResult::Route(()),
            None => RouteResult::Fail(ServerError::err406()),
        }
    }
    fn render(&self, accept: Option<&str>, value: A) -> Rendered {
        let status =
            StatusCode::from_u16(STATUS).expect("Verb STATUS must be a valid HTTP status code");
        render_body::<CTypes, A>(accept, &value, status)
    }
    fn extract(&self, _st: &mut ExtractState<'_>) -> RouteResult<HNil> {
        RouteResult::Route(HNil)
    }
}

/// Negotiate + render a value over content-type list `L`, with no extra headers.
/// Renders only the negotiated representation (a serialization failure becomes a
/// clean `500`, and a failure in an *unused* format can't 500 a serviceable
/// request).
fn render_body<L: AllMime + AllMimeRender<A>, A>(
    accept: Option<&str>,
    value: &A,
    status: StatusCode,
) -> Rendered {
    let media = L::all_media_types();
    let idx = negotiate_media_index(accept, &media)
        .expect("accept_check guarantees a negotiable content type");
    match L::render_index(value, idx) {
        Ok(bytes) => (
            status,
            Some(media[idx].clone()),
            crate::response::full_body(bytes),
            Vec::new(),
        ),
        Err(_msg) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            None,
            crate::response::full_body(Bytes::from_static(b"internal serialization error")),
            Vec::new(),
        ),
    }
}

// --- Terminal: VerbWithHeaders ---

impl<M, const STATUS: u16, CTypes, A> ServerChain
    for servant::api::VerbWithHeaders<M, STATUS, CTypes, A>
where
    M: MethodMarker,
    CTypes: AllMime + AllMimeRender<A>,
{
    fn validate_captures(
        &self,
        _: &[String],
        _: &mut usize,
        _: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        RouteResult::Route(())
    }
    fn request_content_types(&self) -> Option<Vec<Mime>> {
        None
    }
    fn accept_check(&self, accept: Option<&str>) -> RouteResult<()> {
        let media = CTypes::all_media_types();
        match negotiate_media_index(accept, &media) {
            Some(_) => RouteResult::Route(()),
            None => RouteResult::Fail(ServerError::err406()),
        }
    }
    fn render(&self, accept: Option<&str>, value: servant::response::Headers<A>) -> Rendered {
        let status = StatusCode::from_u16(STATUS)
            .expect("VerbWithHeaders STATUS must be a valid HTTP status code");
        let (inner, headers) = value.into_parts();
        let (status, ct, body, _) = render_body::<CTypes, A>(accept, &inner, status);
        (status, ct, body, headers)
    }
    fn extract(&self, _st: &mut ExtractState<'_>) -> RouteResult<HNil> {
        RouteResult::Route(HNil)
    }
}

// --- Terminal: UVerb (union response) ---

impl<M, CTypes, Resp> ServerChain for servant::api::UVerb<M, CTypes, Resp>
where
    M: MethodMarker,
    CTypes: AllMime,
    Resp: UnionResponse<CTypes>,
    Self: Endpoint<Output = Resp, Args = HNil>,
{
    fn validate_captures(
        &self,
        _: &[String],
        _: &mut usize,
        _: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        RouteResult::Route(())
    }
    fn request_content_types(&self) -> Option<Vec<Mime>> {
        None
    }
    fn accept_check(&self, accept: Option<&str>) -> RouteResult<()> {
        let media = CTypes::all_media_types();
        match negotiate_media_index(accept, &media) {
            Some(_) => RouteResult::Route(()),
            None => RouteResult::Fail(ServerError::err406()),
        }
    }
    fn render(&self, accept: Option<&str>, value: Resp) -> Rendered {
        match value.render_union(accept) {
            Ok((status, mime, body, headers)) => {
                (status, mime, crate::response::full_body(body), headers)
            }
            Err(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                None,
                crate::response::full_body(Bytes::from_static(b"internal serialization error")),
                Vec::new(),
            ),
        }
    }
    fn extract(&self, _st: &mut ExtractState<'_>) -> RouteResult<HNil> {
        RouteResult::Route(HNil)
    }
}

// --- Terminal: StreamVerb (chunked streaming body) ---

impl<M, const STATUS: u16, Fr, CType, T> ServerChain
    for servant::api::StreamVerb<M, STATUS, Fr, CType, T>
where
    M: MethodMarker,
    Fr: Framing + 'static,
    CType: MediaType,
    T: MimeRender<CType> + Send + 'static,
    Self: Endpoint<Output = SourceStream<T>, Args = HNil>,
{
    fn validate_captures(
        &self,
        _: &[String],
        _: &mut usize,
        _: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        RouteResult::Route(())
    }
    fn request_content_types(&self) -> Option<Vec<Mime>> {
        None
    }
    fn accept_check(&self, accept: Option<&str>) -> RouteResult<()> {
        let media = [CType::media_type()];
        match negotiate_media_index(accept, &media) {
            Some(_) => RouteResult::Route(()),
            None => RouteResult::Fail(ServerError::err406()),
        }
    }
    fn render(&self, _accept: Option<&str>, value: SourceStream<T>) -> Rendered {
        use futures_util::StreamExt;
        use http_body::Frame;
        use http_body_util::{BodyExt, StreamBody};

        let status = StatusCode::from_u16(STATUS).expect("StreamVerb STATUS must be valid");
        let ct = CType::media_type();
        let framed =
            value
                .into_inner()
                .map(|item| match <T as MimeRender<CType>>::mime_render(&item) {
                    Ok(bytes) => Ok::<_, crate::response::BoxError>(Frame::data(Fr::frame(&bytes))),
                    Err(e) => Err(e.into()),
                });
        let body = BodyExt::boxed_unsync(StreamBody::new(framed));
        (status, Some(ct), body, Vec::new())
    }
    fn extract(&self, _st: &mut ExtractState<'_>) -> RouteResult<HNil> {
        RouteResult::Route(HNil)
    }
}

// --- Terminal: NoContentVerb ---

impl<M> ServerChain for NoContentVerb<M>
where
    M: MethodMarker,
{
    fn validate_captures(
        &self,
        _: &[String],
        _: &mut usize,
        _: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        RouteResult::Route(())
    }
    fn request_content_types(&self) -> Option<Vec<Mime>> {
        None
    }
    fn accept_check(&self, _accept: Option<&str>) -> RouteResult<()> {
        RouteResult::Route(()) // 204 performs no content negotiation
    }
    fn render(&self, _accept: Option<&str>, _value: NoContent) -> Rendered {
        (
            StatusCode::NO_CONTENT,
            None,
            crate::response::full_body(Bytes::new()),
            Vec::new(),
        )
    }
    fn extract(&self, _st: &mut ExtractState<'_>) -> RouteResult<HNil> {
        RouteResult::Route(HNil)
    }
}

// --- helpers for forwarding combinators (no response/body effect) ---

macro_rules! forward_response_checks {
    () => {
        fn request_content_types(&self) -> Option<Vec<Mime>> {
            self.next.request_content_types()
        }
        fn accept_check(&self, accept: Option<&str>) -> RouteResult<()> {
            self.next.accept_check(accept)
        }
        fn render(&self, accept: Option<&str>, value: Self::Output) -> Rendered {
            self.next.render(accept, value)
        }
    };
}

// Helper to thread the tail extraction after a head value.
fn cons_tail<H, Next: ServerChain>(
    head: H,
    next: &Next,
    st: &mut ExtractState<'_>,
) -> RouteResult<HCons<H, Next::Args>> {
    match next.extract(st) {
        RouteResult::Route(tail) => RouteResult::Route(HCons { head, tail }),
        RouteResult::Fail(e) => RouteResult::Fail(e),
        RouteResult::FailFatal(e) => RouteResult::FailFatal(e),
    }
}

// --- Path (no arg, no capture) ---

impl<Next: ServerChain> ServerChain for Path<Next>
where
    Self: Endpoint<Output = Next::Output, Args = Next::Args>,
{
    fn validate_captures(
        &self,
        c: &[String],
        i: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        self.next.validate_captures(c, i, ca)
    }
    forward_response_checks!();
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        self.next.extract(st)
    }
}

// --- Description / Summary (metadata) ---

macro_rules! metadata_chain {
    ($ty:ident) => {
        impl<Next: ServerChain> ServerChain for $ty<Next>
        where
            Self: Endpoint<Output = Next::Output, Args = Next::Args>,
        {
            fn validate_captures(
                &self,
                c: &[String],
                i: &mut usize,
                ca: &Option<Vec<String>>,
            ) -> RouteResult<()> {
                self.next.validate_captures(c, i, ca)
            }
            forward_response_checks!();
            fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
                self.next.extract(st)
            }
        }
    };
}
metadata_chain!(Description);
metadata_chain!(Summary);

// --- Vault (server-only; provides the request's Extensions) ---

impl<Next: ServerChain> ServerChain for Vault<Next>
where
    Self: Endpoint<Output = Next::Output, Args = HCons<Arc<Extensions>, Next::Args>>,
{
    fn validate_captures(
        &self,
        c: &[String],
        i: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        self.next.validate_captures(c, i, ca)
    }
    forward_response_checks!();
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        let ext = st.req.extensions.clone();
        cons_tail(ext, &self.next, st)
    }
}

// --- WithResource (server-only; allocates R from the context) ---

impl<R, Next> ServerChain for WithResource<R, Next>
where
    R: Send + Sync + 'static,
    Next: ServerChain,
    Self: Endpoint<Args = HCons<R, Next::Args>, Output = Next::Output>,
{
    fn validate_captures(
        &self,
        c: &[String],
        i: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        self.next.validate_captures(c, i, ca)
    }
    forward_response_checks!();
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        let Some(provider) = st.lookup_ctx::<crate::context::ResourceProvider<R>>() else {
            return RouteResult::FailFatal(
                ServerError::err500().with_body("resource provider not configured in context"),
            );
        };
        let resource = (provider.0)();
        cons_tail(resource, &self.next, st)
    }
}

// --- BasicAuth (server-only; resolves Usr from the Authorization header) ---

fn parse_basic_auth(header: &str) -> Option<BasicAuthData> {
    let rest = header
        .strip_prefix("Basic ")
        .or_else(|| header.strip_prefix("basic "))?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(rest.trim())
        .ok()?;
    let s = String::from_utf8(decoded).ok()?;
    let (username, password) = s.split_once(':')?;
    Some(BasicAuthData {
        username: username.to_owned(),
        password: password.to_owned(),
    })
}

impl<Usr, Next> ServerChain for BasicAuth<Usr, Next>
where
    Usr: Send + Sync + 'static,
    Next: ServerChain,
    Self: Endpoint<Args = HCons<Usr, Next::Args>, Output = Next::Output>,
{
    fn validate_captures(
        &self,
        c: &[String],
        i: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        self.next.validate_captures(c, i, ca)
    }
    forward_response_checks!();
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        let realm = self.realm.replace('"', "");
        let unauthorized = || {
            let mut e = ServerError::err401();
            if let Ok(v) = http::HeaderValue::from_str(&format!("Basic realm=\"{realm}\"")) {
                e = e.with_header(http::header::WWW_AUTHENTICATE, v);
            }
            RouteResult::FailFatal(e)
        };

        let Some(check) = st.lookup_ctx::<crate::context::BasicAuthCheck<Usr>>() else {
            return RouteResult::FailFatal(
                ServerError::err500().with_body("basic-auth check not configured in context"),
            );
        };
        let Some(data) = st
            .req
            .headers
            .get(http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_basic_auth)
        else {
            return unauthorized();
        };

        match (check.0)(&data) {
            BasicAuthResult::Authorized(usr) => cons_tail(usr, &self.next, st),
            BasicAuthResult::BadPassword
            | BasicAuthResult::NoSuchUser
            | BasicAuthResult::Unauthorized => unauthorized(),
        }
    }
}

// --- AuthProtect (generalized auth) ---

impl<Usr, Next> ServerChain for AuthProtect<Usr, Next>
where
    Usr: Send + Sync + 'static,
    Next: ServerChain,
    Self: Endpoint<Args = HCons<Usr, Next::Args>, Output = Next::Output>,
{
    fn validate_captures(
        &self,
        c: &[String],
        i: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        self.next.validate_captures(c, i, ca)
    }
    forward_response_checks!();
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        let Some(check) = st.lookup_ctx::<crate::context::AuthCheck<Usr>>() else {
            return RouteResult::FailFatal(
                ServerError::err500().with_body("auth check not configured in context"),
            );
        };
        match (check.0)(&st.req.headers) {
            Ok(usr) => cons_tail(usr, &self.next, st),
            Err(e) => RouteResult::FailFatal(e),
        }
    }
}

// --- Request-info combinators (server-only) ---

impl<Next: ServerChain> ServerChain for IsSecure<Next>
where
    Self: Endpoint<Output = Next::Output, Args = HCons<bool, Next::Args>>,
{
    fn validate_captures(
        &self,
        c: &[String],
        i: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        self.next.validate_captures(c, i, ca)
    }
    forward_response_checks!();
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        let secure = st.req.is_secure;
        cons_tail(secure, &self.next, st)
    }
}

impl<Next: ServerChain> ServerChain for HttpVersion<Next>
where
    Self: Endpoint<Output = Next::Output, Args = HCons<http::Version, Next::Args>>,
{
    fn validate_captures(
        &self,
        c: &[String],
        i: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        self.next.validate_captures(c, i, ca)
    }
    forward_response_checks!();
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        let version = st.req.version;
        cons_tail(version, &self.next, st)
    }
}

impl<Next: ServerChain> ServerChain for RemoteHost<Next>
where
    Self: Endpoint<Output = Next::Output, Args = HCons<Option<std::net::SocketAddr>, Next::Args>>,
{
    fn validate_captures(
        &self,
        c: &[String],
        i: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        self.next.validate_captures(c, i, ca)
    }
    forward_response_checks!();
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        let addr = st.req.remote_addr;
        cons_tail(addr, &self.next, st)
    }
}

// --- WithNamedContext (named sub-context scope) ---

impl<Name, Next> ServerChain for WithNamedContext<Name, Next>
where
    Name: 'static,
    Next: ServerChain,
    Self: Endpoint<Output = Next::Output, Args = Next::Args>,
{
    fn validate_captures(
        &self,
        c: &[String],
        i: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        self.next.validate_captures(c, i, ca)
    }
    forward_response_checks!();
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        if let Some(named) = st.lookup_ctx::<crate::context::NamedContext<Name>>() {
            st.push_ctx(&named.context);
            let result = self.next.extract(st);
            st.pop_ctx();
            result
        } else {
            self.next.extract(st)
        }
    }
}

// --- Capture ---

impl<A, S, Next> ServerChain for Capture<A, S, Next>
where
    A: FromHttpApiData,
    S: CaptureShape<A>,
    Next: ServerChain,
    Self: Endpoint<Args = HCons<<S as CaptureShape<A>>::Out, Next::Args>, Output = Next::Output>,
{
    fn validate_captures(
        &self,
        caps: &[String],
        idx: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        let i = *idx;
        *idx += 1;
        let Some(seg) = caps.get(i) else {
            return RouteResult::Fail(not_found_capture());
        };
        match <S as CaptureShape<A>>::build(A::from_url_piece(seg)) {
            Ok(_) => self.next.validate_captures(caps, idx, ca),
            // Strict parse failure: recoverable Fail (a sibling may parse it).
            Err(e) => RouteResult::Fail(bad_request(format!(
                "could not parse capture `{}`: {}",
                self.name, e
            ))),
        }
    }
    forward_response_checks!();
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        let Some(seg) = st.next_capture() else {
            return RouteResult::Fail(not_found_capture());
        };
        match <S as CaptureShape<A>>::build(A::from_url_piece(&seg)) {
            Ok(out) => cons_tail(out, &self.next, st),
            Err(e) => RouteResult::Fail(bad_request(format!(
                "could not parse capture `{}`: {}",
                self.name, e
            ))),
        }
    }
}

fn not_found_capture() -> ServerError {
    ServerError::err404()
}

// --- CaptureAll ---

impl<A, Next> ServerChain for CaptureAll<A, Next>
where
    A: FromHttpApiData,
    Next: ServerChain,
    Self: Endpoint<Args = HCons<Vec<A>, Next::Args>, Output = Next::Output>,
{
    fn validate_captures(
        &self,
        caps: &[String],
        idx: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        if let Some(segs) = ca {
            for s in segs {
                if let Err(e) = A::from_url_piece(s) {
                    return RouteResult::Fail(bad_request(format!(
                        "could not parse capture-all `{}`: {}",
                        self.name, e
                    )));
                }
            }
        }
        self.next.validate_captures(caps, idx, ca)
    }
    forward_response_checks!();
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        let segs = st.take_capture_all();
        let mut out = Vec::with_capacity(segs.len());
        for s in &segs {
            match A::from_url_piece(s) {
                Ok(a) => out.push(a),
                Err(e) => {
                    return RouteResult::Fail(bad_request(format!(
                        "could not parse capture-all `{}`: {}",
                        self.name, e
                    )));
                }
            }
        }
        cons_tail(out, &self.next, st)
    }
}

// --- QueryParam ---

impl<A, P, S, Next> ServerChain for QueryParam<A, P, S, Next>
where
    A: FromHttpApiData,
    (P, S): ArgShape<A>,
    Next: ServerChain,
    Self: Endpoint<Args = HCons<<(P, S) as ArgShape<A>>::Out, Next::Args>, Output = Next::Output>,
{
    fn validate_captures(
        &self,
        c: &[String],
        i: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        self.next.validate_captures(c, i, ca)
    }
    forward_response_checks!();
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        // A bare key (`?x`, no `=`) is ABSENT (Servant's `join`); only a key
        // with a value (`?x=` or `?x=v`) is present.
        let raw: Option<Result<A, ParseError>> = lookup_query(&st.req.query, &self.name)
            .and_then(|v| v.as_deref())
            .map(A::from_query_param);
        match <(P, S) as ArgShape<A>>::build(raw) {
            Ok(out) => cons_tail(out, &self.next, st),
            // Do not echo the raw value (it may be a secret, e.g. an API key).
            Err(ArgError::Missing) => RouteResult::FailFatal(bad_request(format!(
                "missing required query parameter `{}`",
                self.name
            ))),
            Err(ArgError::Parse(_)) => RouteResult::FailFatal(bad_request(format!(
                "could not parse query parameter `{}`",
                self.name
            ))),
        }
    }
}

// --- QueryParams ---

impl<A, Next> ServerChain for QueryParams<A, Next>
where
    A: FromHttpApiData,
    Next: ServerChain,
    Self: Endpoint<Args = HCons<Vec<A>, Next::Args>, Output = Next::Output>,
{
    fn validate_captures(
        &self,
        c: &[String],
        i: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        self.next.validate_captures(c, i, ca)
    }
    forward_response_checks!();
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        // Servant accepts both `name=` and the bracketed `name[]=` form, and
        // drops valueless entries (`mapMaybe snd`).
        let bracketed = format!("{}[]", self.name);
        let mut out = Vec::new();
        for (k, v) in &st.req.query {
            if k == &self.name || k == &bracketed {
                if let Some(s) = v.as_deref() {
                    match A::from_query_param(s) {
                        Ok(a) => out.push(a),
                        Err(_) => {
                            return RouteResult::FailFatal(bad_request(format!(
                                "could not parse query parameter `{}`",
                                self.name
                            )));
                        }
                    }
                }
            }
        }
        cons_tail(out, &self.next, st)
    }
}

// --- QueryFlag ---

impl<Next: ServerChain> ServerChain for QueryFlag<Next>
where
    Self: Endpoint<Output = Next::Output, Args = HCons<bool, Next::Args>>,
{
    fn validate_captures(
        &self,
        c: &[String],
        i: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        self.next.validate_captures(c, i, ca)
    }
    forward_response_checks!();
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        // Value-sensitive, like Servant's `examine`: absent => false; present
        // with no value => true; present with a value => only true for
        // "true"/"1"/empty (so `?flag=false` and `?flag=0` are false).
        let present = st
            .req
            .query
            .iter()
            .find(|(k, _)| k == &self.name)
            .map(|(_, v)| match v {
                None => true,
                Some(s) => s == "true" || s == "1" || s.is_empty(),
            })
            .unwrap_or(false);
        cons_tail(present, &self.next, st)
    }
}

// --- Header ---

impl<A, P, S, Next> ServerChain for Header<A, P, S, Next>
where
    A: FromHttpApiData,
    (P, S): ArgShape<A>,
    Next: ServerChain,
    Self: Endpoint<Args = HCons<<(P, S) as ArgShape<A>>::Out, Next::Args>, Output = Next::Output>,
{
    fn validate_captures(
        &self,
        c: &[String],
        i: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        self.next.validate_captures(c, i, ca)
    }
    forward_response_checks!();
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        let raw: Option<Result<A, ParseError>> =
            st.req
                .headers
                .get(self.name.as_str())
                .map(|v| match v.to_str() {
                    Ok(s) => A::from_header(s),
                    Err(_) => Err(ParseError::new("header value is not valid text")),
                });
        match <(P, S) as ArgShape<A>>::build(raw) {
            Ok(out) => cons_tail(out, &self.next, st),
            // Do not echo header values (they may be sensitive, e.g. Authorization).
            Err(ArgError::Missing) => RouteResult::FailFatal(bad_request(format!(
                "missing required header `{}`",
                self.name
            ))),
            Err(ArgError::Parse(_)) => RouteResult::FailFatal(bad_request(format!(
                "could not parse header `{}`",
                self.name
            ))),
        }
    }
}

// --- ReqBody ---

impl<CTypes, A, S, Next> ServerChain for ReqBody<CTypes, A, S, Next>
where
    CTypes: AllMime + AllMimeUnrender<A>,
    (Required, S): ArgShape<A>,
    Next: ServerChain,
    Self: Endpoint<
            Args = HCons<<(Required, S) as ArgShape<A>>::Out, Next::Args>,
            Output = Next::Output,
        >,
{
    fn validate_captures(
        &self,
        c: &[String],
        i: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        self.next.validate_captures(c, i, ca)
    }
    fn request_content_types(&self) -> Option<Vec<Mime>> {
        Some(CTypes::all_media_types())
    }
    fn accept_check(&self, accept: Option<&str>) -> RouteResult<()> {
        self.next.accept_check(accept)
    }
    fn render(&self, accept: Option<&str>, value: Self::Output) -> Rendered {
        self.next.render(accept, value)
    }
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        let decoded =
            servant::content::negotiate_content::<CTypes, A>(st.req.content_type(), &st.req.body);
        let raw: Option<Result<A, ParseError>> = match decoded {
            // Content-type was already validated (415) before extraction.
            None => return RouteResult::FailFatal(ServerError::err415()),
            Some(r) => Some(r.map_err(ParseError::new)),
        };
        match <(Required, S) as ArgShape<A>>::build(raw) {
            Ok(out) => cons_tail(out, &self.next, st),
            Err(ArgError::Missing) => {
                RouteResult::FailFatal(bad_request("request body is required"))
            }
            // Do not echo the decode error (it may quote body content, which
            // can contain secrets such as a password field).
            Err(ArgError::Parse(_)) => {
                RouteResult::FailFatal(bad_request("could not parse request body"))
            }
        }
    }
}

fn lookup_query<'a>(
    query: &'a [(String, Option<String>)],
    name: &str,
) -> Option<&'a Option<String>> {
    query.iter().find(|(k, _)| k == name).map(|(_, v)| v)
}

/// The content type the request body's media types must match (phase-5 415
/// check), using the server-request octet-stream default for a missing header.
pub fn content_type_acceptable(body_types: &[Mime], content_type: Option<&str>) -> bool {
    let ct: Mime = match content_type {
        Some(s) => match s.trim().parse() {
            Ok(m) => m,
            Err(_) => return false,
        },
        None => mime::APPLICATION_OCTET_STREAM,
    };
    body_types.iter().any(|m| media_type_matches(m, &ct))
}

use http::StatusCode;
use mime::Mime;
use servant::api::Endpoint;
use servant::error::ServerError;
use servant::hlist::HCons;

use super::state::ExtractState;
use crate::result::RouteResult;

pub(super) fn bad_request(msg: impl Into<String>) -> ServerError {
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

// Helper to thread the tail extraction after a head value.
pub(super) fn cons_tail<H, Next: ServerChain>(
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

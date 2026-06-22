//! The endpoint leaf: runs the extraction phases in Servant's order, invokes the
//! async handler, and renders the response.

use std::future::Future;
use std::sync::Arc;

use bytes::Bytes;
use servant::error::ServerError;
use servant::func::HandlerFn;

use crate::extract::{ExtractState, ServerChain, content_type_acceptable};
use crate::request::RequestData;
use crate::response::{build_response, error_response};
use crate::result::RouteResult;
use crate::router::{BoxRouteFuture, LeafService};

/// A concrete endpoint plus its handler. Implements [`LeafService`] by running
/// the phase pipeline and calling the handler with the extracted argument list.
pub struct EndpointLeaf<Api, H> {
    /// The endpoint description (shared with router-shape construction).
    pub chain: Arc<Api>,
    /// The user handler.
    pub handler: Arc<H>,
    /// The server context (auth checks, resource providers).
    pub context: Arc<crate::context::Context>,
}

fn ready(r: RouteResult<http::Response<crate::response::ResponseBody>>) -> BoxRouteFuture<'static> {
    Box::pin(async move { r })
}

impl<Api, H, Fut> LeafService for EndpointLeaf<Api, H>
where
    Api: ServerChain + Send + Sync + 'static,
    Api::Args: Send,
    Api::Output: Send,
    H: HandlerFn<Api::Args, Output = Fut> + Send + Sync + 'static,
    Fut: Future<Output = Result<Api::Output, ServerError>> + Send + 'static,
{
    fn call<'a>(
        &'a self,
        req: &'a RequestData,
        captures: Vec<String>,
        capture_all: Option<Vec<String>>,
    ) -> BoxRouteFuture<'a> {
        // Phase 1: captures (recoverable Fail on a strict parse failure).
        let mut idx = 0usize;
        match self
            .chain
            .validate_captures(&captures, &mut idx, &capture_all)
        {
            RouteResult::Route(()) => {}
            RouteResult::Fail(e) => return ready(RouteResult::Fail(e)),
            RouteResult::FailFatal(e) => return ready(RouteResult::FailFatal(e)),
        }

        // Phase 2: host matching.
        match self.chain.host_check(req) {
            RouteResult::Route(()) => {}
            RouteResult::Fail(e) => return ready(RouteResult::Fail(e)),
            RouteResult::FailFatal(e) => return ready(RouteResult::FailFatal(e)),
        }

        // Phase 3: method (HEAD is served by the GET route).
        let method_ok = req.method == self.chain.method()
            || (req.is_head && self.chain.method() == http::Method::GET);
        if !method_ok {
            return ready(RouteResult::Fail(ServerError::err405()));
        }

        // Phase 4: accept (406).
        let accept = req.accept();
        match self.chain.accept_check(accept) {
            RouteResult::Route(()) => {}
            RouteResult::Fail(e) => return ready(RouteResult::Fail(e)),
            RouteResult::FailFatal(e) => return ready(RouteResult::FailFatal(e)),
        }

        // Phase 5: content-type (415) — only when a body is expected.
        if let Some(body_types) = self.chain.request_content_types() {
            if !content_type_acceptable(&body_types, req.content_type()) {
                return ready(RouteResult::Fail(ServerError::err415()));
            }
        }

        // Phases 6-8: extract the argument list (FailFatal/400 on failure).
        let mut st = ExtractState::new(captures, capture_all, req, &self.context);
        let args = match self.chain.extract(&mut st) {
            RouteResult::Route(a) => a,
            RouteResult::Fail(e) => return ready(RouteResult::Fail(e)),
            RouteResult::FailFatal(e) => return ready(RouteResult::FailFatal(e)),
        };

        // Run the handler, then render (a handler error is still a real response).
        let fut = self.handler.call_hlist(args);
        let chain = self.chain.clone();
        let accept_owned = accept.map(|s| s.to_owned());
        let is_head = req.is_head;
        Box::pin(async move {
            match fut.await {
                Ok(out) => {
                    let (status, mime, mut body, extra_headers) =
                        chain.render(accept_owned.as_deref(), out);
                    if is_head {
                        body = crate::response::full_body(Bytes::new());
                    }
                    RouteResult::Route(build_response(status, mime, body, extra_headers))
                }
                Err(e) => RouteResult::Route(error_response(&e)),
            }
        })
    }
}

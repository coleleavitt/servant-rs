//! Raw terminal request snapshots and leaf services.

use std::future::Future;
use std::sync::Arc;

use bytes::Bytes;
use http::{Extensions, HeaderMap, Method};
use servant::error::ServerError;
use servant::query::Query;

use crate::context::Context;
use crate::request::RequestData;
use crate::response::{ResponseBody, error_response};
use crate::result::RouteResult;
use crate::router::{BoxRouteFuture, LeafService};

/// Owned request data passed to `Raw` and `RawM` terminal handlers.
#[derive(Clone)]
pub struct RawRequest {
    tail: Vec<String>,
    method: Method,
    raw_query: Option<String>,
    query: Query,
    headers: HeaderMap,
    body: Bytes,
    extensions: Arc<Extensions>,
    version: http::Version,
    remote_addr: Option<std::net::SocketAddr>,
    is_secure: bool,
}

impl RawRequest {
    pub(crate) fn from_request(req: &RequestData, tail: Vec<String>) -> Self {
        RawRequest {
            tail,
            method: req.method.clone(),
            raw_query: req.raw_query.clone(),
            query: Query::from_parts(req.raw_query.clone(), req.query.clone()),
            headers: req.headers.clone(),
            body: req.body.clone(),
            extensions: req.extensions.clone(),
            version: req.version,
            remote_addr: req.remote_addr,
            is_secure: req.is_secure,
        }
    }

    /// The unmatched path tail, decoded into path segments.
    pub fn tail(&self) -> &[String] {
        &self.tail
    }

    /// The request method.
    pub fn method(&self) -> Method {
        self.method.clone()
    }

    /// The raw URI query string without the leading `?`, when present.
    pub fn raw_query(&self) -> Option<&str> {
        self.raw_query.as_deref()
    }

    /// The structured query string.
    pub fn query(&self) -> &Query {
        &self.query
    }

    /// Request headers.
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// The buffered request body.
    pub fn body(&self) -> &Bytes {
        &self.body
    }

    /// Request extensions.
    pub fn extensions(&self) -> &Extensions {
        &self.extensions
    }

    /// The HTTP version.
    pub fn version(&self) -> http::Version {
        self.version
    }

    /// The peer socket address, if known.
    pub fn remote_addr(&self) -> Option<std::net::SocketAddr> {
        self.remote_addr
    }

    /// Whether the connection was marked TLS-secure by the adapter.
    pub fn is_secure(&self) -> bool {
        self.is_secure
    }
}

pub(crate) struct RawLeaf<H> {
    pub(crate) handler: Arc<H>,
}

impl<H, Fut> LeafService for RawLeaf<H>
where
    H: Fn(RawRequest) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = http::Response<ResponseBody>> + Send + 'static,
{
    fn call<'a>(
        &'a self,
        req: &'a RequestData,
        tail: Vec<String>,
        _captures: Vec<String>,
        _capture_all: Option<Vec<String>>,
    ) -> BoxRouteFuture<'a> {
        let request = RawRequest::from_request(req, tail);
        let fut = (self.handler)(request);
        Box::pin(async move { RouteResult::Route(fut.await) })
    }
}

pub(crate) struct RawMLeaf<H> {
    pub(crate) handler: Arc<H>,
    pub(crate) context: Arc<Context>,
}

impl<H, Fut> LeafService for RawMLeaf<H>
where
    H: Fn(RawRequest, Arc<Context>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<http::Response<ResponseBody>, ServerError>> + Send + 'static,
{
    fn call<'a>(
        &'a self,
        req: &'a RequestData,
        tail: Vec<String>,
        _captures: Vec<String>,
        _capture_all: Option<Vec<String>>,
    ) -> BoxRouteFuture<'a> {
        let request = RawRequest::from_request(req, tail);
        let context = self.context.clone();
        let fut = (self.handler)(request, context);
        Box::pin(async move {
            match fut.await {
                Ok(response) => RouteResult::Route(response),
                Err(error) => RouteResult::Route(error_response(&error)),
            }
        })
    }
}

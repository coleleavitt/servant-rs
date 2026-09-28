//! The framework-agnostic edge adapter: a [`tower_service::Service`] over any
//! `http::Request<B>`, plus an optional hyper serving loop.
//!
//! Request bodies stay one-shot through route selection. Buffered `ReqBody`
//! endpoints collect with a bound after endpoint checks; `StreamBody` endpoints
//! hand the live body to the handler.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use bytes::Buf;

#[cfg(feature = "hyper")]
use crate::listener::ConnectionLimits;
#[cfg(feature = "hyper")]
use crate::listener::accept_loop;
use crate::request::{RequestBody, RequestData, parse_query, path_segments};
use crate::response::{ResponseBody, error_response};
use crate::result::RouteResult;
use crate::router::{Router, dispatch};

/// Default maximum buffered request body size (2 MiB).
pub const DEFAULT_MAX_BODY: usize = 2 * 1024 * 1024;

/// Per-connection info a serving layer may insert into request extensions; read
/// by the `RemoteHost`/`IsSecure` combinators.
#[derive(Clone, Copy, Debug, Default)]
pub struct ConnectionInfo {
    /// The peer socket address, if known.
    pub remote_addr: Option<std::net::SocketAddr>,
    /// Whether the connection is over TLS.
    pub secure: bool,
}

/// A cloneable [`tower_service::Service`] that serves a [`Router`].
#[derive(Clone)]
pub struct RouterService {
    router: Arc<Router>,
    max_body: usize,
}

impl RouterService {
    /// Wrap a router with the default body limit.
    pub fn new(router: Router) -> Self {
        RouterService {
            router: Arc::new(router),
            max_body: DEFAULT_MAX_BODY,
        }
    }

    /// Set the maximum buffered request body size (bytes).
    pub fn with_max_body(mut self, max_body: usize) -> Self {
        self.max_body = max_body;
        self
    }

    /// Handle one request end-to-end.
    pub async fn handle<B>(&self, req: http::Request<B>) -> http::Response<ResponseBody>
    where
        B: http_body::Body + Send + 'static,
        B::Data: Buf + Send + 'static,
        B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        let (mut parts, body) = req.into_parts();

        let conn = parts
            .extensions
            .get::<ConnectionInfo>()
            .copied()
            .unwrap_or_default();
        let req_data = RequestData {
            is_head: parts.method == http::Method::HEAD,
            method: parts.method.clone(),
            query: parse_query(parts.uri.query()),
            raw_query: parts.uri.query().map(str::to_owned),
            uri_authority: parts.uri.authority().map(ToString::to_string),
            headers: parts.headers.clone(),
            body: RequestBody::new(body, self.max_body),
            version: parts.version,
            remote_addr: conn.remote_addr,
            is_secure: conn.secure,
            extensions: std::sync::Arc::new(std::mem::take(&mut parts.extensions)),
        };
        let segments = path_segments(parts.uri.path());

        let result = dispatch(&self.router, &segments, Vec::new(), None, &req_data).await;
        match result {
            RouteResult::Route(r) => r,
            RouteResult::Fail(e) | RouteResult::FailFatal(e) => error_response(&e),
        }
    }
}

impl<B> tower_service::Service<http::Request<B>> for RouterService
where
    B: http_body::Body + Send + 'static,
    B::Data: Buf + Send + 'static,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    type Response = http::Response<ResponseBody>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Infallible>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<B>) -> Self::Future {
        let this = self.clone();
        Box::pin(async move { Ok(this.handle(req).await) })
    }
}

/// Serve a router over HTTP/1 on an accepted-connection loop (test/example
/// helper), with the default [`ConnectionLimits`]. Available with the `hyper`
/// feature.
///
/// Runs until the task is dropped: a failed `accept` is logged and retried
/// after a short pause, and connections past the cap are closed. See
/// [`serve_listener_with_limits`].
#[cfg(feature = "hyper")]
pub async fn serve_listener(
    listener: tokio::net::TcpListener,
    service: RouterService,
) -> std::io::Result<()> {
    serve_listener_with_limits(listener, service, ConnectionLimits::default()).await
}

/// [`serve_listener`] with explicit connection limits. Available with the
/// `hyper` feature.
///
/// Never returns an error: the `Result` is kept for the signature's sake.
#[cfg(feature = "hyper")]
pub async fn serve_listener_with_limits(
    listener: tokio::net::TcpListener,
    service: RouterService,
    limits: ConnectionLimits,
) -> std::io::Result<()> {
    match accept_loop(listener, limits, move |stream, peer| {
        serve_http1(
            stream,
            service.clone(),
            ConnectionInfo {
                remote_addr: peer,
                secure: false,
            },
        )
    })
    .await {}
}

/// How long the serving adapters wait for a request's headers before closing
/// the connection.
#[cfg(feature = "hyper")]
pub const HEADER_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Serve HTTP/1 on one connection, inserting `conn` into every request so
/// `RemoteHost`/`IsSecure` can read it.
///
/// A client that opens a connection and does not finish a request's headers
/// within [`HEADER_READ_TIMEOUT`] is dropped rather than holding one of the
/// connection slots for good. (hyper applies the timeout only with a timer set,
/// so both are set explicitly.)
#[cfg(feature = "hyper")]
pub(crate) async fn serve_http1<I>(io: I, service: RouterService, conn: ConnectionInfo)
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let hyper_svc =
        hyper::service::service_fn(move |mut req: http::Request<hyper::body::Incoming>| {
            req.extensions_mut().insert(conn);
            let service = service.clone();
            async move { Ok::<_, Infallible>(service.handle(req).await) }
        });
    let _ = hyper::server::conn::http1::Builder::new()
        .timer(hyper_util::rt::TokioTimer::new())
        .header_read_timeout(HEADER_READ_TIMEOUT)
        .serve_connection(hyper_util::rt::TokioIo::new(io), hyper_svc)
        .await;
}

#[cfg(all(test, feature = "hyper"))]
mod tests {
    use std::time::Duration;

    use servant::prelude::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    #[tokio::test(start_paused = true)]
    async fn stalled_request_headers_are_dropped_at_the_deadline() {
        let router = crate::serve(get::<(PlainText,), String>(), || async {
            Ok::<_, ServerError>(String::new())
        });
        let (server_side, mut client) = tokio::io::duplex(1024);
        let start = tokio::time::Instant::now();
        let server = tokio::spawn(serve_http1(
            server_side,
            RouterService::new(router),
            ConnectionInfo::default(),
        ));

        // Half a request, then nothing.
        client
            .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n")
            .await
            .unwrap();
        let mut response = Vec::new();
        let read =
            tokio::time::timeout(HEADER_READ_TIMEOUT * 4, client.read_to_end(&mut response)).await;
        assert!(
            read.is_ok(),
            "connection still open after {:?}",
            start.elapsed()
        );
        let elapsed = start.elapsed();
        assert!(
            elapsed >= HEADER_READ_TIMEOUT
                && elapsed < HEADER_READ_TIMEOUT + Duration::from_secs(1),
            "closed after {elapsed:?}"
        );
        server.await.unwrap();
    }
}

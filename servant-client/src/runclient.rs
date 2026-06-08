//! The transport abstraction (`RunClient`) and a hyper-backed implementation.

use std::pin::Pin;

use bytes::Bytes;
use futures_core::Stream;

use crate::request::{ClientError, ClientRequest, ClientResponse};

/// A transport that can execute a [`ClientRequest`]. The endpoint combinators
/// build the request and decode the response; this only moves bytes.
pub trait RunClient {
    /// Execute the request, returning the raw response (or a connection error).
    fn run_request(
        &self,
        req: ClientRequest,
    ) -> impl std::future::Future<Output = Result<ClientResponse, ClientError>> + Send;
}

/// A streaming response: headers up front, body delivered as a chunk stream.
pub struct StreamingResponse {
    /// Status code.
    pub status: http::StatusCode,
    /// Response headers.
    pub headers: http::HeaderMap,
    /// The response body as a stream of byte chunks.
    pub body: Pin<Box<dyn Stream<Item = Result<Bytes, ClientError>> + Send>>,
}

/// A transport that can execute a request and expose the response body as a
/// stream (for `Stream`/SSE endpoints), instead of buffering it.
pub trait RunStreamingClient: RunClient {
    /// Execute the request, returning the response with a streaming body.
    fn run_streaming(
        &self,
        req: ClientRequest,
    ) -> impl std::future::Future<Output = Result<StreamingResponse, ClientError>> + Send;
}

#[cfg(feature = "hyper")]
mod hyper_transport {
    use bytes::Bytes;
    use http_body_util::{BodyExt, Full, LengthLimitError, Limited};
    use hyper_util::client::legacy::Client;
    use hyper_util::client::legacy::connect::HttpConnector;
    use hyper_util::rt::TokioExecutor;

    use super::*;
    use crate::request::BaseUrl;

    /// Default maximum buffered response body size (8 MiB).
    pub const DEFAULT_MAX_RESPONSE_BODY: usize = 8 * 1024 * 1024;

    /// A [`RunClient`] backed by hyper over plain HTTP.
    #[derive(Clone)]
    pub struct HyperClient {
        base: BaseUrl,
        inner: Client<HttpConnector, Full<Bytes>>,
        max_body: usize,
    }

    impl HyperClient {
        /// Create a client pointing at `base`.
        pub fn new(base: BaseUrl) -> Self {
            let inner = Client::builder(TokioExecutor::new()).build_http();
            HyperClient {
                base,
                inner,
                max_body: DEFAULT_MAX_RESPONSE_BODY,
            }
        }

        /// Set the maximum buffered response body size (bytes).
        pub fn with_max_body(mut self, max_body: usize) -> Self {
            self.max_body = max_body;
            self
        }

        /// The base URL this client targets.
        pub fn base(&self) -> &BaseUrl {
            &self.base
        }

        /// Build the outgoing hyper request from a [`ClientRequest`].
        fn build(&self, req: &ClientRequest) -> Result<http::Request<Full<Bytes>>, ClientError> {
            let url = self.base.url_for(&req.target());
            let mut builder = http::Request::builder().method(req.method.clone()).uri(url);
            if !req.accept.is_empty() {
                let accept = req
                    .accept
                    .iter()
                    .map(|m| m.as_ref())
                    .collect::<Vec<&str>>()
                    .join(", ");
                builder = builder.header(http::header::ACCEPT, accept);
            }
            for (k, v) in req.headers.iter() {
                builder = builder.header(k, v);
            }
            let body = match &req.body {
                Some((bytes, mt)) => {
                    builder = builder.header(http::header::CONTENT_TYPE, mt.as_ref());
                    Full::new(bytes.clone())
                }
                None => Full::new(Bytes::new()),
            };
            builder
                .body(body)
                .map_err(|e| ClientError::ConnectionError(Box::new(e)))
        }
    }

    impl RunClient for HyperClient {
        async fn run_request(&self, req: ClientRequest) -> Result<ClientResponse, ClientError> {
            let request = self.build(&req)?;
            let resp = self
                .inner
                .request(request)
                .await
                .map_err(|e| ClientError::ConnectionError(Box::new(e)))?;

            let status = resp.status();
            let headers = resp.headers().clone();
            // Bound the buffered response body (no unbounded buffering).
            let body = match Limited::new(resp.into_body(), self.max_body)
                .collect()
                .await
            {
                Ok(c) => c.to_bytes(),
                Err(e) => {
                    if e.downcast_ref::<LengthLimitError>().is_some() {
                        return Err(ClientError::ConnectionError(
                            "response body exceeded the configured size limit".into(),
                        ));
                    }
                    return Err(ClientError::ConnectionError(e));
                }
            };
            Ok(ClientResponse {
                status,
                headers,
                body,
            })
        }
    }

    impl RunStreamingClient for HyperClient {
        async fn run_streaming(
            &self,
            req: ClientRequest,
        ) -> Result<StreamingResponse, ClientError> {
            use futures_util::StreamExt;
            let request = self.build(&req)?;
            let resp = self
                .inner
                .request(request)
                .await
                .map_err(|e| ClientError::ConnectionError(Box::new(e)))?;
            let status = resp.status();
            let headers = resp.headers().clone();
            // Stream the body chunk-by-chunk (no buffering).
            let body = resp
                .into_body()
                .into_data_stream()
                .map(|r| r.map_err(|e| ClientError::ConnectionError(Box::new(e))));
            Ok(StreamingResponse {
                status,
                headers,
                body: Box::pin(body),
            })
        }
    }
}

#[cfg(feature = "hyper")]
pub use hyper_transport::HyperClient;

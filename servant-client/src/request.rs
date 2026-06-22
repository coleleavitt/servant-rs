//! The transport-agnostic client request/response model, mirroring
//! `Servant.Client.Core.Request`/`Response`/`BaseUrl`/`ClientError`.

use bytes::Bytes;
use http::{HeaderMap, HeaderName, Method, StatusCode};
use mime::Mime;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use servant::query::{
    DeepQueryPath,
    Query,
    encode_query_component,
    render_deep_query_key,
    render_pairs,
};

mod base_url;
mod streaming;

pub use base_url::{BaseUrl, Scheme};
pub use streaming::{RequestByteStream, StreamingRequestBody};

const PATH_SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

#[derive(Clone)]
enum QueryRender {
    Plain(String, Option<String>),
    DeepObject(String, Option<String>),
}

/// A request built up incrementally by the client combinators.
#[derive(Clone)]
pub struct ClientRequest {
    /// HTTP method.
    pub method: Method,
    /// Percent-encoded path segments (without leading slash).
    pub segments: Vec<String>,
    /// Ordered query parameters; `None` value renders as a bare key.
    pub query: Vec<(String, Option<String>)>,
    query_render: Vec<QueryRender>,
    /// Raw query prefix set by `QueryString`; later query combinators append.
    pub raw_query: Option<String>,
    /// Request headers.
    pub headers: HeaderMap,
    /// Acceptable response media types, in preference order.
    pub accept: Vec<Mime>,
    /// Request body and its media type.
    pub body: Option<(Bytes, Mime)>,
    /// One-shot streaming request body and its media type.
    pub streaming_body: Option<StreamingRequestBody>,
}

impl ClientRequest {
    /// A fresh `GET` request to the root with no query, headers, or body.
    pub fn new() -> Self {
        ClientRequest {
            method: Method::GET,
            segments: Vec::new(),
            query: Vec::new(),
            query_render: Vec::new(),
            raw_query: None,
            headers: HeaderMap::new(),
            accept: Vec::new(),
            body: None,
            streaming_body: None,
        }
    }

    /// Append a raw path segment, percent-encoding it.
    pub fn append_path(&mut self, raw: &str) {
        self.segments
            .push(utf8_percent_encode(raw, PATH_SEGMENT).to_string());
    }

    /// Append a query parameter (`value` `None` renders as a bare key/flag).
    pub fn append_query(&mut self, name: &str, value: Option<String>) {
        let name = name.to_owned();
        self.query.push((name.clone(), value.clone()));
        self.query_render.push(QueryRender::Plain(name, value));
    }

    pub(crate) fn append_deep_query(
        &mut self,
        root: &str,
        path: &DeepQueryPath,
        value: Option<String>,
    ) {
        let key = render_deep_query_key(root, path);
        self.query.push((key.clone(), value.clone()));
        self.query_render.push(QueryRender::DeepObject(key, value));
    }

    /// Replace the full query string.
    pub fn set_query_string(&mut self, query: Query) {
        let (raw, pairs) = query.into_parts();
        self.raw_query = raw;
        self.query = if self.raw_query.is_some() {
            Vec::new()
        } else {
            pairs
        };
        self.query_render = self
            .query
            .iter()
            .map(|(key, value)| QueryRender::Plain(key.clone(), value.clone()))
            .collect();
    }

    /// Append a header value (allowing duplicates).
    pub fn add_header(&mut self, name: HeaderName, value: http::HeaderValue) {
        self.headers.append(name, value);
    }

    /// Set the request body and its media type.
    pub fn set_body(&mut self, body: Bytes, media: Mime) {
        self.body = Some((body, media));
        self.streaming_body = None;
    }

    /// Set a streaming request body and its media type.
    pub fn set_streaming_body(&mut self, body: StreamingRequestBody) {
        self.body = None;
        self.streaming_body = Some(body);
    }

    /// Whether this request contains a one-shot streaming body.
    pub fn has_streaming_body(&self) -> bool {
        self.streaming_body.is_some()
    }

    /// Render the path + query into an origin-form target (e.g. `/a/b?x=1`).
    pub fn target(&self) -> String {
        let mut out = String::from("/");
        out.push_str(&self.segments.join("/"));
        if self.raw_query.is_some() || !self.query.is_empty() {
            out.push('?');
            if let Some(raw) = &self.raw_query {
                out.push_str(raw);
                if !raw.is_empty() && !self.query.is_empty() {
                    out.push('&');
                }
            }
            out.push_str(&render_query(&self.query_render));
        }
        out
    }
}

fn render_query(query: &[QueryRender]) -> String {
    query
        .iter()
        .map(|item| match item {
            QueryRender::Plain(key, value) => render_pairs(&[(key.clone(), value.clone())]),
            QueryRender::DeepObject(key, Some(value)) => {
                format!("{key}={}", encode_query_component(value))
            }
            QueryRender::DeepObject(key, None) => key.clone(),
        })
        .collect::<Vec<_>>()
        .join("&")
}

impl Default for ClientRequest {
    fn default() -> Self {
        Self::new()
    }
}

use servant::redact::is_sensitive_header;

/// Render a `HeaderMap` as redacted `(name, value-or-<redacted>)` pairs.
fn redacted_headers(headers: &HeaderMap) -> Vec<(&str, &str)> {
    headers
        .iter()
        .map(|(k, v)| {
            (
                k.as_str(),
                if is_sensitive_header(k) {
                    "<redacted>"
                } else {
                    v.to_str().unwrap_or("<binary>")
                },
            )
        })
        .collect()
}

impl std::fmt::Debug for ClientRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientRequest")
            .field("method", &self.method)
            .field("target", &self.target())
            .field("accept", &self.accept)
            .field("headers", &redacted_headers(&self.headers))
            .field("body", &self.body.as_ref().map(|(b, m)| (b.len(), m)))
            .field(
                "streaming_body",
                &self
                    .streaming_body
                    .as_ref()
                    .map(StreamingRequestBody::media_type),
            )
            .finish()
    }
}

/// A response received by the transport.
#[derive(Clone)]
pub struct ClientResponse {
    /// Status code.
    pub status: StatusCode,
    /// Response headers.
    pub headers: HeaderMap,
    /// Fully-buffered response body.
    pub body: Bytes,
}

impl std::fmt::Debug for ClientResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Redact sensitive headers (e.g. Set-Cookie) and never print the body
        // content (it may contain secrets) — only its length.
        f.debug_struct("ClientResponse")
            .field("status", &self.status)
            .field("headers", &redacted_headers(&self.headers))
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// Errors surfaced by a typed client. Mirrors Servant's `ClientError`.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// The response status was not the endpoint's declared success status.
    #[error("request failed with status {}", .response.status)]
    FailureResponse {
        /// The unexpected response.
        response: ClientResponse,
    },
    /// The body could not be decoded as the negotiated content type.
    #[error("failed to decode response body: {message}")]
    DecodeFailure {
        /// Decoder error message.
        message: String,
        /// The response.
        response: ClientResponse,
    },
    /// The response `Content-Type` matched no decoder the endpoint declares.
    #[error("unsupported response content type: {media_type}")]
    UnsupportedContentType {
        /// The unsupported media type.
        media_type: Mime,
        /// The response.
        response: ClientResponse,
    },
    /// The response `Content-Type` header was present but unparseable.
    #[error("invalid Content-Type header in response")]
    InvalidContentTypeHeader {
        /// The response.
        response: ClientResponse,
    },
    /// The transport failed to complete the request.
    #[error("connection error: {0}")]
    ConnectionError(Box<dyn std::error::Error + Send + Sync>),
    /// A request body could not be serialized into its content type.
    #[error("failed to encode request body: {message}")]
    EncodeFailure {
        /// The encoder error message.
        message: String,
    },
    /// The selected transport does not support streaming request bodies.
    #[error("streaming request body requires a streaming request transport")]
    StreamingRequestUnsupported,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_renders_path_and_query() {
        let mut req = ClientRequest::new();
        req.append_path("users");
        req.append_path("a b");
        req.append_query("q", Some("x y".into()));
        req.append_query("flag", None);
        assert_eq!(req.target(), "/users/a%20b?q=x%20y&flag");
    }

    #[test]
    fn debug_redacts_authorization() {
        let mut req = ClientRequest::new();
        req.add_header(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_static("Bearer supersecret"),
        );
        let dbg = format!("{req:?}");
        assert!(dbg.contains("<redacted>"));
        assert!(!dbg.contains("supersecret"));
    }

    #[test]
    fn base_url_builds_absolute() {
        let base = BaseUrl::http("localhost", 8080);
        assert_eq!(base.url_for("/a/b?x=1"), "http://localhost:8080/a/b?x=1");
    }
}

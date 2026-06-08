//! The transport-agnostic client request/response model, mirroring
//! `Servant.Client.Core.Request`/`Response`/`BaseUrl`/`ClientError`.

use bytes::Bytes;
use http::{HeaderMap, HeaderName, Method, StatusCode};
use mime::Mime;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

const PATH_SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// A request built up incrementally by the client combinators.
#[derive(Clone)]
pub struct ClientRequest {
    /// HTTP method.
    pub method: Method,
    /// Percent-encoded path segments (without leading slash).
    pub segments: Vec<String>,
    /// Ordered query parameters; `None` value renders as a bare key.
    pub query: Vec<(String, Option<String>)>,
    /// Request headers.
    pub headers: HeaderMap,
    /// Acceptable response media types, in preference order.
    pub accept: Vec<Mime>,
    /// Request body and its media type.
    pub body: Option<(Bytes, Mime)>,
}

impl ClientRequest {
    /// A fresh `GET` request to the root with no query, headers, or body.
    pub fn new() -> Self {
        ClientRequest {
            method: Method::GET,
            segments: Vec::new(),
            query: Vec::new(),
            headers: HeaderMap::new(),
            accept: Vec::new(),
            body: None,
        }
    }

    /// Append a raw path segment, percent-encoding it.
    pub fn append_path(&mut self, raw: &str) {
        self.segments
            .push(utf8_percent_encode(raw, PATH_SEGMENT).to_string());
    }

    /// Append a query parameter (`value` `None` renders as a bare key/flag).
    pub fn append_query(&mut self, name: &str, value: Option<String>) {
        self.query.push((name.to_owned(), value));
    }

    /// Append a header value (allowing duplicates).
    pub fn add_header(&mut self, name: HeaderName, value: http::HeaderValue) {
        self.headers.append(name, value);
    }

    /// Set the request body and its media type.
    pub fn set_body(&mut self, body: Bytes, media: Mime) {
        self.body = Some((body, media));
    }

    /// Render the path + query into an origin-form target (e.g. `/a/b?x=1`).
    pub fn target(&self) -> String {
        let mut out = String::from("/");
        out.push_str(&self.segments.join("/"));
        if !self.query.is_empty() {
            out.push('?');
            let parts: Vec<String> = self
                .query
                .iter()
                .map(|(k, v)| match v {
                    Some(val) => format!(
                        "{}={}",
                        utf8_percent_encode(k, PATH_SEGMENT),
                        utf8_percent_encode(val, PATH_SEGMENT)
                    ),
                    None => utf8_percent_encode(k, PATH_SEGMENT).to_string(),
                })
                .collect();
            out.push_str(&parts.join("&"));
        }
        out
    }
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

/// Where the client points. Default ports: 80 (http) / 443 (https).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseUrl {
    /// `http` or `https`.
    pub scheme: Scheme,
    /// Host name.
    pub host: String,
    /// Port.
    pub port: u16,
    /// Base path prefix (without trailing slash), prepended to request paths.
    pub path: String,
}

/// URL scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// `http://`
    Http,
    /// `https://`
    Https,
}

impl BaseUrl {
    /// `http://host:port` with no base path.
    pub fn http(host: impl Into<String>, port: u16) -> Self {
        BaseUrl {
            scheme: Scheme::Http,
            host: host.into(),
            port,
            path: String::new(),
        }
    }

    /// The scheme as a string.
    pub fn scheme_str(&self) -> &'static str {
        match self.scheme {
            Scheme::Http => "http",
            Scheme::Https => "https",
        }
    }

    /// Build the absolute URL for a request target (which begins with `/`).
    pub fn url_for(&self, target: &str) -> String {
        format!(
            "{}://{}:{}{}{}",
            self.scheme_str(),
            self.host,
            self.port,
            self.path,
            target
        )
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

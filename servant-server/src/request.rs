//! The in-memory request the router and extractors operate on.
//!
//! The hyper adapter buffers the (bounded) body up front, so routing and
//! extraction are synchronous over this owned snapshot; only the handler is
//! async. Streaming bodies are a deliberate later addition.

use std::sync::Arc;

use bytes::Bytes;
use http::{Extensions, HeaderMap, Method};
use servant::query::Query;
use servant::redact::is_sensitive_header;

/// Request-global data shared by every candidate route during dispatch.
#[derive(Clone)]
pub struct RequestData {
    /// The effective method (a `HEAD` request keeps `HEAD` here; GET matching
    /// is handled in the leaf).
    pub method: Method,
    /// Whether the original request was `HEAD` (response body is stripped).
    pub is_head: bool,
    /// Parsed, percent-decoded query parameters in order. `None` value means a
    /// key with no `=` (e.g. `?flag`).
    pub query: Vec<(String, Option<String>)>,
    /// The raw query string from the request URI, without the leading `?`.
    pub raw_query: Option<String>,
    /// The URI authority from absolute-form requests, if present.
    pub uri_authority: Option<String>,
    /// Request headers.
    pub headers: HeaderMap,
    /// The fully-buffered request body.
    pub body: Bytes,
    /// Per-request extensions (set by middleware); read by the `Vault` combinator.
    pub extensions: Arc<Extensions>,
    /// The request's HTTP version (for `HttpVersion`).
    pub version: http::Version,
    /// The peer socket address, if known (for `RemoteHost`).
    pub remote_addr: Option<std::net::SocketAddr>,
    /// Whether the connection is secure/TLS (for `IsSecure`).
    pub is_secure: bool,
}

impl std::fmt::Debug for RequestData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Redact sensitive headers and never print the body content (only its
        // length) — request bodies and Authorization/Cookie may hold secrets.
        let headers: Vec<(&str, &str)> = self
            .headers
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
            .collect();
        f.debug_struct("RequestData")
            .field("method", &self.method)
            .field("is_head", &self.is_head)
            .field("query", &self.query)
            .field("raw_query", &self.raw_query.as_ref().map(|_| "<present>"))
            .field("uri_authority", &self.uri_authority)
            .field("headers", &headers)
            .field("body_len", &self.body.len())
            .finish()
    }
}

impl RequestData {
    /// The `Accept` header value, if present and valid UTF-8.
    pub fn accept(&self) -> Option<&str> {
        self.headers
            .get(http::header::ACCEPT)
            .and_then(|v| v.to_str().ok())
    }

    /// The `Content-Type` header value, if present and valid UTF-8.
    pub fn content_type(&self) -> Option<&str> {
        self.headers
            .get(http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
    }

    /// The effective host authority: `Host` header first, then URI authority.
    pub fn host_authority(&self) -> Option<&str> {
        if let Some(host) = self.headers.get(http::header::HOST) {
            return host.to_str().ok();
        }
        self.uri_authority.as_deref()
    }
}

/// Split a request path into percent-decoded segments, mirroring WAI's
/// `pathInfo`: the leading `/` is dropped and a trailing `/` yields a final
/// empty segment (so the router can treat `["x", ""]` as a trailing slash).
///
/// `"/"` and `""` both yield `[""]`, which the router treats as the empty path.
pub fn path_segments(path: &str) -> Vec<String> {
    let trimmed = path.strip_prefix('/').unwrap_or(path);
    trimmed.split('/').map(percent_decode).collect()
}

/// Parse a raw query string into ordered key/value pairs, decoding
/// percent-escapes and `+` (form convention, matching Servant's
/// `parseQueryText`). A key without `=` yields `None`.
pub fn parse_query(query: Option<&str>) -> Vec<(String, Option<String>)> {
    Query::parse(query).pairs().to_vec()
}

fn percent_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_segments_and_trailing_slash() {
        assert_eq!(path_segments("/users/42"), vec!["users", "42"]);
        assert_eq!(path_segments("/users/"), vec!["users", ""]);
        assert_eq!(path_segments("/"), vec![""]);
        assert_eq!(path_segments("/a%2Fb"), vec!["a/b"]);
    }

    #[test]
    fn query_parsing() {
        assert_eq!(
            parse_query(Some("a=1&b=2&flag&c=a+b")),
            vec![
                ("a".into(), Some("1".into())),
                ("b".into(), Some("2".into())),
                ("flag".into(), None),
                ("c".into(), Some("a b".into())),
            ]
        );
        assert_eq!(parse_query(None), vec![]);
        assert_eq!(
            parse_query(Some("k=%40")),
            vec![("k".into(), Some("@".into()))]
        );
        assert_eq!(
            servant::query::parse_pairs("flag&empty=&bad=%ZZ&plus=a+b"),
            vec![
                ("flag".into(), None),
                ("empty".into(), Some(String::new())),
                ("bad".into(), Some("%ZZ".into())),
                ("plus".into(), Some("a b".into())),
            ]
        );
    }
}

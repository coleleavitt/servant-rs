//! The structured server error type, mirroring `Servant.Server.Internal.ServerError`.
//!
//! A [`ServerError`] is what a handler returns on failure and what the
//! extraction pipeline produces for routing failures (404/405/406/415/400/...).
//! It carries an explicit status, an overridable reason phrase, a body, and
//! headers — the same four fields as Servant's record — so the exact wire
//! response is under the author's control.

use bytes::Bytes;
use http::{HeaderMap, StatusCode};

/// A fully-described HTTP error response.
///
/// Construct the common ones with the `err300`..`err505` constructors (e.g.
/// [`ServerError::err404`]) and customize with the builder methods, mirroring
/// Haskell `throwError $ err400 { errBody = "..." }`.
#[derive(Debug, Clone)]
pub struct ServerError {
    status: StatusCode,
    reason: String,
    body: Bytes,
    headers: HeaderMap,
}

impl ServerError {
    /// Create an error with the given status code and reason phrase.
    pub fn new(status: StatusCode, reason: impl Into<String>) -> Self {
        ServerError {
            status,
            reason: reason.into(),
            body: Bytes::new(),
            headers: HeaderMap::new(),
        }
    }

    /// The numeric status code (`errHTTPCode`).
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// The numeric status code as `u16`.
    pub fn code(&self) -> u16 {
        self.status.as_u16()
    }

    /// The reason phrase (`errReasonPhrase`).
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// The response body (`errBody`).
    pub fn body(&self) -> &Bytes {
        &self.body
    }

    /// The response headers (`errHeaders`).
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Mutable access to the response headers.
    pub fn headers_mut(&mut self) -> &mut HeaderMap {
        &mut self.headers
    }

    /// Set the body (consuming builder), like `err400 { errBody = ... }`.
    pub fn with_body(mut self, body: impl Into<Bytes>) -> Self {
        self.body = body.into();
        self
    }

    /// Set the reason phrase (consuming builder).
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = reason.into();
        self
    }

    /// Add a header (consuming builder).
    pub fn with_header(mut self, name: http::HeaderName, value: http::HeaderValue) -> Self {
        self.headers.append(name, value);
        self
    }
}

impl std::fmt::Display for ServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.status.as_u16(), self.reason)
    }
}

impl std::error::Error for ServerError {}

impl PartialEq for ServerError {
    fn eq(&self, other: &Self) -> bool {
        // HeaderMap is not Eq across orderings cheaply; compare the load-bearing
        // fields plus header pairs as a set-with-multiplicity.
        self.status == other.status
            && self.reason == other.reason
            && self.body == other.body
            && header_pairs(&self.headers) == header_pairs(&other.headers)
    }
}

fn header_pairs(h: &HeaderMap) -> Vec<(String, Vec<u8>)> {
    let mut v: Vec<(String, Vec<u8>)> = h
        .iter()
        .map(|(k, val)| (k.as_str().to_owned(), val.as_bytes().to_vec()))
        .collect();
    v.sort();
    v
}

macro_rules! err_constructors {
    ( $( $name:ident => ($code:literal, $phrase:literal) ),* $(,)? ) => {
        impl ServerError {
            $(
                #[doc = concat!("`", stringify!($code), " ", $phrase, "`")]
                pub fn $name() -> ServerError {
                    ServerError {
                        status: StatusCode::from_u16($code).expect("valid status code"),
                        reason: $phrase.to_owned(),
                        body: Bytes::new(),
                        headers: HeaderMap::new(),
                    }
                }
            )*
        }
    };
}

err_constructors! {
    err300 => (300, "Multiple Choices"),
    err301 => (301, "Moved Permanently"),
    err302 => (302, "Found"),
    err303 => (303, "See Other"),
    err304 => (304, "Not Modified"),
    err305 => (305, "Use Proxy"),
    err307 => (307, "Temporary Redirect"),
    err400 => (400, "Bad Request"),
    err401 => (401, "Unauthorized"),
    err402 => (402, "Payment Required"),
    err403 => (403, "Forbidden"),
    err404 => (404, "Not Found"),
    err405 => (405, "Method Not Allowed"),
    err406 => (406, "Not Acceptable"),
    err407 => (407, "Proxy Authentication Required"),
    err409 => (409, "Conflict"),
    err410 => (410, "Gone"),
    err411 => (411, "Length Required"),
    err412 => (412, "Precondition Failed"),
    err413 => (413, "Request Entity Too Large"),
    err414 => (414, "Request-URI Too Large"),
    err415 => (415, "Unsupported Media Type"),
    err416 => (416, "Request range not satisfiable"),
    err417 => (417, "Expectation Failed"),
    err418 => (418, "I'm a teapot"),
    err422 => (422, "Unprocessable Entity"),
    err429 => (429, "Too Many Requests"),
    err500 => (500, "Internal Server Error"),
    err501 => (501, "Not Implemented"),
    err502 => (502, "Bad Gateway"),
    err503 => (503, "Service Unavailable"),
    err504 => (504, "Gateway Time-out"),
    err505 => (505, "HTTP Version not supported"),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_phrases_are_exact() {
        assert_eq!(ServerError::err404().code(), 404);
        assert_eq!(ServerError::err404().reason(), "Not Found");
        assert_eq!(ServerError::err415().reason(), "Unsupported Media Type");
        assert_eq!(ServerError::err414().reason(), "Request-URI Too Large");
        assert_eq!(ServerError::err418().reason(), "I'm a teapot");
        assert_eq!(ServerError::err400().reason(), "Bad Request");
    }

    #[test]
    fn builder_customizes_body_and_reason() {
        let e = ServerError::err400()
            .with_body("nope")
            .with_reason("Custom");
        assert_eq!(e.body().as_ref(), b"nope");
        assert_eq!(e.reason(), "Custom");
        assert_eq!(e.code(), 400);
    }
}

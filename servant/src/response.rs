//! Response headers, mirroring `Servant.API.ResponseHeaders` (`Headers`).
//!
//! [`Headers`] wraps a response value with extra HTTP response headers a handler
//! wants to set (e.g. `Location` on a 201). It is the `Output` of the
//! `VerbWithHeaders` combinator (a sibling of `Verb`). The body is still
//! content-negotiated over the verb's content types; the headers are attached to
//! the response envelope.
//!
//! Unlike Haskell's type-indexed `Headers '[Header "X" T] a`, the header set here
//! is dynamic (a `Vec`), which keeps the response combinator a single type and
//! avoids type-level header lists — an intentional simplification. **\[diff\]**

use http::{HeaderName, HeaderValue};

/// A response value plus extra response headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Headers<A> {
    value: A,
    headers: Vec<(HeaderName, HeaderValue)>,
}

impl<A> Headers<A> {
    /// Wrap a value with no extra headers.
    pub fn new(value: A) -> Self {
        Headers {
            value,
            headers: Vec::new(),
        }
    }

    /// Attach a header (consuming builder).
    pub fn header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.headers.push((name, value));
        self
    }

    /// Attach a header parsed from strings; a malformed name/value is ignored
    /// (best-effort, never panics).
    pub fn try_header(mut self, name: &str, value: &str) -> Self {
        if let (Ok(n), Ok(v)) = (HeaderName::try_from(name), HeaderValue::try_from(value)) {
            self.headers.push((n, v));
        }
        self
    }

    /// The wrapped response value.
    pub fn value(&self) -> &A {
        &self.value
    }

    /// The attached headers.
    pub fn headers(&self) -> &[(HeaderName, HeaderValue)] {
        &self.headers
    }

    /// Split into the value and its headers.
    pub fn into_parts(self) -> (A, Vec<(HeaderName, HeaderValue)>) {
        (self.value, self.headers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_with_headers() {
        let h = Headers::new(42u32)
            .try_header("x-total-count", "100")
            .try_header("location", "/things/42");
        assert_eq!(*h.value(), 42);
        assert_eq!(h.headers().len(), 2);
        let (v, hs) = h.into_parts();
        assert_eq!(v, 42);
        assert_eq!(hs[0].0.as_str(), "x-total-count");
    }

    #[test]
    fn malformed_header_is_ignored() {
        let h = Headers::new(()).try_header("bad header name", "v");
        assert!(h.headers().is_empty());
    }
}

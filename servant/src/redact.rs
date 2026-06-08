//! Shared policy for which HTTP headers carry secret material and must never be
//! logged or echoed (security rule). Used by client and server `Debug` impls and
//! by error formatting.

use http::HeaderName;

/// Whether a header's value is sensitive (auth/credential/session material) and
/// should be redacted from logs, `Debug` output, and error bodies.
pub fn is_sensitive_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "authorization"
            | "proxy-authorization"
            | "cookie"
            | "set-cookie"
            | "x-api-key"
            | "x-auth-token"
            | "x-csrf-token"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_auth_headers() {
        assert!(is_sensitive_header(&http::header::AUTHORIZATION));
        assert!(is_sensitive_header(&http::header::COOKIE));
        assert!(is_sensitive_header(&http::header::SET_COOKIE));
        assert!(!is_sensitive_header(&http::header::CONTENT_TYPE));
        assert!(!is_sensitive_header(&http::header::ACCEPT));
    }
}

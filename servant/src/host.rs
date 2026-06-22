//! Host matching policy shared by Host-aware interpretations.

use std::str::FromStr;

use http::uri::Authority;

/// Port comparison behavior for a [`crate::api::Host`] requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostPortPolicy {
    /// The required host omitted a port, so any request port is ignored.
    IgnoreRequestPort,
    /// The required host included a port, so the request must include the same
    /// explicit port.
    RequireExplicitPort(u16),
}

/// A parsed Host combinator value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRequirement {
    original: String,
    hostname: String,
    port: Option<u16>,
}

/// Host requirement or request authority parsing failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostParseError;

impl std::fmt::Display for HostParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid host authority")
    }
}

impl std::error::Error for HostParseError {}

impl HostRequirement {
    /// Parse a host authority string such as `example.com` or
    /// `example.com:8443`.
    pub fn parse(authority: impl Into<String>) -> Result<Self, HostParseError> {
        let original = authority.into();
        let parsed = parse_authority(&original)?;
        let hostname = parsed.host();
        if hostname.is_empty() || original.contains('@') {
            return Err(HostParseError);
        }
        let port = match parsed.port() {
            Some(_) => Some(parsed.port_u16().ok_or(HostParseError)?),
            None => None,
        };
        if port.is_none() && has_unparsed_port_marker(&original, hostname) {
            return Err(HostParseError);
        }
        Ok(HostRequirement {
            original,
            hostname: hostname.to_string(),
            port,
        })
    }

    /// The original authority string.
    pub fn as_str(&self) -> &str {
        &self.original
    }

    /// The parsed host name.
    pub fn hostname(&self) -> &str {
        &self.hostname
    }

    /// The parsed explicit port, if any.
    pub fn port(&self) -> Option<u16> {
        self.port
    }

    /// The request port policy implied by the combinator value.
    pub fn port_policy(&self) -> HostPortPolicy {
        match self.port {
            Some(port) => HostPortPolicy::RequireExplicitPort(port),
            None => HostPortPolicy::IgnoreRequestPort,
        }
    }

    /// Return whether a request Host header or URI authority satisfies this
    /// requirement.
    pub fn matches_authority(&self, request_authority: &str) -> bool {
        let Ok(request) = HostRequirement::parse(request_authority) else {
            return false;
        };
        if !self.hostname.eq_ignore_ascii_case(request.hostname()) {
            return false;
        }
        match self.port {
            Some(port) => request.port == Some(port),
            None => true,
        }
    }
}

fn parse_authority(authority: &str) -> Result<Authority, HostParseError> {
    Authority::from_str(authority).map_err(|_| HostParseError)
}

fn has_unparsed_port_marker(authority: &str, hostname: &str) -> bool {
    authority
        .strip_prefix(hostname)
        .is_some_and(|suffix| suffix.starts_with(':'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_names_match_case_insensitively() {
        let required = HostRequirement::parse("API.example.COM").unwrap();
        assert!(required.matches_authority("api.EXAMPLE.com"));
    }

    #[test]
    fn explicit_port_must_match() {
        let required = HostRequirement::parse("api.example.com:8443").unwrap();
        assert!(required.matches_authority("API.EXAMPLE.COM:8443"));
        assert!(!required.matches_authority("api.example.com:443"));
        assert!(!required.matches_authority("api.example.com"));
    }

    #[test]
    fn portless_requirement_ignores_request_port() {
        let required = HostRequirement::parse("api.example.com").unwrap();
        assert!(required.matches_authority("api.example.com:9443"));
    }

    #[test]
    fn malformed_authority_does_not_match() {
        let required = HostRequirement::parse("api.example.com").unwrap();
        assert!(!required.matches_authority("api.example.com:notaport"));
        assert!(HostRequirement::parse("user@api.example.com").is_err());
    }
}

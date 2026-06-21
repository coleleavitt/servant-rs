//! Structured query-string values.
//!
//! [`Query`] preserves the decoded ordered key/value pairs every interpreter can
//! reason about, plus the original raw query string when a boundary supplied it.

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};

const QUERY_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// A full URI query string.
///
/// `pairs` are percent-decoded in request order. `None` means a bare key with no
/// `=`, while `Some("")` means an explicitly empty value. `raw` is present when
/// the value came from an HTTP request URI or from [`Query::from_raw`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    raw: Option<String>,
    pairs: Vec<(String, Option<String>)>,
}

impl Query {
    /// Build a query from decoded ordered pairs.
    pub fn new(pairs: Vec<(String, Option<String>)>) -> Self {
        Query { raw: None, pairs }
    }

    /// Build a query with an already-encoded raw query string and decoded pairs.
    ///
    /// `raw` is the part after `?` and must not include the leading question
    /// mark. Renderers preserve it byte-for-byte.
    pub fn from_raw(raw: impl Into<String>, pairs: Vec<(String, Option<String>)>) -> Self {
        Query {
            raw: Some(raw.into()),
            pairs,
        }
    }

    /// Parse an optional raw query string into decoded ordered pairs.
    pub fn parse(raw: Option<&str>) -> Self {
        match raw {
            Some(raw) => Query::from_raw(raw, parse_pairs(raw)),
            None => Query::default(),
        }
    }

    /// Build a query from optional raw text and already-decoded pairs.
    pub fn from_parts(raw: Option<String>, pairs: Vec<(String, Option<String>)>) -> Self {
        Query { raw, pairs }
    }

    /// The original raw query string, when available.
    pub fn raw(&self) -> Option<&str> {
        self.raw.as_deref()
    }

    /// The decoded ordered query pairs.
    pub fn pairs(&self) -> &[(String, Option<String>)] {
        &self.pairs
    }

    /// Consume the query into raw text and decoded ordered pairs.
    pub fn into_parts(self) -> (Option<String>, Vec<(String, Option<String>)>) {
        (self.raw, self.pairs)
    }

    /// Render this query to the part after `?`.
    pub fn to_query_string(&self) -> String {
        match &self.raw {
            Some(raw) => raw.clone(),
            None => render_pairs(&self.pairs),
        }
    }
}

/// Parse a raw query string into ordered, decoded pairs.
pub fn parse_pairs(raw: &str) -> Vec<(String, Option<String>)> {
    raw.split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((k, v)) => (decode_component(k), Some(decode_component(v))),
            None => (decode_component(pair), None),
        })
        .collect()
}

/// Render decoded ordered pairs as a query string.
pub fn render_pairs(pairs: &[(String, Option<String>)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| match value {
            Some(value) => format!("{}={}", encode_component(key), encode_component(value)),
            None => encode_component(key),
        })
        .collect::<Vec<_>>()
        .join("&")
}

fn encode_component(value: &str) -> String {
    utf8_percent_encode(value, QUERY_COMPONENT).to_string()
}

fn decode_component(value: &str) -> String {
    let spaced = value.replace('+', " ");
    percent_decode_str(&spaced).decode_utf8_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_preserves_pair_order_and_value_shape() {
        let query = Query::parse(Some("name=bob&name=alice&flag&empty=&encoded=%40"));

        assert_eq!(
            query.raw(),
            Some("name=bob&name=alice&flag&empty=&encoded=%40")
        );
        assert_eq!(
            query.pairs(),
            [
                ("name".to_string(), Some("bob".to_string())),
                ("name".to_string(), Some("alice".to_string())),
                ("flag".to_string(), None),
                ("empty".to_string(), Some(String::new())),
                ("encoded".to_string(), Some("@".to_string())),
            ]
        );
    }

    #[test]
    fn invalid_percent_escape_stays_literal() {
        let query = Query::parse(Some("bad=%ZZ&plus=a+b"));

        assert_eq!(
            query.pairs(),
            [
                ("bad".to_string(), Some("%ZZ".to_string())),
                ("plus".to_string(), Some("a b".to_string())),
            ]
        );
    }

    #[test]
    fn decoded_pairs_render_with_bare_and_empty_values() {
        let query = Query::new(vec![
            ("name".to_string(), Some("bob".to_string())),
            ("flag".to_string(), None),
            ("empty".to_string(), Some(String::new())),
            ("encoded".to_string(), Some("@".to_string())),
        ]);

        assert_eq!(query.to_query_string(), "name=bob&flag&empty=&encoded=%40");
    }
}

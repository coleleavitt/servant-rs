//! Rendering and parsing of scalar values that travel in the URL path, the
//! query string, or headers — the analogue of Haskell's `Web.HttpApiData`
//! (`ToHttpApiData` / `FromHttpApiData`).
//!
//! These are deliberately *open* traits: downstream users implement them for
//! their own capture/parameter/header types. They are distinct from the body
//! codecs in [`crate::content`], which negotiate on media type.

use crate::modifiers::ParseError;

/// Render a value into the textual form used in a URL or header.
///
/// `to_url_piece` is the canonical rendering; the query and header variants
/// default to it but may be overridden (e.g. to match a server's lenient
/// header formatting).
pub trait ToHttpApiData {
    /// Rendering used for a path segment (a `Capture`). Not yet percent-encoded;
    /// the link/client layer escapes it.
    fn to_url_piece(&self) -> String;

    /// Rendering used for a query-string value.
    fn to_query_param(&self) -> String {
        self.to_url_piece()
    }

    /// Rendering used for a header value.
    fn to_header(&self) -> String {
        self.to_url_piece()
    }
}

/// Parse a value out of its textual URL/header form.
pub trait FromHttpApiData: Sized {
    /// Parse from a (already percent-decoded) path segment.
    fn from_url_piece(s: &str) -> Result<Self, ParseError>;

    /// Parse from a query-string value. Defaults to [`Self::from_url_piece`].
    fn from_query_param(s: &str) -> Result<Self, ParseError> {
        Self::from_url_piece(s)
    }

    /// Parse from a header value. Defaults to [`Self::from_url_piece`].
    fn from_header(s: &str) -> Result<Self, ParseError> {
        Self::from_url_piece(s)
    }
}

// --- String-like ---

impl ToHttpApiData for String {
    fn to_url_piece(&self) -> String {
        self.clone()
    }
}

impl ToHttpApiData for &str {
    fn to_url_piece(&self) -> String {
        (*self).to_owned()
    }
}

impl ToHttpApiData for str {
    fn to_url_piece(&self) -> String {
        self.to_owned()
    }
}

impl FromHttpApiData for String {
    fn from_url_piece(s: &str) -> Result<Self, ParseError> {
        Ok(s.to_owned())
    }
}

impl ToHttpApiData for char {
    fn to_url_piece(&self) -> String {
        self.to_string()
    }
}

impl FromHttpApiData for char {
    fn from_url_piece(s: &str) -> Result<Self, ParseError> {
        let mut it = s.chars();
        match (it.next(), it.next()) {
            (Some(c), None) => Ok(c),
            _ => Err(ParseError::new(format!(
                "expected a single character, got {s:?}"
            ))),
        }
    }
}

// --- bool (Servant accepts case-insensitive true/false) ---

impl ToHttpApiData for bool {
    fn to_url_piece(&self) -> String {
        if *self { "true" } else { "false" }.to_owned()
    }
}

impl FromHttpApiData for bool {
    fn from_url_piece(s: &str) -> Result<Self, ParseError> {
        match s.to_ascii_lowercase().as_str() {
            "true" => Ok(true),
            "false" => Ok(false),
            other => Err(ParseError::new(format!(
                "could not parse {other:?} as bool"
            ))),
        }
    }
}

// --- numeric types, via FromStr / Display ---

macro_rules! impl_via_fromstr_display {
    ( $( $t:ty ),* $(,)? ) => {
        $(
            impl ToHttpApiData for $t {
                fn to_url_piece(&self) -> String {
                    self.to_string()
                }
            }
            impl FromHttpApiData for $t {
                fn from_url_piece(s: &str) -> Result<Self, ParseError> {
                    s.trim().parse::<$t>().map_err(|e| {
                        ParseError::new(format!(
                            concat!("could not parse {:?} as ", stringify!($t), ": {}"),
                            s, e
                        ))
                    })
                }
            }
        )*
    };
}

impl_via_fromstr_display!(
    u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize, f32, f64,
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        assert_eq!(u64::from_url_piece(&42u64.to_url_piece()), Ok(42));
        assert_eq!(i32::from_url_piece(&(-7i32).to_url_piece()), Ok(-7));
        assert_eq!(bool::from_url_piece(&true.to_url_piece()), Ok(true));
        assert_eq!(String::from_url_piece("hi"), Ok("hi".to_owned()));
    }

    #[test]
    fn bool_is_case_insensitive() {
        assert_eq!(bool::from_url_piece("TRUE"), Ok(true));
        assert_eq!(bool::from_url_piece("False"), Ok(false));
        assert!(bool::from_url_piece("yes").is_err());
    }

    #[test]
    fn numeric_parse_errors_are_structured() {
        assert!(u8::from_url_piece("notanumber").is_err());
        assert!(u8::from_url_piece("300").is_err()); // overflow
    }

    #[test]
    fn char_requires_exactly_one() {
        assert_eq!(char::from_url_piece("x"), Ok('x'));
        assert!(char::from_url_piece("xy").is_err());
        assert!(char::from_url_piece("").is_err());
    }
}

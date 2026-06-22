//! Deep-object query parsing and rendering.

use super::encode_query_component;
use crate::modifiers::ParseError;

/// A nested path inside a deep-object query parameter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeepQueryPath {
    segments: Vec<String>,
}

impl DeepQueryPath {
    /// Build a path from decoded field names.
    pub fn new<I, S>(segments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        DeepQueryPath {
            segments: segments.into_iter().map(Into::into).collect(),
        }
    }

    /// The decoded path segments.
    pub fn segments(&self) -> &[String] {
        &self.segments
    }

    fn matches(&self, path: &[&str]) -> bool {
        self.segments.len() == path.len()
            && self
                .segments
                .iter()
                .zip(path.iter())
                .all(|(actual, expected)| actual == expected)
    }
}

/// One deep-object query entry, preserving request order and valueless keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeepQueryEntry {
    path: DeepQueryPath,
    value: Option<String>,
}

impl DeepQueryEntry {
    /// Build an entry from decoded path segments and an optional decoded value.
    pub fn new<I, S>(path: I, value: Option<String>) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        DeepQueryEntry {
            path: DeepQueryPath::new(path),
            value,
        }
    }

    /// Build a value-bearing entry.
    pub fn with_value<I, S>(path: I, value: impl Into<String>) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        DeepQueryEntry::new(path, Some(value.into()))
    }

    /// Build a valueless entry.
    pub fn flag<I, S>(path: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        DeepQueryEntry::new(path, None)
    }

    /// The decoded nested path.
    pub fn path(&self) -> &DeepQueryPath {
        &self.path
    }

    /// The decoded value, if the query key had one.
    pub fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }
}

/// Ordered deep-object query entries for one root query parameter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeepQueryParams {
    entries: Vec<DeepQueryEntry>,
}

impl DeepQueryParams {
    /// Build a parameter set from already-decoded ordered entries.
    pub fn new(entries: Vec<DeepQueryEntry>) -> Self {
        DeepQueryParams { entries }
    }

    /// Ordered entries, including duplicates and valueless keys.
    pub fn entries(&self) -> &[DeepQueryEntry] {
        &self.entries
    }

    /// Consume the parameter set into ordered entries.
    pub fn into_entries(self) -> Vec<DeepQueryEntry> {
        self.entries
    }

    /// First entry for `path`, preserving first-wins scalar semantics.
    pub fn first(&self, path: &[&str]) -> Option<&Option<String>> {
        self.entries
            .iter()
            .find(|entry| entry.path.matches(path))
            .map(|entry| &entry.value)
    }

    /// First value for `path`, ignoring later duplicates.
    pub fn first_value(&self, path: &[&str]) -> Option<&str> {
        self.first(path).and_then(Option::as_deref)
    }

    /// Every value for `path`, preserving duplicate order and skipping flags.
    pub fn values(&self, path: &[&str]) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|entry| entry.path.matches(path))
            .filter_map(DeepQueryEntry::value)
            .collect()
    }
}

/// Build a value from deep-object query entries.
pub trait FromDeepQuery: Sized {
    /// Parse from the full ordered entry list.
    fn from_deep_query(params: &DeepQueryParams) -> Result<Self, ParseError>;
}

impl FromDeepQuery for DeepQueryParams {
    fn from_deep_query(params: &DeepQueryParams) -> Result<Self, ParseError> {
        Ok(params.clone())
    }
}

/// Render a value as deep-object query entries.
pub trait ToDeepQuery {
    /// Convert to ordered entries relative to the deep-query root name.
    fn to_deep_query(&self) -> DeepQueryParams;
}

impl ToDeepQuery for DeepQueryParams {
    fn to_deep_query(&self) -> DeepQueryParams {
        self.clone()
    }
}

/// The class of deep-query bracket syntax error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeepQueryParseErrorKind {
    /// A key suffix did not begin with `[`.
    MissingOpeningBracket,
    /// A bracketed field did not close with `]`.
    MissingClosingBracket,
    /// Text appeared between bracketed fields.
    UnexpectedTextAfterBracket,
}

impl std::fmt::Display for DeepQueryParseErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeepQueryParseErrorKind::MissingOpeningBracket => {
                f.write_str("missing opening bracket in deep query key")
            }
            DeepQueryParseErrorKind::MissingClosingBracket => {
                f.write_str("missing closing bracket in deep query key")
            }
            DeepQueryParseErrorKind::UnexpectedTextAfterBracket => {
                f.write_str("unexpected text after bracket in deep query key")
            }
        }
    }
}

/// A structured deep-query parser error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{kind}")]
pub struct DeepQueryParseError {
    kind: DeepQueryParseErrorKind,
}

impl DeepQueryParseError {
    /// Build a parser error from its kind.
    pub const fn new(kind: DeepQueryParseErrorKind) -> Self {
        DeepQueryParseError { kind }
    }

    /// The parser error kind.
    pub const fn kind(&self) -> DeepQueryParseErrorKind {
        self.kind
    }
}

/// Parse decoded query pairs into deep-object entries for `root`.
pub fn parse_deep_query(
    root: &str,
    query: &[(String, Option<String>)],
) -> Result<DeepQueryParams, DeepQueryParseError> {
    let mut entries = Vec::new();
    for (key, value) in query {
        let Some(suffix) = deep_query_suffix(root, key) else {
            continue;
        };
        entries.push(DeepQueryEntry {
            path: parse_path(suffix)?,
            value: value.clone(),
        });
    }
    Ok(DeepQueryParams::new(entries))
}

/// Render a deep-object query key, preserving bracket syntax.
pub fn render_deep_query_key(root: &str, path: &DeepQueryPath) -> String {
    let mut key = encode_query_component(root);
    for segment in path.segments() {
        key.push('[');
        key.push_str(&encode_query_component(segment));
        key.push(']');
    }
    key
}

/// Render one deep-object query entry as `key` or `key=value`.
pub fn render_deep_query_entry(root: &str, entry: &DeepQueryEntry) -> String {
    let key = render_deep_query_key(root, entry.path());
    match entry.value() {
        Some(value) => format!("{key}={}", encode_query_component(value)),
        None => key,
    }
}

fn deep_query_suffix<'a>(root: &str, key: &'a str) -> Option<&'a str> {
    let suffix = key.strip_prefix(root)?;
    if suffix.is_empty() || suffix.starts_with('[') {
        Some(suffix)
    } else {
        None
    }
}

fn parse_path(suffix: &str) -> Result<DeepQueryPath, DeepQueryParseError> {
    if suffix.is_empty() {
        return Ok(DeepQueryPath::default());
    }

    let mut rest = suffix;
    let mut segments = Vec::new();
    while !rest.is_empty() {
        let after_open = rest.strip_prefix('[').ok_or_else(|| {
            DeepQueryParseError::new(DeepQueryParseErrorKind::MissingOpeningBracket)
        })?;
        let close = after_open.find(']').ok_or_else(|| {
            DeepQueryParseError::new(DeepQueryParseErrorKind::MissingClosingBracket)
        })?;
        segments.push(after_open[..close].to_string());
        rest = &after_open[close + 1..];
        if !rest.is_empty() && !rest.starts_with('[') {
            return Err(DeepQueryParseError::new(
                DeepQueryParseErrorKind::UnexpectedTextAfterBracket,
            ));
        }
    }

    Ok(DeepQueryPath::new(segments))
}

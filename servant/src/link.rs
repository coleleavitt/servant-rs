//! The link value model, mirroring `Servant.Links`.
//!
//! A [`Link`] accumulates escaped path segments, query parameters, and an
//! optional fragment while walking an API toward an endpoint. This module owns
//! the *value* and its rendering/escaping; the type-checked `HasLink` walk that
//! produces a link builder for a specific endpoint is layered on top later.

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use crate::query::{Query, render_pairs};

/// Characters that do **not** need percent-encoding in a path/query component:
/// the RFC 3986 unreserved set (`ALPHA` / `DIGIT` / `-` `_` `.` `~`).
const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// An already percent-encoded path segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Escaped(String);

impl Escaped {
    /// Escape a raw (decoded) segment into a path-safe form.
    pub fn escape(raw: &str) -> Self {
        Escaped(utf8_percent_encode(raw, COMPONENT).to_string())
    }

    /// Wrap an already-escaped segment without re-escaping.
    pub fn already_escaped(s: impl Into<String>) -> Self {
        Escaped(s.into())
    }

    /// The escaped string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A query-string parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Param {
    /// `key=value` (from `QueryParam`).
    Single(String, String),
    /// One element of a repeated key (from `QueryParams`); rendered either as
    /// `key[]=value` or `key=value` depending on [`ArrayElementStyle`].
    ArrayElem(String, String),
    /// A bare `key` with no value (from `QueryFlag`).
    Flag(String),
}

/// How `QueryParams` array elements are rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArrayElementStyle {
    /// `key[]=value` (Servant's default).
    #[default]
    Bracket,
    /// `key=value` (repeated key, no brackets).
    Plain,
}

/// An internal link: escaped segments + query parameters + optional fragment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Link {
    segments: Vec<Escaped>,
    query: Vec<Param>,
    raw_query: Option<String>,
    fragment: Option<String>,
}

impl Link {
    /// An empty link (the API root).
    pub fn new() -> Self {
        Link::default()
    }

    /// Append a raw segment, escaping it.
    pub fn add_segment(&mut self, raw: &str) {
        self.segments.push(Escaped::escape(raw));
    }

    /// Append an already-escaped segment.
    pub fn push_escaped(&mut self, seg: Escaped) {
        self.segments.push(seg);
    }

    /// Append a query parameter.
    pub fn add_query(&mut self, param: Param) {
        self.query.push(param);
    }

    /// Replace the full query string.
    pub fn set_query_string(&mut self, query: Query) {
        let (raw, pairs) = query.into_parts();
        self.raw_query = raw;
        self.query.clear();
        if self.raw_query.is_none() {
            self.query
                .extend(pairs.into_iter().map(|(key, value)| match value {
                    Some(value) => Param::Single(key, value),
                    None => Param::Flag(key),
                }));
        }
    }

    /// Set the URI fragment (the part after `#`).
    pub fn set_fragment(&mut self, frag: impl Into<String>) {
        self.fragment = Some(frag.into());
    }

    /// The escaped path segments.
    pub fn segments(&self) -> &[Escaped] {
        &self.segments
    }

    /// The query parameters.
    pub fn query(&self) -> &[Param] {
        &self.query
    }

    /// Render to a relative URL string (no leading slash), e.g.
    /// `bye?name=Hubert`. Uses [`ArrayElementStyle::Bracket`].
    pub fn to_url_piece(&self) -> String {
        self.render(ArrayElementStyle::Bracket, false)
    }

    /// Render to an absolute path (leading slash), e.g. `/bye?name=Hubert`.
    pub fn to_uri(&self) -> String {
        self.render(ArrayElementStyle::Bracket, true)
    }

    /// Render with an explicit array-element style and leading-slash choice.
    pub fn render_with(&self, style: ArrayElementStyle, leading_slash: bool) -> String {
        self.render(style, leading_slash)
    }

    fn render(&self, style: ArrayElementStyle, leading_slash: bool) -> String {
        let mut out = String::new();
        if leading_slash {
            out.push('/');
        }
        out.push_str(
            &self
                .segments
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join("/"),
        );

        if self.raw_query.is_some() || !self.query.is_empty() {
            out.push('?');
            if let Some(raw) = &self.raw_query {
                out.push_str(raw);
                if !raw.is_empty() && !self.query.is_empty() {
                    out.push('&');
                }
            }
            out.push_str(&render_params(&self.query, style));
        }

        if let Some(frag) = &self.fragment {
            out.push('#');
            out.push_str(&encode(frag));
        }
        out
    }
}

fn encode(s: &str) -> String {
    utf8_percent_encode(s, COMPONENT).to_string()
}

fn render_params(params: &[Param], style: ArrayElementStyle) -> String {
    let plain_pairs: Option<Vec<(String, Option<String>)>> = params
        .iter()
        .map(|param| match param {
            Param::Single(key, value) => Some((key.clone(), Some(value.clone()))),
            Param::Flag(key) => Some((key.clone(), None)),
            Param::ArrayElem(_, _) => None,
        })
        .collect();

    match plain_pairs {
        Some(pairs) => render_pairs(&pairs),
        None => params
            .iter()
            .map(|p| match p {
                Param::Single(k, v) => {
                    format!("{}={}", encode(k), encode(v))
                }
                Param::ArrayElem(k, v) => match style {
                    ArrayElementStyle::Bracket => format!("{}[]={}", encode(k), encode(v)),
                    ArrayElementStyle::Plain => format!("{}={}", encode(k), encode(v)),
                },
                Param::Flag(k) => encode(k),
            })
            .collect::<Vec<_>>()
            .join("&"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_path_segments() {
        assert_eq!(Escaped::escape("foo/bar").as_str(), "foo%2Fbar");
        assert_eq!(
            Escaped::escape("test@example.com").as_str(),
            "test%40example.com"
        );
        assert_eq!(Escaped::escape("a-b_c.d~e").as_str(), "a-b_c.d~e");
    }

    #[test]
    fn renders_relative_and_absolute() {
        let mut l = Link::new();
        l.add_segment("bye");
        l.add_query(Param::Single("name".into(), "Hubert".into()));
        assert_eq!(l.to_url_piece(), "bye?name=Hubert");
        assert_eq!(l.to_uri(), "/bye?name=Hubert");
    }

    #[test]
    fn flag_renders_without_equals() {
        let mut l = Link::new();
        l.add_segment("search");
        l.add_query(Param::Flag("verbose".into()));
        assert_eq!(l.to_url_piece(), "search?verbose");
    }

    #[test]
    fn array_element_styles() {
        let mut l = Link::new();
        l.add_segment("items");
        l.add_query(Param::ArrayElem("id".into(), "1".into()));
        l.add_query(Param::ArrayElem("id".into(), "2".into()));
        assert_eq!(
            l.render_with(ArrayElementStyle::Bracket, false),
            "items?id[]=1&id[]=2"
        );
        assert_eq!(
            l.render_with(ArrayElementStyle::Plain, false),
            "items?id=1&id=2"
        );
    }

    #[test]
    fn query_values_are_encoded() {
        let mut l = Link::new();
        l.add_query(Param::Single("q".into(), "a b&c".into()));
        assert_eq!(l.to_url_piece(), "?q=a%20b%26c");
    }
}

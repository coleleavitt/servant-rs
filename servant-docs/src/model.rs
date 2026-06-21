//! The documentation value model.
//!
//! Mirrors the data Haskell servant-docs reflects an API into: an `API` is a
//! list of per-endpoint `Action`s keyed by `Endpoint` (path + method). We keep a
//! flat [`ApiDoc`] of [`EndpointDoc`]s; each [`EndpointDoc`] is the accumulator
//! the [`crate::HasDocs`] walk threads through the combinator chain and finalizes
//! at the terminal verb.
//!
//! **[diff]** Servant stores the per-endpoint accumulator in a `HashMap` keyed by
//! `(path, method)` and merges colliding keys with a left-biased, non-commutative
//! `combineAction`. We keep an insertion-ordered `Vec` and merge colliding
//! `(path, method)` keys the same way (left wins on the status; list fields
//! concatenate), so the rendered ordering follows the API's left-to-right
//! structure rather than Servant's sort-by-`(path, method)`. See
//! [`ApiDoc::push`].

use mime::Mime;

/// One segment of an endpoint path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathPart {
    /// A literal path segment, e.g. `users`.
    Static(String),
    /// A single-segment capture, e.g. `Capture "id" u64`. `type_name` is the
    /// Rust type the segment is parsed as (from [`std::any::type_name`]).
    Capture {
        /// The capture's documentation/link name.
        name: String,
        /// The Rust type the segment is parsed as.
        type_name: &'static str,
    },
    /// A capture of all remaining segments, e.g. `CaptureAll "rest" String`.
    CaptureAll {
        /// The capture's documentation/link name.
        name: String,
        /// The Rust element type each remaining segment is parsed as.
        type_name: &'static str,
    },
}

impl PathPart {
    /// Render this part as it appears in a path line: a literal segment, or
    /// `:name` for a capture (Servant's markdown form).
    pub fn render(&self) -> String {
        match self {
            PathPart::Static(s) => s.clone(),
            PathPart::Capture { name, .. } => format!(":{name}"),
            PathPart::CaptureAll { name, .. } => format!(":{name}"),
        }
    }
}

/// What kind of query parameter a [`ParamDoc`] describes — mirrors Servant's
/// `ParamKind` (`Normal`/`List`/`Flag`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    /// A single-valued `QueryParam`.
    Normal,
    /// A repeated `QueryParams` (collected into a list).
    List,
    /// A valueless `QueryFlag`.
    Flag,
}

/// A documented query parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamDoc {
    /// The query key.
    pub name: String,
    /// Whether the parameter is single, a list, or a flag.
    pub kind: ParamKind,
    /// The Rust type each value is parsed as (empty for [`ParamKind::Flag`]).
    pub type_name: &'static str,
}

/// A documented request body: the content types it accepts and the Rust type it
/// is decoded as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyDoc {
    /// The media types the body may be sent as (the `ReqBody` content-type list).
    pub content_types: Vec<Mime>,
    /// The Rust type the body is decoded as.
    pub type_name: &'static str,
}

/// A documented URI fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FragmentDoc {
    /// The Rust type rendered into the URI fragment.
    pub type_name: &'static str,
    /// Human-readable fragment description.
    pub description: String,
}

/// Query-string capture metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryStringDoc {
    /// Whether decoded ordered pairs are available to the handler.
    pub decoded_ordered_pairs: bool,
    /// Whether the original raw query string is available when supplied.
    pub raw_query_available: bool,
}

/// A single fully-resolved endpoint: a path/extractor chain ending in a verb.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointDoc {
    /// The path, in segment order (static segments and captures interleaved).
    pub path: Vec<PathPart>,
    /// The HTTP method.
    pub method: http::Method,
    /// The declared success status.
    pub status: http::StatusCode,
    /// Documented query parameters, in declaration order.
    pub query_params: Vec<ParamDoc>,
    /// Names of headers the endpoint is sensitive to, in declaration order.
    pub headers: Vec<String>,
    /// The request body, if any.
    pub request_body: Option<BodyDoc>,
    /// The response content types negotiated for `Accept` (empty for 204).
    pub response_types: Vec<Mime>,
    /// An optional one-line summary (`Summary`).
    pub summary: Option<String>,
    /// An optional longer description (`Description`).
    pub description: Option<String>,
    /// An optional stable OpenAPI operation identifier (`OperationId`).
    pub operation_id: Option<String>,
    /// Optional URI fragment metadata (`Fragment`).
    pub fragment: Option<FragmentDoc>,
    /// Optional full query-string capture metadata (`QueryString`).
    pub query_string: Option<QueryStringDoc>,
}

impl EndpointDoc {
    /// A fresh accumulator: empty path, defaulting to `GET /` with status `200`.
    /// The terminal verb overwrites `method`/`status`/`response_types`.
    pub(crate) fn empty() -> Self {
        EndpointDoc {
            path: Vec::new(),
            method: http::Method::GET,
            status: http::StatusCode::OK,
            query_params: Vec::new(),
            headers: Vec::new(),
            request_body: None,
            response_types: Vec::new(),
            summary: None,
            description: None,
            operation_id: None,
            fragment: None,
            query_string: None,
        }
    }

    /// The merge key: an endpoint is "the same" as another iff its rendered path
    /// and method match. Captures compare by `:name` form, like Servant.
    fn key(&self) -> (Vec<String>, http::Method) {
        (
            self.path.iter().map(PathPart::render).collect(),
            self.method.clone(),
        )
    }

    /// Left-biased, non-commutative merge of two actions sharing a key, mirroring
    /// Servant's `combineAction`: concatenate the list fields, keep the left's
    /// status/summary/description/body, and union response content types.
    fn combine(&mut self, other: EndpointDoc) {
        self.query_params.extend(other.query_params);
        self.headers.extend(other.headers);
        for t in other.response_types {
            if !self.response_types.contains(&t) {
                self.response_types.push(t);
            }
        }
        // Status, summary, description and body come from the left (already set);
        // only fill them if the left did not provide them.
        if self.request_body.is_none() {
            self.request_body = other.request_body;
        }
        if self.summary.is_none() {
            self.summary = other.summary;
        }
        if self.description.is_none() {
            self.description = other.description;
        }
        if self.operation_id.is_none() {
            self.operation_id = other.operation_id;
        }
        if self.fragment.is_none() {
            self.fragment = other.fragment;
        }
        if self.query_string.is_none() {
            self.query_string = other.query_string;
        }
    }
}

/// The whole documentation model: every endpoint of an API, in left-to-right
/// (`Alt`) order, with endpoints sharing a `(path, method)` key merged.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApiDoc(pub Vec<EndpointDoc>);

impl ApiDoc {
    /// An empty document (the unit, contributed by `EmptyApi`).
    pub fn empty() -> Self {
        ApiDoc(Vec::new())
    }

    /// A document with a single endpoint.
    pub(crate) fn single(ep: EndpointDoc) -> Self {
        ApiDoc(vec![ep])
    }

    /// All endpoints, in order.
    pub fn endpoints(&self) -> &[EndpointDoc] {
        &self.0
    }

    /// Append `ep`, merging into an existing endpoint with the same
    /// `(path, method)` key (left-biased), else pushing it.
    pub(crate) fn push(&mut self, ep: EndpointDoc) {
        let key = ep.key();
        if let Some(existing) = self.0.iter_mut().find(|e| e.key() == key) {
            existing.combine(ep);
        } else {
            self.0.push(ep);
        }
    }

    /// Concatenate `other` into `self`, left-biased: `self`'s endpoints keep
    /// precedence; `other`'s are appended/merged in order. This realizes
    /// [`servant::api::Alt`]'s left-to-right, left-wins semantics.
    pub(crate) fn extend(&mut self, other: ApiDoc) {
        for ep in other.0 {
            self.push(ep);
        }
    }
}

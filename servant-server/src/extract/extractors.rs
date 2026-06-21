use mime::Mime;
use servant::api::{Endpoint, Header, QueryFlag, QueryParam, QueryParams, QueryString, ReqBody};
use servant::content::{AllMime, AllMimeUnrender, media_type_matches};
use servant::error::ServerError;
use servant::hlist::HCons;
use servant::http_data::FromHttpApiData;
use servant::modifiers::{ArgError, ArgShape, ParseError, Required};
use servant::query::Query;

use super::chain::{Rendered, ServerChain, bad_request, cons_tail};
use super::state::ExtractState;
use crate::result::RouteResult;

// --- QueryParam ---

impl<A, P, S, Next> ServerChain for QueryParam<A, P, S, Next>
where
    A: FromHttpApiData,
    (P, S): ArgShape<A>,
    Next: ServerChain,
    Self: Endpoint<Args = HCons<<(P, S) as ArgShape<A>>::Out, Next::Args>, Output = Next::Output>,
{
    fn validate_captures(
        &self,
        c: &[String],
        i: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        self.next.validate_captures(c, i, ca)
    }
    forward_response_checks!();
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        // A bare key (`?x`, no `=`) is ABSENT (Servant's `join`); only a key
        // with a value (`?x=` or `?x=v`) is present.
        let raw: Option<Result<A, ParseError>> = lookup_query(&st.req.query, &self.name)
            .and_then(|v| v.as_deref())
            .map(A::from_query_param);
        match <(P, S) as ArgShape<A>>::build(raw) {
            Ok(out) => cons_tail(out, &self.next, st),
            // Do not echo the raw value (it may be a secret, e.g. an API key).
            Err(ArgError::Missing) => RouteResult::FailFatal(bad_request(format!(
                "missing required query parameter `{}`",
                self.name
            ))),
            Err(ArgError::Parse(_)) => RouteResult::FailFatal(bad_request(format!(
                "could not parse query parameter `{}`",
                self.name
            ))),
        }
    }
}

// --- QueryParams ---

impl<A, Next> ServerChain for QueryParams<A, Next>
where
    A: FromHttpApiData,
    Next: ServerChain,
    Self: Endpoint<Args = HCons<Vec<A>, Next::Args>, Output = Next::Output>,
{
    fn validate_captures(
        &self,
        c: &[String],
        i: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        self.next.validate_captures(c, i, ca)
    }
    forward_response_checks!();
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        // Servant accepts both `name=` and the bracketed `name[]=` form, and
        // drops valueless entries (`mapMaybe snd`).
        let bracketed = format!("{}[]", self.name);
        let mut out = Vec::new();
        for (k, v) in &st.req.query {
            if k == &self.name || k == &bracketed {
                if let Some(s) = v.as_deref() {
                    match A::from_query_param(s) {
                        Ok(a) => out.push(a),
                        Err(_) => {
                            return RouteResult::FailFatal(bad_request(format!(
                                "could not parse query parameter `{}`",
                                self.name
                            )));
                        }
                    }
                }
            }
        }
        cons_tail(out, &self.next, st)
    }
}

// --- QueryFlag ---

impl<Next: ServerChain> ServerChain for QueryFlag<Next>
where
    Self: Endpoint<Output = Next::Output, Args = HCons<bool, Next::Args>>,
{
    fn validate_captures(
        &self,
        c: &[String],
        i: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        self.next.validate_captures(c, i, ca)
    }
    forward_response_checks!();
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        // Value-sensitive, like Servant's `examine`: absent => false; present
        // with no value => true; present with a value => only true for
        // "true"/"1"/empty (so `?flag=false` and `?flag=0` are false).
        let present = st
            .req
            .query
            .iter()
            .find(|(k, _)| k == &self.name)
            .map(|(_, v)| match v {
                None => true,
                Some(s) => s == "true" || s == "1" || s.is_empty(),
            })
            .unwrap_or(false);
        cons_tail(present, &self.next, st)
    }
}

// --- QueryString ---

impl<Next> ServerChain for QueryString<Next>
where
    Next: ServerChain,
    Self: Endpoint<Args = HCons<Query, Next::Args>, Output = Next::Output>,
{
    fn validate_captures(
        &self,
        c: &[String],
        i: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        self.next.validate_captures(c, i, ca)
    }
    forward_response_checks!();
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        cons_tail(
            Query::from_parts(st.req.raw_query.clone(), st.req.query.clone()),
            &self.next,
            st,
        )
    }
}

// --- Header ---

impl<A, P, S, Next> ServerChain for Header<A, P, S, Next>
where
    A: FromHttpApiData,
    (P, S): ArgShape<A>,
    Next: ServerChain,
    Self: Endpoint<Args = HCons<<(P, S) as ArgShape<A>>::Out, Next::Args>, Output = Next::Output>,
{
    fn validate_captures(
        &self,
        c: &[String],
        i: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        self.next.validate_captures(c, i, ca)
    }
    forward_response_checks!();
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        let raw: Option<Result<A, ParseError>> =
            st.req
                .headers
                .get(self.name.as_str())
                .map(|v| match v.to_str() {
                    Ok(s) => A::from_header(s),
                    Err(_) => Err(ParseError::new("header value is not valid text")),
                });
        match <(P, S) as ArgShape<A>>::build(raw) {
            Ok(out) => cons_tail(out, &self.next, st),
            // Do not echo header values (they may be sensitive, e.g. Authorization).
            Err(ArgError::Missing) => RouteResult::FailFatal(bad_request(format!(
                "missing required header `{}`",
                self.name
            ))),
            Err(ArgError::Parse(_)) => RouteResult::FailFatal(bad_request(format!(
                "could not parse header `{}`",
                self.name
            ))),
        }
    }
}

// --- ReqBody ---

impl<CTypes, A, S, Next> ServerChain for ReqBody<CTypes, A, S, Next>
where
    CTypes: AllMime + AllMimeUnrender<A>,
    (Required, S): ArgShape<A>,
    Next: ServerChain,
    Self: Endpoint<
            Args = HCons<<(Required, S) as ArgShape<A>>::Out, Next::Args>,
            Output = Next::Output,
        >,
{
    fn validate_captures(
        &self,
        c: &[String],
        i: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        self.next.validate_captures(c, i, ca)
    }
    fn request_content_types(&self) -> Option<Vec<Mime>> {
        Some(CTypes::all_media_types())
    }
    fn accept_check(&self, accept: Option<&str>) -> RouteResult<()> {
        self.next.accept_check(accept)
    }
    fn render(&self, accept: Option<&str>, value: Self::Output) -> Rendered {
        self.next.render(accept, value)
    }
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        let decoded =
            servant::content::negotiate_content::<CTypes, A>(st.req.content_type(), &st.req.body);
        let raw: Option<Result<A, ParseError>> = match decoded {
            // Content-type was already validated (415) before extraction.
            None => return RouteResult::FailFatal(ServerError::err415()),
            Some(r) => Some(r.map_err(ParseError::new)),
        };
        match <(Required, S) as ArgShape<A>>::build(raw) {
            Ok(out) => cons_tail(out, &self.next, st),
            Err(ArgError::Missing) => {
                RouteResult::FailFatal(bad_request("request body is required"))
            }
            // Do not echo the decode error (it may quote body content, which
            // can contain secrets such as a password field).
            Err(ArgError::Parse(_)) => {
                RouteResult::FailFatal(bad_request("could not parse request body"))
            }
        }
    }
}

fn lookup_query<'a>(
    query: &'a [(String, Option<String>)],
    name: &str,
) -> Option<&'a Option<String>> {
    query.iter().find(|(k, _)| k == name).map(|(_, v)| v)
}

/// The content type the request body's media types must match (phase-5 415
/// check), using the server-request octet-stream default for a missing header.
pub fn content_type_acceptable(body_types: &[Mime], content_type: Option<&str>) -> bool {
    let ct: Mime = match content_type {
        Some(s) => match s.trim().parse() {
            Ok(m) => m,
            Err(_) => return false,
        },
        None => mime::APPLICATION_OCTET_STREAM,
    };
    body_types.iter().any(|m| media_type_matches(m, &ct))
}

use servant::api::{DeepQuery, Endpoint};
use servant::hlist::HCons;
use servant::query::{FromDeepQuery, parse_deep_query};

use super::chain::{Rendered, ServerChain, bad_request, cons_tail};
use super::state::ExtractState;
use crate::result::RouteResult;

impl<A, Next> ServerChain for DeepQuery<A, Next>
where
    A: FromDeepQuery,
    Next: ServerChain,
    Self: Endpoint<Args = HCons<A, Next::Args>, Output = Next::Output>,
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
        let params = match parse_deep_query(&self.name, &st.req.query) {
            Ok(params) => params,
            Err(_) => {
                return RouteResult::FailFatal(bad_request(format!(
                    "could not parse deep query parameter `{}`",
                    self.name
                )));
            }
        };
        match A::from_deep_query(&params) {
            Ok(out) => cons_tail(out, &self.next, st),
            Err(_) => RouteResult::FailFatal(bad_request(format!(
                "could not parse deep query parameter `{}`",
                self.name
            ))),
        }
    }
}

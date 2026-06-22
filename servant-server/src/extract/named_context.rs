use servant::api::{Endpoint, WithNamedContext};

use super::chain::{Rendered, ServerChain};
use super::state::ExtractState;
use crate::result::RouteResult;

fn with_named_context<Name, R>(
    st: &mut ExtractState<'_>,
    f: impl FnOnce(&mut ExtractState<'_>) -> R,
) -> R
where
    Name: 'static,
{
    if let Some(named) = st.lookup_ctx::<crate::context::NamedContext<Name>>() {
        st.push_ctx(&named.context);
        let result = f(st);
        st.pop_ctx();
        result
    } else {
        f(st)
    }
}

impl<Name, Next> ServerChain for WithNamedContext<Name, Next>
where
    Name: 'static,
    Next: ServerChain,
    Self: Endpoint<Output = Next::Output, Args = Next::Args>,
{
    fn validate_captures(
        &self,
        c: &[String],
        i: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        self.next.validate_captures(c, i, ca)
    }
    forward_response_checks_without_pre_body!();

    fn pre_body_check(&self, st: &mut ExtractState<'_>) -> RouteResult<()> {
        with_named_context::<Name, _>(st, |st| self.next.pre_body_check(st))
    }

    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        with_named_context::<Name, _>(st, |st| self.next.extract(st))
    }
}

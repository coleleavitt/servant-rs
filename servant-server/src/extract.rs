//! Request extraction: the `ServerChain` trait and its per-combinator impls.
//!
//! Mirrors Servant's `Delayed` pipeline. The leaf (see [`crate::handler`]) runs
//! the phases in Servant's order — captures → method → accept → content-type →
//! (query/header/body) — short-circuiting on the first failure, with the
//! `Fail`/`FailFatal` distinction per combinator:
//!
//! - **`Fail`** (recoverable): capture parse failure, content-type unsupported.
//! - **`FailFatal`** (commit): missing-required/strict-parse of query, header,
//!   or body (all 400).
//!
//! See `docs/DESIGN.md` for the one documented ordering difference: capture
//! parse validation runs at the leaf via [`ServerChain::validate_captures`]
//! (phase 1, before method), and query/header/body extraction follows
//! combinator order rather than strict query→header→body grouping.

macro_rules! forward_response_checks {
    () => {
        fn request_content_types(&self) -> Option<Vec<mime::Mime>> {
            self.next.request_content_types()
        }
        fn accept_check(&self, accept: Option<&str>) -> crate::result::RouteResult<()> {
            self.next.accept_check(accept)
        }
        fn render(&self, accept: Option<&str>, value: Self::Output) -> Rendered {
            self.next.render(accept, value)
        }
    };
}

mod auth;
mod captures;
mod chain;
mod extractors;
mod state;
mod terminal;
mod wrappers;

pub use chain::{Rendered, ServerChain};
pub use extractors::content_type_acceptable;
pub use state::ExtractState;

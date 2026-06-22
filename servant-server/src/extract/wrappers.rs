use std::sync::Arc;

use http::Extensions;
use servant::api::{
    Description,
    Endpoint,
    Fragment,
    Host,
    HttpVersion,
    IsSecure,
    OperationId,
    Path,
    RemoteHost,
    Summary,
    Vault,
    WithNamedContext,
    WithResource,
};
use servant::error::ServerError;
use servant::hlist::HCons;
use servant::host::HostRequirement;

use super::chain::{Rendered, ServerChain, cons_tail};
use super::state::ExtractState;
use crate::request::RequestData;
use crate::result::RouteResult;

// --- Path (no arg, no capture) ---

impl<Next: ServerChain> ServerChain for Path<Next>
where
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
    forward_response_checks!();
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        self.next.extract(st)
    }
}

// --- Host (recoverable routing check) ---

impl<Next: ServerChain> ServerChain for Host<Next>
where
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

    fn host_check(&self, req: &RequestData) -> RouteResult<()> {
        let Ok(required) = HostRequirement::parse(&self.name) else {
            return RouteResult::Fail(ServerError::err404());
        };
        match req.host_authority() {
            Some(authority) if required.matches_authority(authority) => self.next.host_check(req),
            Some(_) | None => RouteResult::Fail(ServerError::err404()),
        }
    }

    fn request_content_types(&self) -> Option<Vec<mime::Mime>> {
        self.next.request_content_types()
    }

    fn accept_check(&self, accept: Option<&str>) -> RouteResult<()> {
        self.next.accept_check(accept)
    }

    fn render(&self, accept: Option<&str>, value: Self::Output) -> Rendered {
        self.next.render(accept, value)
    }

    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        self.next.extract(st)
    }
}

// --- Description / Summary (metadata) ---

macro_rules! metadata_chain {
    ($ty:ident < $($g:ident),+ >) => {
        impl<$($g),+, Next: ServerChain> ServerChain for $ty<$($g),+, Next>
        where
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
            forward_response_checks!();
            fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
                self.next.extract(st)
            }
        }
    };
    ($ty:ident) => {
        impl<Next: ServerChain> ServerChain for $ty<Next>
        where
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
            forward_response_checks!();
            fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
                self.next.extract(st)
            }
        }
    };
}
metadata_chain!(Description);
metadata_chain!(Summary);
metadata_chain!(OperationId);
metadata_chain!(Fragment<A>);

// --- Vault (server-only; provides the request's Extensions) ---

impl<Next: ServerChain> ServerChain for Vault<Next>
where
    Self: Endpoint<Output = Next::Output, Args = HCons<Arc<Extensions>, Next::Args>>,
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
        let ext = st.req.extensions.clone();
        cons_tail(ext, &self.next, st)
    }
}

// --- WithResource (server-only; allocates R from the context) ---

impl<R, Next> ServerChain for WithResource<R, Next>
where
    R: Send + Sync + 'static,
    Next: ServerChain,
    Self: Endpoint<Args = HCons<R, Next::Args>, Output = Next::Output>,
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
        let Some(provider) = st.lookup_ctx::<crate::context::ResourceProvider<R>>() else {
            return RouteResult::FailFatal(
                ServerError::err500().with_body("resource provider not configured in context"),
            );
        };
        let resource = (provider.0)();
        cons_tail(resource, &self.next, st)
    }
}

// --- Request-info combinators (server-only) ---

impl<Next: ServerChain> ServerChain for IsSecure<Next>
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
        let secure = st.req.is_secure;
        cons_tail(secure, &self.next, st)
    }
}

impl<Next: ServerChain> ServerChain for HttpVersion<Next>
where
    Self: Endpoint<Output = Next::Output, Args = HCons<http::Version, Next::Args>>,
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
        let version = st.req.version;
        cons_tail(version, &self.next, st)
    }
}

impl<Next: ServerChain> ServerChain for RemoteHost<Next>
where
    Self: Endpoint<Output = Next::Output, Args = HCons<Option<std::net::SocketAddr>, Next::Args>>,
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
        let addr = st.req.remote_addr;
        cons_tail(addr, &self.next, st)
    }
}

// --- WithNamedContext (named sub-context scope) ---

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
    forward_response_checks!();
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        if let Some(named) = st.lookup_ctx::<crate::context::NamedContext<Name>>() {
            st.push_ctx(&named.context);
            let result = self.next.extract(st);
            st.pop_ctx();
            result
        } else {
            self.next.extract(st)
        }
    }
}

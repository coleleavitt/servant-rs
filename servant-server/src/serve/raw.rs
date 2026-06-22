use std::future::Future;
use std::sync::Arc;

use servant::api::{Description, Fragment, Host, OperationId, Path, Raw, RawM, Summary};
use servant::error::ServerError;
use servant::host::HostRequirement;

use super::RouterShape;
use super::kind::{BuildServerByKind, RawServer};
use crate::context::Context;
use crate::raw::{RawLeaf, RawMLeaf, RawRequest};
use crate::request::RequestData;
use crate::result::RouteResult;
use crate::router::{BoxRouteFuture, Leaf, LeafService, Router};

impl RouterShape for Raw {
    fn shape(&self, leaf: Leaf) -> Router {
        Router::Raw(leaf)
    }
}

impl RouterShape for RawM {
    fn shape(&self, leaf: Leaf) -> Router {
        Router::Raw(leaf)
    }
}

pub(super) trait HasRawServer<H> {
    fn into_raw_router(self, handler: H, context: Arc<Context>) -> Router;
}

impl<Api, H> BuildServerByKind<RawServer, H> for Api
where
    Api: HasRawServer<H>,
{
    fn into_router_for_kind(self, handler: H, context: Arc<Context>) -> Router {
        self.into_raw_router(handler, context)
    }
}

impl<H, Fut> HasRawServer<H> for Raw
where
    H: Fn(RawRequest) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = http::Response<crate::response::ResponseBody>> + Send + 'static,
{
    fn into_raw_router(self, handler: H, _context: Arc<Context>) -> Router {
        let leaf: Leaf = Arc::new(RawLeaf {
            handler: Arc::new(handler),
        });
        self.shape(leaf)
    }
}

impl<H, Fut> HasRawServer<H> for RawM
where
    H: Fn(RawRequest, Arc<Context>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<http::Response<crate::response::ResponseBody>, ServerError>>
        + Send
        + 'static,
{
    fn into_raw_router(self, handler: H, context: Arc<Context>) -> Router {
        let leaf: Leaf = Arc::new(RawMLeaf {
            handler: Arc::new(handler),
            context,
        });
        self.shape(leaf)
    }
}

impl<Next, H> HasRawServer<H> for Path<Next>
where
    Next: HasRawServer<H>,
{
    fn into_raw_router(self, handler: H, context: Arc<Context>) -> Router {
        Router::path(self.segment, self.next.into_raw_router(handler, context))
    }
}

impl<Next, H> HasRawServer<H> for Host<Next>
where
    Next: HasRawServer<H>,
{
    fn into_raw_router(self, handler: H, context: Arc<Context>) -> Router {
        let guard = HostGuard::from_name(&self.name);
        guard_raw_router(self.next.into_raw_router(handler, context), Arc::new(guard))
    }
}

macro_rules! metadata_raw_server {
    ($ty:ident < $($g:ident),+ >) => {
        impl<$($g),+, Next, H> HasRawServer<H> for $ty<$($g),+, Next>
        where
            Next: HasRawServer<H>,
        {
            fn into_raw_router(self, handler: H, context: Arc<Context>) -> Router {
                self.next.into_raw_router(handler, context)
            }
        }
    };
    ($ty:ident) => {
        impl<Next, H> HasRawServer<H> for $ty<Next>
        where
            Next: HasRawServer<H>,
        {
            fn into_raw_router(self, handler: H, context: Arc<Context>) -> Router {
                self.next.into_raw_router(handler, context)
            }
        }
    };
}

metadata_raw_server!(Description);
metadata_raw_server!(Summary);
metadata_raw_server!(OperationId);
metadata_raw_server!(Fragment<A>);

enum HostGuard {
    Match(HostRequirement),
    Invalid,
}

impl HostGuard {
    fn from_name(name: &str) -> Self {
        match HostRequirement::parse(name) {
            Ok(requirement) => HostGuard::Match(requirement),
            Err(_) => HostGuard::Invalid,
        }
    }

    fn check(&self, req: &RequestData) -> RouteResult<()> {
        match self {
            HostGuard::Match(required) => match req.host_authority() {
                Some(authority) if required.matches_authority(authority) => RouteResult::Route(()),
                Some(_) | None => RouteResult::Fail(ServerError::err404()),
            },
            HostGuard::Invalid => RouteResult::Fail(ServerError::err404()),
        }
    }
}

struct HostGuardLeaf {
    guard: Arc<HostGuard>,
    inner: Leaf,
}

impl LeafService for HostGuardLeaf {
    fn call<'a>(
        &'a self,
        req: &'a RequestData,
        tail: Vec<String>,
        captures: Vec<String>,
        capture_all: Option<Vec<String>>,
    ) -> BoxRouteFuture<'a> {
        match self.guard.check(req) {
            RouteResult::Route(()) => self.inner.call(req, tail, captures, capture_all),
            RouteResult::Fail(error) => Box::pin(async move { RouteResult::Fail(error) }),
            RouteResult::FailFatal(error) => Box::pin(async move { RouteResult::FailFatal(error) }),
        }
    }
}

fn guard_leaf(leaf: Leaf, guard: Arc<HostGuard>) -> Leaf {
    Arc::new(HostGuardLeaf { guard, inner: leaf })
}

fn guard_raw_router(router: Router, guard: Arc<HostGuard>) -> Router {
    match router {
        Router::Static(map, leaves) => Router::Static(
            map.into_iter()
                .map(|(segment, inner)| (segment, guard_raw_router(inner, guard.clone())))
                .collect(),
            leaves
                .into_iter()
                .map(|leaf| guard_leaf(leaf, guard.clone()))
                .collect(),
        ),
        Router::Capture(hints, inner) => {
            Router::Capture(hints, Box::new(guard_raw_router(*inner, guard)))
        }
        Router::CaptureAll(hints, inner) => {
            Router::CaptureAll(hints, Box::new(guard_raw_router(*inner, guard)))
        }
        Router::Raw(leaf) => Router::Raw(guard_leaf(leaf, guard)),
        Router::Choice(left, right) => Router::Choice(
            Box::new(guard_raw_router(*left, guard.clone())),
            Box::new(guard_raw_router(*right, guard)),
        ),
    }
}

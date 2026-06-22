//! Assembling a [`Router`] from an API description and its handlers.
//!
//! [`RouterShape`] walks the path-bearing combinators to build the routing tree
//! around a single leaf; [`HasServer`] creates that leaf (binding the handler)
//! and, for [`Alt`], merges the two sub-routers with the [`choice`]
//! smart-constructor — left-biased, exactly as Servant's `:<|>`.

use std::sync::Arc;

use servant::api::{
    Alt,
    AuthProtect,
    BasicAuth,
    Capture,
    CaptureAll,
    DeepQuery,
    Description,
    Fragment,
    Header,
    Host,
    HttpVersion,
    IsSecure,
    NoContentVerb,
    OperationId,
    Path,
    QueryFlag,
    QueryParam,
    QueryParams,
    QueryString,
    RemoteHost,
    ReqBody,
    StreamBody,
    Summary,
    Vault,
    Verb,
    WithNamedContext,
    WithResource,
};

use crate::context::Context;
use crate::router::{CaptureHint, Leaf, Router, choice};

mod kind;
mod raw;

use kind::{BuildServerByKind, ServerKind};

/// Build the routing tree for a single endpoint around `leaf`.
pub trait RouterShape {
    /// Wrap `leaf` in this combinator's path structure.
    fn shape(&self, leaf: Leaf) -> Router;
}

impl<Next: RouterShape> RouterShape for Path<Next> {
    fn shape(&self, leaf: Leaf) -> Router {
        Router::path(self.segment.clone(), self.next.shape(leaf))
    }
}

impl<A, S, Next: RouterShape> RouterShape for Capture<A, S, Next> {
    fn shape(&self, leaf: Leaf) -> Router {
        Router::Capture(
            vec![CaptureHint {
                name: self.name.clone(),
                type_name: std::any::type_name::<A>(),
            }],
            Box::new(self.next.shape(leaf)),
        )
    }
}

impl<A, Next: RouterShape> RouterShape for CaptureAll<A, Next> {
    fn shape(&self, leaf: Leaf) -> Router {
        Router::CaptureAll(
            vec![CaptureHint {
                name: self.name.clone(),
                type_name: std::any::type_name::<Vec<A>>(),
            }],
            Box::new(self.next.shape(leaf)),
        )
    }
}

macro_rules! forward_shape {
    ($ty:ident < $($g:ident),+ >) => {
        impl<$($g),+, Next: RouterShape> RouterShape for $ty<$($g),+, Next> {
            fn shape(&self, leaf: Leaf) -> Router {
                self.next.shape(leaf)
            }
        }
    };
    ($ty:ident) => {
        impl<Next: RouterShape> RouterShape for $ty<Next> {
            fn shape(&self, leaf: Leaf) -> Router {
                self.next.shape(leaf)
            }
        }
    };
}
forward_shape!(QueryParam<A, P, S>);
forward_shape!(QueryParams<A>);
forward_shape!(QueryString);
forward_shape!(DeepQuery<A>);
forward_shape!(Header<A, P, S>);
forward_shape!(Host);
forward_shape!(ReqBody<CTypes, A, S>);
forward_shape!(StreamBody<Framing, CType, T>);
forward_shape!(Fragment<A>);
forward_shape!(QueryFlag);
forward_shape!(Description);
forward_shape!(Summary);
forward_shape!(OperationId);
forward_shape!(Vault);
forward_shape!(WithResource<R>);
forward_shape!(BasicAuth<Usr>);
forward_shape!(AuthProtect<Usr>);
forward_shape!(IsSecure);
forward_shape!(HttpVersion);
forward_shape!(RemoteHost);
forward_shape!(WithNamedContext<Name>);

impl<M, const STATUS: u16, CTypes, A> RouterShape for Verb<M, STATUS, CTypes, A> {
    fn shape(&self, leaf: Leaf) -> Router {
        Router::leaf(leaf)
    }
}

impl<M> RouterShape for NoContentVerb<M> {
    fn shape(&self, leaf: Leaf) -> Router {
        Router::leaf(leaf)
    }
}

impl<M, const STATUS: u16, CTypes, A> RouterShape
    for servant::api::VerbWithHeaders<M, STATUS, CTypes, A>
{
    fn shape(&self, leaf: Leaf) -> Router {
        Router::leaf(leaf)
    }
}

impl<M, CTypes, Resp> RouterShape for servant::api::UVerb<M, CTypes, Resp> {
    fn shape(&self, leaf: Leaf) -> Router {
        Router::leaf(leaf)
    }
}

impl<M, const STATUS: u16, Framing, CType, T> RouterShape
    for servant::api::StreamVerb<M, STATUS, Framing, CType, T>
{
    fn shape(&self, leaf: Leaf) -> Router {
        Router::leaf(leaf)
    }
}

/// Bind an API description and matching handler(s) into a [`Router`].
///
/// For a single endpoint, `H` is the handler closure (its parameters are checked
/// against the endpoint's [`servant::api::HasArgs::Args`]); for [`Alt`], `H` is a
/// `(left, right)` handler pair mirroring the API's structure.
pub trait HasServer<H> {
    /// Produce the router, given the shared server context.
    fn into_router(self, handler: H, context: Arc<Context>) -> Router;
}

impl<Api, H> HasServer<H> for Api
where
    Api: ServerKind + BuildServerByKind<<Api as ServerKind>::Kind, H>,
{
    fn into_router(self, handler: H, context: Arc<Context>) -> Router {
        self.into_router_for_kind(handler, context)
    }
}

impl<L, R, HL, HR> HasServer<(HL, HR)> for Alt<L, R>
where
    L: HasServer<HL>,
    R: HasServer<HR>,
{
    fn into_router(self, handler: (HL, HR), context: Arc<Context>) -> Router {
        choice(
            self.left.into_router(handler.0, context.clone()),
            self.right.into_router(handler.1, context),
        )
    }
}

/// Build a [`Router`] from an API description and its handler(s).
pub fn serve<Api, H>(api: Api, handler: H) -> Router
where
    Api: HasServer<H>,
{
    api.into_router(handler, Arc::new(Context::new()))
}

/// Build a [`Router`] with a server [`Context`] (auth checks, resource
/// providers) available to combinators like `BasicAuth` and `WithResource`.
pub fn serve_with_context<Api, H>(api: Api, handler: H, context: Context) -> Router
where
    Api: HasServer<H>,
{
    api.into_router(handler, Arc::new(context))
}

/// Render the API's routing tree as text, for debugging — analogous to
/// Servant's `layout`.
pub fn layout(router: &Router) -> String {
    let mut out = String::from("/\n");
    fn go(r: &Router, indent: usize, out: &mut String) {
        let pad = "  ".repeat(indent);
        match r {
            Router::Static(map, leaves) => {
                for (seg, inner) in map {
                    out.push_str(&format!("{pad}{seg}/\n"));
                    go(inner, indent + 1, out);
                }
                for _ in leaves {
                    out.push_str(&format!("{pad}•\n"));
                }
            }
            Router::Capture(hints, inner) => {
                let names: Vec<&str> = hints.iter().map(|h| h.name.as_str()).collect();
                out.push_str(&format!("{pad}<{}>/\n", names.join("|")));
                go(inner, indent + 1, out);
            }
            Router::CaptureAll(hints, inner) => {
                let names: Vec<&str> = hints.iter().map(|h| h.name.as_str()).collect();
                out.push_str(&format!("{pad}<{}...>/\n", names.join("|")));
                go(inner, indent + 1, out);
            }
            Router::Raw(_) => out.push_str(&format!("{pad}<raw>\n")),
            Router::Choice(a, b) => {
                go(a, indent, out);
                out.push_str(&format!("{pad}┆\n"));
                go(b, indent, out);
            }
        }
    }
    go(router, 0, &mut out);
    out
}

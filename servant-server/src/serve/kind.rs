use std::future::Future;
use std::sync::Arc;

use servant::api::{
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
    Raw,
    RawM,
    RemoteHost,
    ReqBody,
    Summary,
    Vault,
    Verb,
    WithNamedContext,
    WithResource,
};
use servant::error::ServerError;
use servant::func::HandlerFn;

use super::RouterShape;
use crate::context::Context;
use crate::extract::ServerChain;
use crate::handler::EndpointLeaf;
use crate::router::{Leaf, Router};

pub(super) struct TypedServer;

pub(super) struct RawServer;

pub(super) trait ServerKind {
    type Kind;
}

impl<M, const STATUS: u16, CTypes, A> ServerKind for Verb<M, STATUS, CTypes, A> {
    type Kind = TypedServer;
}

impl<M> ServerKind for NoContentVerb<M> {
    type Kind = TypedServer;
}

impl<M, const STATUS: u16, CTypes, A> ServerKind
    for servant::api::VerbWithHeaders<M, STATUS, CTypes, A>
{
    type Kind = TypedServer;
}

impl<M, CTypes, Resp> ServerKind for servant::api::UVerb<M, CTypes, Resp> {
    type Kind = TypedServer;
}

impl<M, const STATUS: u16, Framing, CType, T> ServerKind
    for servant::api::StreamVerb<M, STATUS, Framing, CType, T>
{
    type Kind = TypedServer;
}

impl ServerKind for Raw {
    type Kind = RawServer;
}

impl ServerKind for RawM {
    type Kind = RawServer;
}

macro_rules! forward_server_kind {
    ($ty:ident < $($g:ident),+ >) => {
        impl<$($g),+, Next> ServerKind for $ty<$($g),+, Next>
        where
            Next: ServerKind,
        {
            type Kind = Next::Kind;
        }
    };
    ($ty:ident) => {
        impl<Next> ServerKind for $ty<Next>
        where
            Next: ServerKind,
        {
            type Kind = Next::Kind;
        }
    };
}

forward_server_kind!(Path);
forward_server_kind!(Capture<A, S>);
forward_server_kind!(CaptureAll<A>);
forward_server_kind!(QueryParam<A, P, S>);
forward_server_kind!(QueryParams<A>);
forward_server_kind!(QueryString);
forward_server_kind!(DeepQuery<A>);
forward_server_kind!(Header<A, P, S>);
forward_server_kind!(Host);
forward_server_kind!(ReqBody<CTypes, A, S>);
forward_server_kind!(Fragment<A>);
forward_server_kind!(QueryFlag);
forward_server_kind!(Description);
forward_server_kind!(Summary);
forward_server_kind!(OperationId);
forward_server_kind!(Vault);
forward_server_kind!(WithResource<R>);
forward_server_kind!(BasicAuth<Usr>);
forward_server_kind!(AuthProtect<Usr>);
forward_server_kind!(IsSecure);
forward_server_kind!(HttpVersion);
forward_server_kind!(RemoteHost);
forward_server_kind!(WithNamedContext<Name>);

pub(super) trait BuildServerByKind<Kind, H> {
    fn into_router_for_kind(self, handler: H, context: Arc<Context>) -> Router;
}

impl<Api, H, Fut> BuildServerByKind<TypedServer, H> for Api
where
    Api: ServerChain + RouterShape + Send + Sync + 'static,
    Api::Args: Send,
    Api::Output: Send,
    H: HandlerFn<Api::Args, Output = Fut> + Send + Sync + 'static,
    Fut: Future<Output = Result<Api::Output, ServerError>> + Send + 'static,
{
    fn into_router_for_kind(self, handler: H, context: Arc<Context>) -> Router {
        let api = Arc::new(self);
        let leaf: Leaf = Arc::new(EndpointLeaf {
            chain: api.clone(),
            handler: Arc::new(handler),
            context,
        });
        api.shape(leaf)
    }
}

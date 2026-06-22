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
    Raw,
    RawM,
    RemoteHost,
    ReqBody,
    StreamBody,
    Summary,
    Vault,
    Verb,
    WithNamedContext,
    WithResource,
};

use super::endpoint::{ClientEndpoint, HasClient, HasRawClient, MakeClient, RawClientEndpoint};

#[doc(hidden)]
pub struct TypedClient;

#[doc(hidden)]
pub struct RawClient;

#[doc(hidden)]
pub trait ClientKind {
    type Kind;
}

impl<M, const STATUS: u16, CTypes, A> ClientKind for Verb<M, STATUS, CTypes, A> {
    type Kind = TypedClient;
}

impl<M> ClientKind for NoContentVerb<M> {
    type Kind = TypedClient;
}

impl<M, const STATUS: u16, CTypes, A> ClientKind
    for servant::api::VerbWithHeaders<M, STATUS, CTypes, A>
{
    type Kind = TypedClient;
}

impl<M, CTypes, Resp> ClientKind for servant::api::UVerb<M, CTypes, Resp> {
    type Kind = TypedClient;
}

impl<M, const STATUS: u16, Fr, CType, T> ClientKind
    for servant::api::StreamVerb<M, STATUS, Fr, CType, T>
{
    type Kind = TypedClient;
}

impl ClientKind for Raw {
    type Kind = RawClient;
}

impl ClientKind for RawM {
    type Kind = RawClient;
}

macro_rules! forward_client_kind {
    ($ty:ident < $($g:ident),+ >) => {
        impl<$($g),+, Next> ClientKind for $ty<$($g),+, Next>
        where
            Next: ClientKind,
        {
            type Kind = Next::Kind;
        }
    };
    ($ty:ident) => {
        impl<Next> ClientKind for $ty<Next>
        where
            Next: ClientKind,
        {
            type Kind = Next::Kind;
        }
    };
}

forward_client_kind!(Path);
forward_client_kind!(Capture<A, S>);
forward_client_kind!(CaptureAll<A>);
forward_client_kind!(QueryParam<A, P, S>);
forward_client_kind!(QueryParams<A>);
forward_client_kind!(QueryString);
forward_client_kind!(DeepQuery<A>);
forward_client_kind!(Header<A, P, S>);
forward_client_kind!(Host);
forward_client_kind!(ReqBody<CTypes, A, S>);
forward_client_kind!(StreamBody<Framing, CType, T>);
forward_client_kind!(Fragment<A>);
forward_client_kind!(QueryFlag);
forward_client_kind!(Description);
forward_client_kind!(Summary);
forward_client_kind!(OperationId);
forward_client_kind!(Vault);
forward_client_kind!(WithResource<R>);
forward_client_kind!(BasicAuth<Usr>);
forward_client_kind!(AuthProtect<Usr>);
forward_client_kind!(IsSecure);
forward_client_kind!(HttpVersion);
forward_client_kind!(RemoteHost);
forward_client_kind!(WithNamedContext<Name>);

#[doc(hidden)]
pub trait BuildClientByKind<Kind> {
    type Client;

    fn make_client_for_kind(self) -> Self::Client;
}

impl<Api> BuildClientByKind<TypedClient> for Api
where
    Api: HasClient,
{
    type Client = ClientEndpoint<Api>;

    fn make_client_for_kind(self) -> Self::Client {
        ClientEndpoint {
            chain: Arc::new(self),
        }
    }
}

impl<Api> BuildClientByKind<RawClient> for Api
where
    Api: HasRawClient,
{
    type Client = RawClientEndpoint<Api>;

    fn make_client_for_kind(self) -> Self::Client {
        RawClientEndpoint {
            chain: Arc::new(self),
        }
    }
}

impl<Api> MakeClient for Api
where
    Api: ClientKind + BuildClientByKind<<Api as ClientKind>::Kind>,
{
    type Client = <Api as BuildClientByKind<<Api as ClientKind>::Kind>>::Client;

    fn make_client(self) -> Self::Client {
        self.make_client_for_kind()
    }
}

impl<L, R> MakeClient for Alt<L, R>
where
    L: MakeClient,
    R: MakeClient,
{
    type Client = (L::Client, R::Client);

    fn make_client(self) -> Self::Client {
        (self.left.make_client(), self.right.make_client())
    }
}

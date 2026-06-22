//! Type-safe link generation, mirroring `Servant.Links` (`HasLink`/`MkLink`/
//! `safeLink`).
//!
//! [`links`] walks the **same** API description, producing a link builder per
//! endpoint. The builder's argument list ([`HasLink::LinkArgs`]) contains only
//! the path- and query-bearing combinators (captures, query params/flags) — not
//! headers or request bodies, which don't appear in a URL — so a link can only
//! be built for an endpoint that actually exists in the API, with exactly the
//! values its URL needs. This is the fourth interpretation of the one
//! description, alongside server, client, and docs.

use std::sync::Arc;

use crate::api::{
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
use crate::hlist::{HCons, HList, HNil};
use crate::http_data::ToHttpApiData;
use crate::link::{Link, Param};
use crate::modifiers::{ArgShape, CaptureShape};
use crate::query::{Query, ToDeepQuery, render_deep_query_key};

/// Walk an endpoint, contributing its path segments and query parameters to a
/// [`Link`]. The `LinkArgs` HList omits header/body arguments.
pub trait HasLink {
    /// The values needed to build a link to this endpoint (captures + query).
    type LinkArgs: HList;
    /// Contribute this combinator's segments/params, consuming its args.
    fn add_to_link(&self, args: Self::LinkArgs, link: &mut Link);
}

impl<M, const STATUS: u16, CTypes, A> HasLink for Verb<M, STATUS, CTypes, A> {
    type LinkArgs = HNil;
    fn add_to_link(&self, _args: HNil, _link: &mut Link) {}
}

impl<M> HasLink for NoContentVerb<M> {
    type LinkArgs = HNil;
    fn add_to_link(&self, _args: HNil, _link: &mut Link) {}
}

impl<M, const STATUS: u16, CTypes, A> HasLink
    for crate::api::VerbWithHeaders<M, STATUS, CTypes, A>
{
    type LinkArgs = HNil;
    fn add_to_link(&self, _args: HNil, _link: &mut Link) {}
}

impl<M, CTypes, Resp> HasLink for crate::api::UVerb<M, CTypes, Resp> {
    type LinkArgs = HNil;
    fn add_to_link(&self, _args: HNil, _link: &mut Link) {}
}

impl<M, const STATUS: u16, Framing, CType, T> HasLink
    for crate::api::StreamVerb<M, STATUS, Framing, CType, T>
{
    type LinkArgs = HNil;
    fn add_to_link(&self, _args: HNil, _link: &mut Link) {}
}

impl<Next: HasLink> HasLink for Path<Next> {
    type LinkArgs = Next::LinkArgs;
    fn add_to_link(&self, args: Self::LinkArgs, link: &mut Link) {
        link.add_segment(&self.segment);
        self.next.add_to_link(args, link);
    }
}

impl<A, S, Next> HasLink for Capture<A, S, Next>
where
    A: ToHttpApiData,
    S: CaptureShape<A>,
    Next: HasLink,
{
    type LinkArgs = HCons<<S as CaptureShape<A>>::Out, Next::LinkArgs>;
    fn add_to_link(&self, args: Self::LinkArgs, link: &mut Link) {
        let HCons { head, tail } = args;
        match <S as CaptureShape<A>>::into_value(head) {
            Some(a) => link.add_segment(&a.to_url_piece()),
            None => link.add_segment(""),
        }
        self.next.add_to_link(tail, link);
    }
}

impl<A, Next> HasLink for CaptureAll<A, Next>
where
    A: ToHttpApiData,
    Next: HasLink,
{
    type LinkArgs = HCons<Vec<A>, Next::LinkArgs>;
    fn add_to_link(&self, args: Self::LinkArgs, link: &mut Link) {
        let HCons { head, tail } = args;
        for a in &head {
            link.add_segment(&a.to_url_piece());
        }
        self.next.add_to_link(tail, link);
    }
}

impl<A, P, S, Next> HasLink for QueryParam<A, P, S, Next>
where
    A: ToHttpApiData,
    (P, S): ArgShape<A>,
    Next: HasLink,
{
    type LinkArgs = HCons<<(P, S) as ArgShape<A>>::Out, Next::LinkArgs>;
    fn add_to_link(&self, args: Self::LinkArgs, link: &mut Link) {
        let HCons { head, tail } = args;
        if let Some(a) = <(P, S) as ArgShape<A>>::into_value(head) {
            link.add_query(Param::Single(self.name.clone(), a.to_query_param()));
        }
        self.next.add_to_link(tail, link);
    }
}

impl<A, Next> HasLink for QueryParams<A, Next>
where
    A: ToHttpApiData,
    Next: HasLink,
{
    type LinkArgs = HCons<Vec<A>, Next::LinkArgs>;
    fn add_to_link(&self, args: Self::LinkArgs, link: &mut Link) {
        let HCons { head, tail } = args;
        for a in &head {
            link.add_query(Param::ArrayElem(self.name.clone(), a.to_query_param()));
        }
        self.next.add_to_link(tail, link);
    }
}

impl<Next: HasLink> HasLink for QueryFlag<Next> {
    type LinkArgs = HCons<bool, Next::LinkArgs>;
    fn add_to_link(&self, args: Self::LinkArgs, link: &mut Link) {
        let HCons { head, tail } = args;
        if head {
            link.add_query(Param::Flag(self.name.clone()));
        }
        self.next.add_to_link(tail, link);
    }
}

impl<Next: HasLink> HasLink for QueryString<Next> {
    type LinkArgs = HCons<Query, Next::LinkArgs>;
    fn add_to_link(&self, args: Self::LinkArgs, link: &mut Link) {
        let HCons { head, tail } = args;
        link.set_query_string(head);
        self.next.add_to_link(tail, link);
    }
}

impl<A, Next> HasLink for DeepQuery<A, Next>
where
    A: ToDeepQuery,
    Next: HasLink,
{
    type LinkArgs = HCons<A, Next::LinkArgs>;
    fn add_to_link(&self, args: Self::LinkArgs, link: &mut Link) {
        let HCons { head, tail } = args;
        for entry in head.to_deep_query().entries() {
            link.add_query(Param::DeepObject(
                render_deep_query_key(&self.name, entry.path()),
                entry.value().map(str::to_owned),
            ));
        }
        self.next.add_to_link(tail, link);
    }
}

impl<A, Next> HasLink for Fragment<A, Next>
where
    A: ToHttpApiData,
    Next: HasLink,
{
    type LinkArgs = HCons<A, Next::LinkArgs>;
    fn add_to_link(&self, args: Self::LinkArgs, link: &mut Link) {
        let HCons { head, tail } = args;
        link.set_fragment(head.to_query_param());
        self.next.add_to_link(tail, link);
    }
}

// Header and ReqBody do not appear in URLs: they contribute no link args.
macro_rules! link_skip {
    ($ty:ident < $($g:ident),+ >) => {
        impl<$($g),+, Next: HasLink> HasLink for $ty<$($g),+, Next> {
            type LinkArgs = Next::LinkArgs;
            fn add_to_link(&self, args: Self::LinkArgs, link: &mut Link) {
                self.next.add_to_link(args, link);
            }
        }
    };
    ($ty:ident) => {
        impl<Next: HasLink> HasLink for $ty<Next> {
            type LinkArgs = Next::LinkArgs;
            fn add_to_link(&self, args: Self::LinkArgs, link: &mut Link) {
                self.next.add_to_link(args, link);
            }
        }
    };
}
link_skip!(Header<A, P, S>);
link_skip!(Host);
link_skip!(ReqBody<CTypes, A, S>);
link_skip!(StreamBody<Framing, CType, T>);
link_skip!(Description);
link_skip!(Summary);
link_skip!(OperationId);
// Server-only combinators are URL-transparent: they add no link arguments.
link_skip!(Vault);
link_skip!(WithResource<R>);
link_skip!(BasicAuth<Usr>);
link_skip!(AuthProtect<Usr>);
link_skip!(IsSecure);
link_skip!(HttpVersion);
link_skip!(RemoteHost);
link_skip!(WithNamedContext<Name>);

/// A link builder for one endpoint.
pub struct LinkEndpoint<Api> {
    chain: Arc<Api>,
}

impl<Api: HasLink> LinkEndpoint<Api> {
    /// Build a [`Link`] to this endpoint from its path/query arguments.
    pub fn link(&self, args: Api::LinkArgs) -> Link {
        let mut link = Link::new();
        self.chain.add_to_link(args, &mut link);
        link
    }
}

/// Build a link-builder value from an API description: a [`LinkEndpoint`] per
/// endpoint, or a nested tuple mirroring [`Alt`].
pub trait MakeLink {
    /// The resulting link-builder value.
    type Links;
    /// Construct it.
    fn make_link(self) -> Self::Links;
}

impl<Api: HasLink> MakeLink for Api {
    type Links = LinkEndpoint<Api>;
    fn make_link(self) -> Self::Links {
        LinkEndpoint {
            chain: Arc::new(self),
        }
    }
}

impl<L: MakeLink, R: MakeLink> MakeLink for Alt<L, R> {
    type Links = (L::Links, R::Links);
    fn make_link(self) -> Self::Links {
        (self.left.make_link(), self.right.make_link())
    }
}

/// Build type-safe link builders from an API description.
pub fn links<Api: MakeLink>(api: Api) -> Api::Links {
    api.make_link()
}

#[cfg(test)]
mod tests;

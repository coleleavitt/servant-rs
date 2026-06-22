use std::net::SocketAddr;
use std::sync::Arc;

use http::{Extensions, Version};

use super::combinators::*;
use super::sealed;
use crate::hlist::{HCons, HList, HNil};
use crate::modifiers::{ArgShape, CaptureShape, Required};

/// Computes the heterogeneous argument list a handler for this API fragment
/// must accept. Each value-extracting combinator prepends exactly one element
/// (its [`crate::modifiers::ArgShape`]-wrapped type) in left-to-right order;
/// structural and metadata combinators prepend nothing.
///
/// Sealed: only this crate's combinators implement it.
pub trait HasArgs: sealed::Sealed {
    /// The accumulated argument list.
    type Args: HList;
}

impl<M, const STATUS: u16, CTypes, A> HasArgs for Verb<M, STATUS, CTypes, A> {
    type Args = HNil;
}
impl<M> HasArgs for NoContentVerb<M> {
    type Args = HNil;
}
impl<M, const STATUS: u16, CTypes, A> HasArgs for VerbWithHeaders<M, STATUS, CTypes, A> {
    type Args = HNil;
}
impl<M, CTypes, Resp> HasArgs for UVerb<M, CTypes, Resp> {
    type Args = HNil;
}
impl<M, const STATUS: u16, Framing, CType, T> HasArgs for StreamVerb<M, STATUS, Framing, CType, T> {
    type Args = HNil;
}
impl<Next: HasArgs> HasArgs for Path<Next> {
    type Args = Next::Args;
}
impl<A, S, Next> HasArgs for Capture<A, S, Next>
where
    S: CaptureShape<A>,
    Next: HasArgs,
{
    type Args = HCons<<S as CaptureShape<A>>::Out, Next::Args>;
}
impl<A, Next: HasArgs> HasArgs for CaptureAll<A, Next> {
    type Args = HCons<Vec<A>, Next::Args>;
}
impl<A, P, S, Next> HasArgs for QueryParam<A, P, S, Next>
where
    (P, S): ArgShape<A>,
    Next: HasArgs,
{
    type Args = HCons<<(P, S) as ArgShape<A>>::Out, Next::Args>;
}
impl<A, Next: HasArgs> HasArgs for QueryParams<A, Next> {
    type Args = HCons<Vec<A>, Next::Args>;
}
impl<Next: HasArgs> HasArgs for QueryFlag<Next> {
    type Args = HCons<bool, Next::Args>;
}
impl<Next: HasArgs> HasArgs for QueryString<Next> {
    type Args = HCons<crate::query::Query, Next::Args>;
}
impl<A, Next: HasArgs> HasArgs for DeepQuery<A, Next> {
    type Args = HCons<A, Next::Args>;
}
impl<A, P, S, Next> HasArgs for Header<A, P, S, Next>
where
    (P, S): ArgShape<A>,
    Next: HasArgs,
{
    type Args = HCons<<(P, S) as ArgShape<A>>::Out, Next::Args>;
}
impl<Next: HasArgs> HasArgs for Host<Next> {
    type Args = Next::Args;
}
impl<CTypes, A, S, Next> HasArgs for ReqBody<CTypes, A, S, Next>
where
    (Required, S): ArgShape<A>,
    Next: HasArgs,
{
    type Args = HCons<<(Required, S) as ArgShape<A>>::Out, Next::Args>;
}
impl<Next: HasArgs> HasArgs for Description<Next> {
    type Args = Next::Args;
}
impl<Next: HasArgs> HasArgs for Summary<Next> {
    type Args = Next::Args;
}
impl<Next: HasArgs> HasArgs for OperationId<Next> {
    type Args = Next::Args;
}
impl<A, Next: HasArgs> HasArgs for Fragment<A, Next> {
    type Args = Next::Args;
}
impl<Next: HasArgs> HasArgs for Vault<Next> {
    type Args = HCons<Arc<Extensions>, Next::Args>;
}
impl<R, Next: HasArgs> HasArgs for WithResource<R, Next> {
    type Args = HCons<R, Next::Args>;
}
impl<Usr, Next: HasArgs> HasArgs for BasicAuth<Usr, Next> {
    type Args = HCons<Usr, Next::Args>;
}
impl<Usr, Next: HasArgs> HasArgs for AuthProtect<Usr, Next> {
    type Args = HCons<Usr, Next::Args>;
}
impl<Next: HasArgs> HasArgs for IsSecure<Next> {
    type Args = HCons<bool, Next::Args>;
}
impl<Next: HasArgs> HasArgs for HttpVersion<Next> {
    type Args = HCons<Version, Next::Args>;
}
impl<Next: HasArgs> HasArgs for RemoteHost<Next> {
    type Args = HCons<Option<SocketAddr>, Next::Args>;
}
impl<Name, Next: HasArgs> HasArgs for WithNamedContext<Name, Next> {
    type Args = Next::Args;
}

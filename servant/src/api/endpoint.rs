use http::{Method, StatusCode};

use super::args::HasArgs;
use super::combinators::*;
use crate::content::NoContent;
use crate::method::MethodMarker;

/// A single endpoint chain (path/extractors ending in a verb). Exposes the
/// response value type, the response content-type list, and the runtime
/// method/status. (Not implemented for [`Alt`], which is a *set* of endpoints.)
pub trait Endpoint: HasArgs {
    /// The response value the handler returns.
    type Output;
    /// The response content-type list used for `Accept` negotiation.
    type ResponseTypes;
    /// The HTTP method.
    fn method(&self) -> Method;
    /// The HTTP success status.
    fn status(&self) -> StatusCode;
}

impl<M: MethodMarker, const STATUS: u16, CTypes, A> Endpoint for Verb<M, STATUS, CTypes, A> {
    type Output = A;
    type ResponseTypes = CTypes;
    fn method(&self) -> Method {
        M::method()
    }
    fn status(&self) -> StatusCode {
        StatusCode::from_u16(STATUS).expect("Verb STATUS must be a valid HTTP status code")
    }
}

impl<M: MethodMarker, const STATUS: u16, CTypes, A> Endpoint
    for VerbWithHeaders<M, STATUS, CTypes, A>
{
    type Output = crate::response::Headers<A>;
    type ResponseTypes = CTypes;
    fn method(&self) -> Method {
        M::method()
    }
    fn status(&self) -> StatusCode {
        StatusCode::from_u16(STATUS)
            .expect("VerbWithHeaders STATUS must be a valid HTTP status code")
    }
}

impl<M: MethodMarker, const STATUS: u16, Framing, CType, T> Endpoint
    for StreamVerb<M, STATUS, Framing, CType, T>
{
    type Output = crate::stream::SourceStream<T>;
    type ResponseTypes = (CType,);
    fn method(&self) -> Method {
        M::method()
    }
    fn status(&self) -> StatusCode {
        StatusCode::from_u16(STATUS).expect("StreamVerb STATUS must be a valid HTTP status code")
    }
}

impl<M: MethodMarker, CTypes, Resp> Endpoint for UVerb<M, CTypes, Resp> {
    type Output = Resp;
    type ResponseTypes = CTypes;
    fn method(&self) -> Method {
        M::method()
    }
    fn status(&self) -> StatusCode {
        // A union has no single status; the active arm supplies it at render
        // time. 200 is the nominal status used for docs/layout.
        StatusCode::OK
    }
}

impl<M: MethodMarker> Endpoint for NoContentVerb<M> {
    type Output = NoContent;
    /// No content negotiation occurs for a 204 response.
    type ResponseTypes = ();
    fn method(&self) -> Method {
        M::method()
    }
    fn status(&self) -> StatusCode {
        StatusCode::NO_CONTENT
    }
}

macro_rules! forward_endpoint {
    ($ty:ident < $($g:ident),+ >) => {
        impl<$($g),+, Next> Endpoint for $ty<$($g),+, Next>
        where
            Self: HasArgs,
            Next: Endpoint,
        {
            type Output = Next::Output;
            type ResponseTypes = Next::ResponseTypes;
            fn method(&self) -> Method { self.next.method() }
            fn status(&self) -> StatusCode { self.next.status() }
        }
    };
}

impl<Next> Endpoint for Path<Next>
where
    Self: HasArgs,
    Next: Endpoint,
{
    type Output = Next::Output;
    type ResponseTypes = Next::ResponseTypes;
    fn method(&self) -> Method {
        self.next.method()
    }
    fn status(&self) -> StatusCode {
        self.next.status()
    }
}
forward_endpoint!(Capture<A, S>);
forward_endpoint!(CaptureAll<A>);
forward_endpoint!(QueryParam<A, P, S>);
forward_endpoint!(QueryParams<A>);
forward_endpoint!(DeepQuery<A>);
forward_endpoint!(Header<A, P, S>);
forward_endpoint!(ReqBody<CTypes, A, S>);
forward_endpoint!(StreamBody<Framing, CType, T>);
forward_endpoint!(Fragment<A>);

// Combinators whose only type parameter is `Next` (the macro requires ≥1 extra
// generic, so these are written out).
macro_rules! forward_endpoint_unary {
    ($ty:ident) => {
        impl<Next> Endpoint for $ty<Next>
        where
            Self: HasArgs,
            Next: Endpoint,
        {
            type Output = Next::Output;
            type ResponseTypes = Next::ResponseTypes;
            fn method(&self) -> Method {
                self.next.method()
            }
            fn status(&self) -> StatusCode {
                self.next.status()
            }
        }
    };
}
forward_endpoint_unary!(QueryFlag);
forward_endpoint_unary!(QueryString);
forward_endpoint_unary!(Host);
forward_endpoint_unary!(Description);
forward_endpoint_unary!(Summary);
forward_endpoint_unary!(OperationId);
forward_endpoint_unary!(Vault);
forward_endpoint!(WithResource<R>);
forward_endpoint!(BasicAuth<Usr>);
forward_endpoint!(AuthProtect<Usr>);
forward_endpoint_unary!(IsSecure);
forward_endpoint_unary!(HttpVersion);
forward_endpoint_unary!(RemoteHost);
forward_endpoint!(WithNamedContext<Name>);

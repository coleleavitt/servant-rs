//! The [`HasDocs`] walk: a documentation interpretation of the API description.
//!
//! This is the docs analogue of `servant-server`'s `ServerChain`/`RouterShape`
//! and `servant-client`'s client walk: a trait implemented per combinator that
//! recurses through each combinator's `next` field down to the terminal verb,
//! reading the *same* API description value the server routes on.
//!
//! Each combinator threads an accumulating [`EndpointDoc`] left-to-right (exactly
//! Servant's `docsFor :: (Endpoint, Action) -> API` accumulator):
//!
//! - [`Path`] pushes a static segment;
//! - [`Capture`]/[`CaptureAll`] push a capture (name + `type_name::<A>()`);
//! - [`QueryParam`]/[`QueryParams`]/[`QueryFlag`] append a [`ParamDoc`];
//! - [`Header`] appends a header name;
//! - [`ReqBody`] sets the request body (content types via `AllMime`);
//! - [`Description`]/[`Summary`] set the note;
//! - [`Verb`]/[`NoContentVerb`] finalize the endpoint and emit a one-endpoint
//!   [`ApiDoc`];
//! - [`Alt`] runs both branches on the *same* inherited accumulator and
//!   concatenates the results left-biased (`left` then `right`), mirroring
//!   Servant's `docsFor a <> docsFor b`.
//!
//! **[diff]** Servant samples example request/response bodies via a `ToSample`
//! class. We record the body/response *types* (content types + Rust type name)
//! but do not synthesize example payloads; sampling can be layered on later
//! without changing this walk.

use servant::api::{
    Alt,
    Capture,
    CaptureAll,
    Description,
    EmptyApi,
    Endpoint,
    Fragment,
    Header,
    NoContentVerb,
    OperationId,
    Path,
    QueryFlag,
    QueryParam,
    QueryParams,
    ReqBody,
    Summary,
    Verb,
};
use servant::content::AllMime;
use servant::method::MethodMarker;

use crate::model::{ApiDoc, BodyDoc, EndpointDoc, FragmentDoc, ParamDoc, ParamKind, PathPart};

mod server;

/// A documentation interpretation of an API description.
///
/// Implemented for every combinator and for [`Alt`]/[`EmptyApi`]. Call
/// [`HasDocs::docs`] to produce the full [`ApiDoc`]; the per-combinator recursion
/// lives in [`HasDocs::docs_walk`], which threads the accumulating
/// [`EndpointDoc`].
pub trait HasDocs {
    /// Produce the documentation for this API fragment, given the accumulator
    /// inherited from the combinators to the left.
    fn docs_walk(&self, acc: EndpointDoc) -> ApiDoc;

    /// Produce the full documentation model for this API, starting from a fresh
    /// accumulator (`GET /`, status `200`).
    fn docs(&self) -> ApiDoc {
        self.docs_walk(EndpointDoc::empty())
    }
}

// --- Alternatives -----------------------------------------------------------

impl<L: HasDocs, R: HasDocs> HasDocs for Alt<L, R> {
    fn docs_walk(&self, acc: EndpointDoc) -> ApiDoc {
        // Both branches inherit the SAME accumulator (Servant clones it);
        // left is documented first, right appended (left-biased on collisions).
        let mut out = self.left.docs_walk(acc.clone());
        out.extend(self.right.docs_walk(acc));
        out
    }
}

impl HasDocs for EmptyApi {
    fn docs_walk(&self, _acc: EndpointDoc) -> ApiDoc {
        ApiDoc::empty()
    }
}

// --- Terminal verbs ---------------------------------------------------------

impl<M, const STATUS: u16, CTypes, A> HasDocs for Verb<M, STATUS, CTypes, A>
where
    M: MethodMarker,
    CTypes: AllMime,
    Self: Endpoint,
{
    fn docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.method = self.method();
        acc.status = self.status();
        acc.response_types = CTypes::all_media_types();
        ApiDoc::single(acc)
    }
}

impl<M, const STATUS: u16, CTypes, A> HasDocs
    for servant::api::VerbWithHeaders<M, STATUS, CTypes, A>
where
    M: MethodMarker,
    CTypes: AllMime,
    Self: Endpoint,
{
    fn docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.method = self.method();
        acc.status = self.status();
        acc.response_types = CTypes::all_media_types();
        ApiDoc::single(acc)
    }
}

impl<M, CTypes, Resp> HasDocs for servant::api::UVerb<M, CTypes, Resp>
where
    M: MethodMarker,
    CTypes: AllMime,
    Self: Endpoint,
{
    fn docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.method = self.method();
        acc.status = self.status(); // nominal 200; arms carry their own statuses
        acc.response_types = CTypes::all_media_types();
        acc.description = Some(match acc.description {
            Some(d) => format!("{d}\n\nReturns one of several status codes (union response)."),
            None => "Returns one of several status codes (union response).".to_string(),
        });
        ApiDoc::single(acc)
    }
}

impl<M, const STATUS: u16, Framing, CType, T> HasDocs
    for servant::api::StreamVerb<M, STATUS, Framing, CType, T>
where
    M: MethodMarker,
    (CType,): AllMime,
    Self: Endpoint,
{
    fn docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.method = self.method();
        acc.status = self.status();
        acc.response_types = <(CType,) as AllMime>::all_media_types();
        acc.description = Some(match acc.description {
            Some(d) => format!("{d}\n\nStreaming response."),
            None => "Streaming response.".to_string(),
        });
        ApiDoc::single(acc)
    }
}

impl<M> HasDocs for NoContentVerb<M>
where
    M: MethodMarker,
    Self: Endpoint,
{
    fn docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.method = self.method();
        acc.status = self.status();
        // [diff] NoContentVerb's ResponseTypes is `()` (no content negotiation),
        // so there are no response media types to record.
        acc.response_types = Vec::new();
        ApiDoc::single(acc)
    }
}

// --- Path & captures --------------------------------------------------------

impl<Next: HasDocs> HasDocs for Path<Next> {
    fn docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.path.push(PathPart::Static(self.segment.clone()));
        self.next.docs_walk(acc)
    }
}

impl<A, S, Next: HasDocs> HasDocs for Capture<A, S, Next> {
    fn docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.path.push(PathPart::Capture {
            name: self.name.clone(),
            type_name: std::any::type_name::<A>(),
        });
        self.next.docs_walk(acc)
    }
}

impl<A, Next: HasDocs> HasDocs for CaptureAll<A, Next> {
    fn docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.path.push(PathPart::CaptureAll {
            name: self.name.clone(),
            type_name: std::any::type_name::<A>(),
        });
        self.next.docs_walk(acc)
    }
}

// --- Query parameters -------------------------------------------------------

impl<A, P, S, Next: HasDocs> HasDocs for QueryParam<A, P, S, Next> {
    fn docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.query_params.push(ParamDoc {
            name: self.name.clone(),
            kind: ParamKind::Normal,
            type_name: std::any::type_name::<A>(),
        });
        self.next.docs_walk(acc)
    }
}

impl<A, Next: HasDocs> HasDocs for QueryParams<A, Next> {
    fn docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.query_params.push(ParamDoc {
            name: self.name.clone(),
            kind: ParamKind::List,
            type_name: std::any::type_name::<A>(),
        });
        self.next.docs_walk(acc)
    }
}

impl<Next: HasDocs> HasDocs for QueryFlag<Next> {
    fn docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.query_params.push(ParamDoc {
            name: self.name.clone(),
            kind: ParamKind::Flag,
            type_name: "",
        });
        self.next.docs_walk(acc)
    }
}

// --- Headers ----------------------------------------------------------------

impl<A, P, S, Next: HasDocs> HasDocs for Header<A, P, S, Next> {
    fn docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.headers.push(self.name.clone());
        self.next.docs_walk(acc)
    }
}

// --- Request body -----------------------------------------------------------

impl<CTypes, A, S, Next: HasDocs> HasDocs for ReqBody<CTypes, A, S, Next>
where
    CTypes: AllMime,
{
    fn docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.request_body = Some(BodyDoc {
            content_types: CTypes::all_media_types(),
            type_name: std::any::type_name::<A>(),
        });
        self.next.docs_walk(acc)
    }
}

// --- Metadata ---------------------------------------------------------------

impl<Next: HasDocs> HasDocs for Description<Next> {
    fn docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.description = Some(self.text.clone());
        self.next.docs_walk(acc)
    }
}

impl<Next: HasDocs> HasDocs for Summary<Next> {
    fn docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.summary = Some(self.text.clone());
        self.next.docs_walk(acc)
    }
}

impl<Next: HasDocs> HasDocs for OperationId<Next> {
    fn docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.operation_id = Some(self.id.clone());
        self.next.docs_walk(acc)
    }
}

impl<A, Next: HasDocs> HasDocs for Fragment<A, Next> {
    fn docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.fragment = Some(FragmentDoc {
            type_name: std::any::type_name::<A>(),
            description: self.description.clone(),
        });
        self.next.docs_walk(acc)
    }
}

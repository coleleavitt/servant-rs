use servant::api::{Endpoint, NoContentVerb, Raw, RawM, StreamVerb, UVerb, Verb, VerbWithHeaders};
use servant::content::AllMime;
use servant::method::MethodMarker;
use servant_docs::{ApiDoc, EndpointDoc, RawDoc, SchemaDoc, ToSchema};

use super::HasOpenApi;

impl<M, const STATUS: u16, CTypes, A> HasOpenApi for Verb<M, STATUS, CTypes, A>
where
    M: MethodMarker,
    CTypes: AllMime,
    A: ToSchema,
    Self: Endpoint,
{
    fn openapi_docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.method = self.method();
        acc.status = self.status();
        acc.response_types = CTypes::all_media_types();
        acc.response_schema = Some(SchemaDoc::for_type::<A>());
        ApiDoc::single(acc)
    }
}

impl<M, const STATUS: u16, CTypes, A> HasOpenApi for VerbWithHeaders<M, STATUS, CTypes, A>
where
    M: MethodMarker,
    CTypes: AllMime,
    A: ToSchema,
    Self: Endpoint,
{
    fn openapi_docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.method = self.method();
        acc.status = self.status();
        acc.response_types = CTypes::all_media_types();
        acc.response_schema = Some(SchemaDoc::for_type::<A>());
        ApiDoc::single(acc)
    }
}

impl<M, CTypes, Resp> HasOpenApi for UVerb<M, CTypes, Resp>
where
    M: MethodMarker,
    CTypes: AllMime,
    Resp: ToSchema,
    Self: Endpoint,
{
    fn openapi_docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.method = self.method();
        acc.status = self.status();
        acc.response_types = CTypes::all_media_types();
        acc.response_schema = Some(SchemaDoc::for_type::<Resp>());
        acc.description = Some(match acc.description {
            Some(d) => format!("{d}\n\nReturns one of several status codes (union response)."),
            None => "Returns one of several status codes (union response).".to_string(),
        });
        ApiDoc::single(acc)
    }
}

impl<M, const STATUS: u16, Framing, CType, T> HasOpenApi
    for StreamVerb<M, STATUS, Framing, CType, T>
where
    M: MethodMarker,
    (CType,): AllMime,
    T: ToSchema,
    Self: Endpoint,
{
    fn openapi_docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.method = self.method();
        acc.status = self.status();
        acc.response_types = <(CType,) as AllMime>::all_media_types();
        acc.response_schema = Some(SchemaDoc::for_type::<T>());
        acc.description = Some(match acc.description {
            Some(d) => format!("{d}\n\nStreaming response."),
            None => "Streaming response.".to_string(),
        });
        ApiDoc::single(acc)
    }
}

impl<M> HasOpenApi for NoContentVerb<M>
where
    M: MethodMarker,
    Self: Endpoint,
{
    fn openapi_docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.method = self.method();
        acc.status = self.status();
        acc.response_types = Vec::new();
        acc.response_schema = None;
        ApiDoc::single(acc)
    }
}

impl HasOpenApi for Raw {
    fn openapi_docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.raw = Some(RawDoc::Raw);
        ApiDoc::single(acc)
    }
}

impl HasOpenApi for RawM {
    fn openapi_docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.raw = Some(RawDoc::RawM);
        ApiDoc::single(acc)
    }
}

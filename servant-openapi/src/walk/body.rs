use servant::api::{ReqBody, StreamBody};
use servant::content::AllMime;
use servant_docs::{ApiDoc, BodyDoc, EndpointDoc, SchemaDoc, ToSchema};

use super::HasOpenApi;

impl<CTypes, A, S, Next: HasOpenApi> HasOpenApi for ReqBody<CTypes, A, S, Next>
where
    CTypes: AllMime,
    A: ToSchema,
{
    fn openapi_docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.request_body = Some(BodyDoc {
            content_types: CTypes::all_media_types(),
            type_name: std::any::type_name::<A>(),
            schema: SchemaDoc::for_type::<A>(),
            streaming: false,
        });
        self.next.openapi_docs_walk(acc)
    }
}

impl<Framing, CType, T, Next: HasOpenApi> HasOpenApi for StreamBody<Framing, CType, T, Next>
where
    (CType,): AllMime,
    T: ToSchema,
{
    fn openapi_docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.request_body = Some(BodyDoc {
            content_types: <(CType,) as AllMime>::all_media_types(),
            type_name: std::any::type_name::<T>(),
            schema: SchemaDoc::for_type::<T>(),
            streaming: true,
        });
        self.next.openapi_docs_walk(acc)
    }
}

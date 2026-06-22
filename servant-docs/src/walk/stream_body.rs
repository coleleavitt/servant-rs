use servant::api::StreamBody;
use servant::content::AllMime;

use crate::model::{ApiDoc, BodyDoc, EndpointDoc};
use crate::schema::SchemaDoc;
use crate::walk::HasDocs;

impl<Framing, CType, T, Next: HasDocs> HasDocs for StreamBody<Framing, CType, T, Next>
where
    (CType,): AllMime,
{
    fn docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.request_body = Some(BodyDoc {
            content_types: <(CType,) as AllMime>::all_media_types(),
            type_name: std::any::type_name::<T>(),
            schema: SchemaDoc::type_name(std::any::type_name::<T>()),
            streaming: true,
        });
        self.next.docs_walk(acc)
    }
}

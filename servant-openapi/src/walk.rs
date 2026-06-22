//! OpenAPI-specific docs-model walk that records structural schemas.

mod body;
mod metadata;
mod route;
mod terminals;

use servant::api::{Alt, EmptyApi};
use servant_docs::{ApiDoc, EndpointDoc};

/// OpenAPI interpretation of a servant-rs API description.
pub trait HasOpenApi {
    /// Continue walking with an inherited endpoint accumulator.
    fn openapi_docs_walk(&self, acc: EndpointDoc) -> ApiDoc;

    /// Reflect this API into the shared docs model with schema metadata.
    fn openapi_docs(&self) -> ApiDoc {
        self.openapi_docs_walk(EndpointDoc::empty())
    }
}

impl<L: HasOpenApi, R: HasOpenApi> HasOpenApi for Alt<L, R> {
    fn openapi_docs_walk(&self, acc: EndpointDoc) -> ApiDoc {
        let mut out = self.left.openapi_docs_walk(acc.clone());
        out.extend(self.right.openapi_docs_walk(acc));
        out
    }
}

impl HasOpenApi for EmptyApi {
    fn openapi_docs_walk(&self, _acc: EndpointDoc) -> ApiDoc {
        ApiDoc::empty()
    }
}

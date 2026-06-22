use servant::api::{
    Capture,
    CaptureAll,
    DeepQuery,
    Header,
    Host,
    Path,
    QueryFlag,
    QueryParam,
    QueryParams,
    QueryString,
};
use servant_docs::{
    ApiDoc,
    DeepQueryDoc,
    EndpointDoc,
    HostDoc,
    ParamDoc,
    ParamKind,
    PathPart,
    QueryStringDoc,
};

use super::HasOpenApi;

impl<Next: HasOpenApi> HasOpenApi for Path<Next> {
    fn openapi_docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.path.push(PathPart::Static(self.segment.clone()));
        self.next.openapi_docs_walk(acc)
    }
}

impl<A, S, Next: HasOpenApi> HasOpenApi for Capture<A, S, Next> {
    fn openapi_docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.path.push(PathPart::Capture {
            name: self.name.clone(),
            type_name: std::any::type_name::<A>(),
        });
        self.next.openapi_docs_walk(acc)
    }
}

impl<A, Next: HasOpenApi> HasOpenApi for CaptureAll<A, Next> {
    fn openapi_docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.path.push(PathPart::CaptureAll {
            name: self.name.clone(),
            type_name: std::any::type_name::<A>(),
        });
        self.next.openapi_docs_walk(acc)
    }
}

impl<A, P, S, Next: HasOpenApi> HasOpenApi for QueryParam<A, P, S, Next> {
    fn openapi_docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.query_params.push(ParamDoc {
            name: self.name.clone(),
            kind: ParamKind::Normal,
            type_name: std::any::type_name::<A>(),
        });
        self.next.openapi_docs_walk(acc)
    }
}

impl<A, Next: HasOpenApi> HasOpenApi for QueryParams<A, Next> {
    fn openapi_docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.query_params.push(ParamDoc {
            name: self.name.clone(),
            kind: ParamKind::List,
            type_name: std::any::type_name::<A>(),
        });
        self.next.openapi_docs_walk(acc)
    }
}

impl<Next: HasOpenApi> HasOpenApi for QueryFlag<Next> {
    fn openapi_docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.query_params.push(ParamDoc {
            name: self.name.clone(),
            kind: ParamKind::Flag,
            type_name: "",
        });
        self.next.openapi_docs_walk(acc)
    }
}

impl<Next: HasOpenApi> HasOpenApi for QueryString<Next> {
    fn openapi_docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.query_string = Some(QueryStringDoc {
            decoded_ordered_pairs: true,
            raw_query_available: true,
        });
        self.next.openapi_docs_walk(acc)
    }
}

impl<A, Next: HasOpenApi> HasOpenApi for DeepQuery<A, Next> {
    fn openapi_docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.deep_queries.push(DeepQueryDoc {
            name: self.name.clone(),
            type_name: std::any::type_name::<A>(),
        });
        self.next.openapi_docs_walk(acc)
    }
}

impl<A, P, S, Next: HasOpenApi> HasOpenApi for Header<A, P, S, Next> {
    fn openapi_docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.headers.push(self.name.clone());
        self.next.openapi_docs_walk(acc)
    }
}

impl<Next: HasOpenApi> HasOpenApi for Host<Next> {
    fn openapi_docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.host = Some(HostDoc::from_name(&self.name));
        self.next.openapi_docs_walk(acc)
    }
}

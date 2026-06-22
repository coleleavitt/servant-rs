use servant::api::{
    AuthProtect,
    BasicAuth,
    Description,
    Fragment,
    HttpVersion,
    IsSecure,
    OperationId,
    RemoteHost,
    Summary,
    Vault,
    WithNamedContext,
    WithResource,
};
use servant_docs::{ApiDoc, EndpointDoc, FragmentDoc};

use super::HasOpenApi;

impl<Next: HasOpenApi> HasOpenApi for Description<Next> {
    fn openapi_docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.description = Some(self.text.clone());
        self.next.openapi_docs_walk(acc)
    }
}

impl<Next: HasOpenApi> HasOpenApi for Summary<Next> {
    fn openapi_docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.summary = Some(self.text.clone());
        self.next.openapi_docs_walk(acc)
    }
}

impl<Next: HasOpenApi> HasOpenApi for OperationId<Next> {
    fn openapi_docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.operation_id = Some(self.id.clone());
        self.next.openapi_docs_walk(acc)
    }
}

impl<A, Next: HasOpenApi> HasOpenApi for Fragment<A, Next> {
    fn openapi_docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.fragment = Some(FragmentDoc {
            type_name: std::any::type_name::<A>(),
            description: self.description.clone(),
        });
        self.next.openapi_docs_walk(acc)
    }
}

impl<Next: HasOpenApi> HasOpenApi for Vault<Next> {
    fn openapi_docs_walk(&self, acc: EndpointDoc) -> ApiDoc {
        self.next.openapi_docs_walk(acc)
    }
}

impl<R, Next: HasOpenApi> HasOpenApi for WithResource<R, Next> {
    fn openapi_docs_walk(&self, acc: EndpointDoc) -> ApiDoc {
        self.next.openapi_docs_walk(acc)
    }
}

impl<Usr, Next: HasOpenApi> HasOpenApi for BasicAuth<Usr, Next> {
    fn openapi_docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        let note = format!(
            "Requires HTTP Basic authentication (realm `{}`).",
            self.realm
        );
        acc.description = Some(match acc.description {
            Some(d) => format!("{d}\n\n{note}"),
            None => note,
        });
        self.next.openapi_docs_walk(acc)
    }
}

impl<Usr, Next: HasOpenApi> HasOpenApi for AuthProtect<Usr, Next> {
    fn openapi_docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        let note = "Requires authentication.".to_string();
        acc.description = Some(match acc.description {
            Some(d) => format!("{d}\n\n{note}"),
            None => note,
        });
        self.next.openapi_docs_walk(acc)
    }
}

impl<Next: HasOpenApi> HasOpenApi for IsSecure<Next> {
    fn openapi_docs_walk(&self, acc: EndpointDoc) -> ApiDoc {
        self.next.openapi_docs_walk(acc)
    }
}

impl<Next: HasOpenApi> HasOpenApi for HttpVersion<Next> {
    fn openapi_docs_walk(&self, acc: EndpointDoc) -> ApiDoc {
        self.next.openapi_docs_walk(acc)
    }
}

impl<Next: HasOpenApi> HasOpenApi for RemoteHost<Next> {
    fn openapi_docs_walk(&self, acc: EndpointDoc) -> ApiDoc {
        self.next.openapi_docs_walk(acc)
    }
}

impl<Name, Next: HasOpenApi> HasOpenApi for WithNamedContext<Name, Next> {
    fn openapi_docs_walk(&self, acc: EndpointDoc) -> ApiDoc {
        self.next.openapi_docs_walk(acc)
    }
}

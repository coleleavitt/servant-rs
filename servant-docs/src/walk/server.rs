use super::HasDocs;
use crate::model::{ApiDoc, EndpointDoc};

impl<Next: HasDocs> HasDocs for servant::api::Vault<Next> {
    fn docs_walk(&self, acc: EndpointDoc) -> ApiDoc {
        self.next.docs_walk(acc)
    }
}

impl<R, Next: HasDocs> HasDocs for servant::api::WithResource<R, Next> {
    fn docs_walk(&self, acc: EndpointDoc) -> ApiDoc {
        self.next.docs_walk(acc)
    }
}

impl<Usr, Next: HasDocs> HasDocs for servant::api::BasicAuth<Usr, Next> {
    fn docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        let note = format!(
            "Requires HTTP Basic authentication (realm `{}`).",
            self.realm
        );
        acc.description = Some(match acc.description {
            Some(d) => format!("{d}\n\n{note}"),
            None => note,
        });
        self.next.docs_walk(acc)
    }
}

impl<Usr, Next: HasDocs> HasDocs for servant::api::AuthProtect<Usr, Next> {
    fn docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        let note = "Requires authentication.".to_string();
        acc.description = Some(match acc.description {
            Some(d) => format!("{d}\n\n{note}"),
            None => note,
        });
        self.next.docs_walk(acc)
    }
}

impl<Next: HasDocs> HasDocs for servant::api::IsSecure<Next> {
    fn docs_walk(&self, acc: EndpointDoc) -> ApiDoc {
        self.next.docs_walk(acc)
    }
}

impl<Next: HasDocs> HasDocs for servant::api::HttpVersion<Next> {
    fn docs_walk(&self, acc: EndpointDoc) -> ApiDoc {
        self.next.docs_walk(acc)
    }
}

impl<Next: HasDocs> HasDocs for servant::api::RemoteHost<Next> {
    fn docs_walk(&self, acc: EndpointDoc) -> ApiDoc {
        self.next.docs_walk(acc)
    }
}

impl<Name, Next: HasDocs> HasDocs for servant::api::WithNamedContext<Name, Next> {
    fn docs_walk(&self, acc: EndpointDoc) -> ApiDoc {
        self.next.docs_walk(acc)
    }
}

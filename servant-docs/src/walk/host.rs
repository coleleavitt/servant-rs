use servant::api::Host;

use super::HasDocs;
use crate::model::{ApiDoc, EndpointDoc, HostDoc};

impl<Next: HasDocs> HasDocs for Host<Next> {
    fn docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.host = Some(HostDoc::from_name(&self.name));
        self.next.docs_walk(acc)
    }
}

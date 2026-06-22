use servant::api::{Raw, RawM};

use super::HasDocs;
use crate::model::{ApiDoc, EndpointDoc, RawDoc};

impl HasDocs for Raw {
    fn docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.raw = Some(RawDoc::Raw);
        ApiDoc::single(acc)
    }
}

impl HasDocs for RawM {
    fn docs_walk(&self, mut acc: EndpointDoc) -> ApiDoc {
        acc.raw = Some(RawDoc::RawM);
        ApiDoc::single(acc)
    }
}

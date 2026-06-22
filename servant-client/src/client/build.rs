use std::str::FromStr;

use http::{HeaderName, HeaderValue};
use servant::api::{
    Capture,
    CaptureAll,
    DeepQuery,
    Description,
    Endpoint,
    Fragment,
    Header,
    OperationId,
    Path,
    QueryFlag,
    QueryParam,
    QueryParams,
    QueryString,
    ReqBody,
    Summary,
};
use servant::content::{AllMime, AllMimeRender};
use servant::hlist::HCons;
use servant::http_data::ToHttpApiData;
use servant::modifiers::{ArgShape, CaptureShape, Required};
use servant::query::{Query, ToDeepQuery};

use super::endpoint::HasClient;
use crate::request::{ClientError, ClientRequest, ClientResponse};

// --- forwarding helpers ---

macro_rules! forward_decode {
    () => {
        fn decode(&self, resp: ClientResponse) -> Result<Self::Output, ClientError> {
            self.next.decode(resp)
        }
    };
}

mod host;

impl<Next: HasClient> HasClient for Path<Next>
where
    Self: Endpoint<Output = Next::Output, Args = Next::Args>,
{
    fn build_request(&self, args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        req.append_path(&self.segment);
        self.next.build_request(args, req)
    }
    forward_decode!();
}

macro_rules! metadata_client {
    ($ty:ident < $($g:ident),+ >) => {
        impl<$($g),+, Next: HasClient> HasClient for $ty<$($g),+, Next>
        where
            Self: Endpoint<Output = Next::Output, Args = Next::Args>,
        {
            fn build_request(
                &self,
                args: Self::Args,
                req: &mut ClientRequest,
            ) -> Result<(), String> {
                self.next.build_request(args, req)
            }
            forward_decode!();
        }
    };
    ($ty:ident) => {
        impl<Next: HasClient> HasClient for $ty<Next>
        where
            Self: Endpoint<Output = Next::Output, Args = Next::Args>,
        {
            fn build_request(
                &self,
                args: Self::Args,
                req: &mut ClientRequest,
            ) -> Result<(), String> {
                self.next.build_request(args, req)
            }
            forward_decode!();
        }
    };
}
metadata_client!(Description);
metadata_client!(Summary);
metadata_client!(OperationId);
metadata_client!(Fragment<A>);

impl<A, S, Next> HasClient for Capture<A, S, Next>
where
    A: ToHttpApiData,
    S: CaptureShape<A>,
    Next: HasClient,
    Self: Endpoint<Args = HCons<<S as CaptureShape<A>>::Out, Next::Args>, Output = Next::Output>,
{
    fn build_request(&self, args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        let HCons { head, tail } = args;
        match <S as CaptureShape<A>>::into_value(head) {
            Some(a) => req.append_path(&a.to_url_piece()),
            None => req.append_path(""),
        }
        self.next.build_request(tail, req)
    }
    forward_decode!();
}

impl<A, Next> HasClient for CaptureAll<A, Next>
where
    A: ToHttpApiData,
    Next: HasClient,
    Self: Endpoint<Args = HCons<Vec<A>, Next::Args>, Output = Next::Output>,
{
    fn build_request(&self, args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        let HCons { head, tail } = args;
        for a in &head {
            req.append_path(&a.to_url_piece());
        }
        self.next.build_request(tail, req)
    }
    forward_decode!();
}

impl<A, P, S, Next> HasClient for QueryParam<A, P, S, Next>
where
    A: ToHttpApiData,
    (P, S): ArgShape<A>,
    Next: HasClient,
    Self: Endpoint<Args = HCons<<(P, S) as ArgShape<A>>::Out, Next::Args>, Output = Next::Output>,
{
    fn build_request(&self, args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        let HCons { head, tail } = args;
        if let Some(a) = <(P, S) as ArgShape<A>>::into_value(head) {
            req.append_query(&self.name, Some(a.to_query_param()));
        }
        self.next.build_request(tail, req)
    }
    forward_decode!();
}

impl<A, Next> HasClient for QueryParams<A, Next>
where
    A: ToHttpApiData,
    Next: HasClient,
    Self: Endpoint<Args = HCons<Vec<A>, Next::Args>, Output = Next::Output>,
{
    fn build_request(&self, args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        let HCons { head, tail } = args;
        for a in &head {
            req.append_query(&self.name, Some(a.to_query_param()));
        }
        self.next.build_request(tail, req)
    }
    forward_decode!();
}

impl<Next> HasClient for QueryFlag<Next>
where
    Next: HasClient,
    Self: Endpoint<Args = HCons<bool, Next::Args>, Output = Next::Output>,
{
    fn build_request(&self, args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        let HCons { head, tail } = args;
        if head {
            req.append_query(&self.name, None);
        }
        self.next.build_request(tail, req)
    }
    forward_decode!();
}

impl<Next> HasClient for QueryString<Next>
where
    Next: HasClient,
    Self: Endpoint<Args = HCons<Query, Next::Args>, Output = Next::Output>,
{
    fn build_request(&self, args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        let HCons { head, tail } = args;
        req.set_query_string(head);
        self.next.build_request(tail, req)
    }
    forward_decode!();
}

impl<A, Next> HasClient for DeepQuery<A, Next>
where
    A: ToDeepQuery,
    Next: HasClient,
    Self: Endpoint<Args = HCons<A, Next::Args>, Output = Next::Output>,
{
    fn build_request(&self, args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        let HCons { head, tail } = args;
        for entry in head.to_deep_query().entries() {
            req.append_deep_query(&self.name, entry.path(), entry.value().map(str::to_owned));
        }
        self.next.build_request(tail, req)
    }
    forward_decode!();
}

impl<A, P, S, Next> HasClient for Header<A, P, S, Next>
where
    A: ToHttpApiData,
    (P, S): ArgShape<A>,
    Next: HasClient,
    Self: Endpoint<Args = HCons<<(P, S) as ArgShape<A>>::Out, Next::Args>, Output = Next::Output>,
{
    fn build_request(&self, args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        let HCons { head, tail } = args;
        if let Some(a) = <(P, S) as ArgShape<A>>::into_value(head) {
            if let (Ok(name), Ok(value)) = (
                HeaderName::from_str(&self.name),
                HeaderValue::from_str(&a.to_header()),
            ) {
                req.add_header(name, value);
            }
        }
        self.next.build_request(tail, req)
    }
    forward_decode!();
}

impl<CTypes, A, S, Next> HasClient for ReqBody<CTypes, A, S, Next>
where
    CTypes: AllMime + AllMimeRender<A>,
    (Required, S): ArgShape<A>,
    Next: HasClient,
    Self: Endpoint<
            Args = HCons<<(Required, S) as ArgShape<A>>::Out, Next::Args>,
            Output = Next::Output,
        >,
{
    fn build_request(&self, args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        let HCons { head, tail } = args;
        if let Some(a) = <(Required, S) as ArgShape<A>>::into_value(head) {
            // The request body is sent in the FIRST (primary) content type;
            // propagate a serialization failure as an error (never panic).
            let (mime, bytes) = CTypes::render_primary(&a)?;
            req.set_body(bytes, mime);
        }
        self.next.build_request(tail, req)
    }
    forward_decode!();
}

#[cfg(test)]
mod tests {
    use servant::api::{fragment, get, operation_id, path};
    use servant::content::Json;
    use servant::hlist::HNil;

    use super::*;

    #[test]
    fn operation_id_and_fragment_do_not_change_client_request_target() {
        let api = operation_id(
            "getArticle",
            path(
                "article",
                fragment::<String, _>("article section", get::<(Json,), String>()),
            ),
        );
        let mut req = ClientRequest::new();

        api.build_request(HNil, &mut req)
            .expect("client request builds");

        assert_eq!(req.method, http::Method::GET);
        assert_eq!(req.target(), "/article");
        assert!(req.query.is_empty());
    }
}

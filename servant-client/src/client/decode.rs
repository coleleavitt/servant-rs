use http::StatusCode;
use mime::Mime;
use servant::api::{Endpoint, NoContentVerb, UVerb, Verb, VerbWithHeaders};
use servant::content::{AllMime, AllMimeUnrender, NoContent};
use servant::method::MethodMarker;
use servant::uverb::UnionDecode;

use super::endpoint::HasClient;
use crate::request::{ClientError, ClientRequest, ClientResponse};

// --- Terminal: Verb ---

impl<M, const STATUS: u16, CTypes, A> HasClient for Verb<M, STATUS, CTypes, A>
where
    M: MethodMarker,
    CTypes: AllMime + AllMimeUnrender<A>,
    Self: Endpoint<Output = A, Args = servant::hlist::HNil>,
{
    fn build_request(&self, _args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        req.method = M::method();
        req.accept = CTypes::all_media_types();
        Ok(())
    }
    fn decode(&self, resp: ClientResponse) -> Result<Self::Output, ClientError> {
        let expected = StatusCode::from_u16(STATUS).expect("valid status");
        if resp.status != expected {
            return Err(ClientError::FailureResponse { response: resp });
        }
        decode_body::<CTypes, A>(resp)
    }
}

// --- Terminal: VerbWithHeaders ---

impl<M, const STATUS: u16, CTypes, A> HasClient for VerbWithHeaders<M, STATUS, CTypes, A>
where
    M: MethodMarker,
    CTypes: AllMime + AllMimeUnrender<A>,
    Self: Endpoint<Output = servant::response::Headers<A>, Args = servant::hlist::HNil>,
{
    fn build_request(&self, _args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        req.method = M::method();
        req.accept = CTypes::all_media_types();
        Ok(())
    }
    fn decode(&self, resp: ClientResponse) -> Result<Self::Output, ClientError> {
        let expected = StatusCode::from_u16(STATUS).expect("valid status");
        if resp.status != expected {
            return Err(ClientError::FailureResponse { response: resp });
        }
        // Capture the response headers, then decode the body into the value.
        let headers: Vec<(http::HeaderName, http::HeaderValue)> = resp
            .headers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let value = decode_body::<CTypes, A>(resp)?;
        let mut out = servant::response::Headers::new(value);
        for (k, v) in headers {
            out = out.header(k, v);
        }
        Ok(out)
    }
}

// --- Terminal: UVerb (union response, decoded by status) ---

impl<M, CTypes, Resp> HasClient for UVerb<M, CTypes, Resp>
where
    M: MethodMarker,
    CTypes: AllMime,
    Resp: UnionDecode<CTypes>,
    Self: Endpoint<Output = Resp, Args = servant::hlist::HNil>,
{
    fn build_request(&self, _args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        req.method = M::method();
        req.accept = CTypes::all_media_types();
        Ok(())
    }
    fn decode(&self, resp: ClientResponse) -> Result<Self::Output, ClientError> {
        let ct = resp
            .headers
            .get(http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<Mime>().ok());
        match Resp::decode_union(resp.status, &resp.headers, ct.as_ref(), &resp.body) {
            Some(Ok(v)) => Ok(v),
            Some(Err(message)) => Err(ClientError::DecodeFailure {
                message,
                response: resp,
            }),
            None => Err(ClientError::FailureResponse { response: resp }),
        }
    }
}

// --- Terminal: NoContentVerb ---

impl<M> HasClient for NoContentVerb<M>
where
    M: MethodMarker,
    Self: Endpoint<Output = NoContent, Args = servant::hlist::HNil>,
{
    fn build_request(&self, _args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        req.method = M::method();
        Ok(())
    }
    fn decode(&self, resp: ClientResponse) -> Result<Self::Output, ClientError> {
        if resp.status != StatusCode::NO_CONTENT {
            return Err(ClientError::FailureResponse { response: resp });
        }
        Ok(NoContent)
    }
}

/// Decode a response body for content-type list `L` into `A`, distinguishing the
/// Servant `ClientError` variants. A missing `Content-Type` defaults to
/// `application/octet-stream` (client-side default, separate from the server's).
fn decode_body<L: AllMimeUnrender<A>, A>(resp: ClientResponse) -> Result<A, ClientError> {
    let ct: Mime = match resp.headers.get(http::header::CONTENT_TYPE) {
        None => mime::APPLICATION_OCTET_STREAM,
        Some(v) => match v.to_str().ok().and_then(|s| s.parse::<Mime>().ok()) {
            Some(m) => m,
            None => return Err(ClientError::InvalidContentTypeHeader { response: resp }),
        },
    };
    match L::unrender(&ct, &resp.body) {
        Some(Ok(v)) => Ok(v),
        Some(Err(message)) => Err(ClientError::DecodeFailure {
            message,
            response: resp,
        }),
        None => Err(ClientError::UnsupportedContentType {
            media_type: ct,
            response: resp,
        }),
    }
}

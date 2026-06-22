use std::sync::Arc;

use http::HeaderValue;
use servant::api::{Description, Endpoint, Fragment, Host, OperationId, Path, Raw, RawM, Summary};
use servant::hlist::HNil;
use servant::host::HostRequirement;

use crate::request::{ClientError, ClientRequest, ClientResponse};
use crate::runclient::RunClient;

/// The client interpretation of a single endpoint chain.
pub trait HasClient: Endpoint {
    /// Build the request by consuming the argument list in combinator order.
    /// Fails only if a request body cannot be encoded into its content type.
    fn build_request(&self, args: Self::Args, req: &mut ClientRequest) -> Result<(), String>;
    /// Decode the response into the endpoint's output (checking status + type).
    fn decode(&self, resp: ClientResponse) -> Result<Self::Output, ClientError>;
}

// --- The client tree ---

/// A callable client for one endpoint.
pub struct ClientEndpoint<Api> {
    pub(crate) chain: Arc<Api>,
}

impl<Api> ClientEndpoint<Api>
where
    Api: HasClient,
{
    /// Execute this endpoint over `transport` with the given argument list.
    pub async fn call<T: RunClient>(
        &self,
        transport: &T,
        args: Api::Args,
    ) -> Result<Api::Output, ClientError> {
        let mut req = ClientRequest::new();
        self.chain
            .build_request(args, &mut req)
            .map_err(|message| ClientError::EncodeFailure { message })?;
        if req.has_streaming_body() && !transport.supports_streaming_request_body() {
            return Err(ClientError::StreamingRequestUnsupported);
        }
        let resp = transport.run_request(req).await?;
        self.chain.decode(resp)
    }
}

/// A callable generated client for `Raw` and `RawM` terminal endpoints.
pub struct RawClientEndpoint<Api> {
    pub(crate) chain: Arc<Api>,
}

/// Client-side request building for opaque raw terminal chains.
pub trait HasRawClient {
    /// Arguments consumed by combinators before the raw terminal.
    type Args;

    /// Build the request prefix before the caller-selected method is applied.
    fn build_raw_request(&self, args: Self::Args, req: &mut ClientRequest) -> Result<(), String>;
}

impl<Api> RawClientEndpoint<Api>
where
    Api: HasRawClient<Args = HNil>,
{
    /// Execute a raw request with the selected method and return the response
    /// without status or content-type decoding.
    pub async fn call_raw<T: RunClient>(
        &self,
        transport: &T,
        method: http::Method,
    ) -> Result<ClientResponse, ClientError> {
        self.call_raw_with(transport, HNil, method).await
    }
}

impl<Api> RawClientEndpoint<Api>
where
    Api: HasRawClient,
{
    /// Execute a raw request with explicit pre-terminal arguments.
    pub async fn call_raw_with<T: RunClient>(
        &self,
        transport: &T,
        args: Api::Args,
        method: http::Method,
    ) -> Result<ClientResponse, ClientError> {
        let mut req = ClientRequest::new();
        self.chain
            .build_raw_request(args, &mut req)
            .map_err(|message| ClientError::EncodeFailure { message })?;
        req.method = method;
        transport.run_request(req).await
    }
}

impl HasRawClient for Raw {
    type Args = HNil;

    fn build_raw_request(&self, _args: Self::Args, _req: &mut ClientRequest) -> Result<(), String> {
        Ok(())
    }
}

impl HasRawClient for RawM {
    type Args = HNil;

    fn build_raw_request(&self, _args: Self::Args, _req: &mut ClientRequest) -> Result<(), String> {
        Ok(())
    }
}

impl<Next> HasRawClient for Path<Next>
where
    Next: HasRawClient,
{
    type Args = Next::Args;

    fn build_raw_request(&self, args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        req.append_path(&self.segment);
        self.next.build_raw_request(args, req)
    }
}

impl<Next> HasRawClient for Host<Next>
where
    Next: HasRawClient,
{
    type Args = Next::Args;

    fn build_raw_request(&self, args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        let required = HostRequirement::parse(self.name.as_str())
            .map_err(|err| format!("{err}: `{}`", self.name))?;
        let value = HeaderValue::from_str(required.as_str()).map_err(|err| err.to_string())?;
        req.headers.insert(http::header::HOST, value);
        self.next.build_raw_request(args, req)
    }
}

macro_rules! metadata_raw_client {
    ($ty:ident < $($g:ident),+ >) => {
        impl<$($g),+, Next> HasRawClient for $ty<$($g),+, Next>
        where
            Next: HasRawClient,
        {
            type Args = Next::Args;

            fn build_raw_request(
                &self,
                args: Self::Args,
                req: &mut ClientRequest,
            ) -> Result<(), String> {
                self.next.build_raw_request(args, req)
            }
        }
    };
    ($ty:ident) => {
        impl<Next> HasRawClient for $ty<Next>
        where
            Next: HasRawClient,
        {
            type Args = Next::Args;

            fn build_raw_request(
                &self,
                args: Self::Args,
                req: &mut ClientRequest,
            ) -> Result<(), String> {
                self.next.build_raw_request(args, req)
            }
        }
    };
}

metadata_raw_client!(Description);
metadata_raw_client!(Summary);
metadata_raw_client!(OperationId);
metadata_raw_client!(Fragment<A>);

/// Build a typed client value from an API description: a [`ClientEndpoint`] for a
/// single endpoint, or a nested tuple mirroring the
/// [`Alt`](servant::api::Alt) structure.
pub trait MakeClient {
    /// The resulting client value.
    type Client;
    /// Construct it.
    fn make_client(self) -> Self::Client;
}

/// Build a typed client from an API description.
pub fn client<Api: MakeClient>(api: Api) -> Api::Client {
    api.make_client()
}

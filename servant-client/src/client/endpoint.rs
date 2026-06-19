use std::sync::Arc;

use servant::api::{Alt, Endpoint};

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
        let resp = transport.run_request(req).await?;
        self.chain.decode(resp)
    }
}

/// Build a typed client value from an API description: a [`ClientEndpoint`] for a
/// single endpoint, or a nested tuple mirroring the [`Alt`] structure.
pub trait MakeClient {
    /// The resulting client value.
    type Client;
    /// Construct it.
    fn make_client(self) -> Self::Client;
}

impl<Api> MakeClient for Api
where
    Api: HasClient,
{
    type Client = ClientEndpoint<Api>;
    fn make_client(self) -> Self::Client {
        ClientEndpoint {
            chain: Arc::new(self),
        }
    }
}

impl<L, R> MakeClient for Alt<L, R>
where
    L: MakeClient,
    R: MakeClient,
{
    type Client = (L::Client, R::Client);
    fn make_client(self) -> Self::Client {
        (self.left.make_client(), self.right.make_client())
    }
}

/// Build a typed client from an API description.
pub fn client<Api: MakeClient>(api: Api) -> Api::Client {
    api.make_client()
}

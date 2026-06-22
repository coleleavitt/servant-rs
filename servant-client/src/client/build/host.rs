use http::HeaderValue;
use servant::api::{Endpoint, Host};
use servant::host::HostRequirement;

use super::super::endpoint::HasClient;
use crate::request::{ClientError, ClientRequest, ClientResponse};

impl<Next: HasClient> HasClient for Host<Next>
where
    Self: Endpoint<Output = Next::Output, Args = Next::Args>,
{
    fn build_request(&self, args: Self::Args, req: &mut ClientRequest) -> Result<(), String> {
        let required = HostRequirement::parse(self.name.as_str())
            .map_err(|err| format!("{err}: `{}`", self.name))?;
        let value = HeaderValue::from_str(required.as_str()).map_err(|err| err.to_string())?;
        req.headers.insert(http::header::HOST, value);
        self.next.build_request(args, req)
    }

    fn decode(&self, resp: ClientResponse) -> Result<Self::Output, ClientError> {
        self.next.decode(resp)
    }
}

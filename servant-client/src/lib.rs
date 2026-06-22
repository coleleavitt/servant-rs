//! `servant-client` — typed HTTP clients generated from a servant-rs API.
//!
//! [`client()`] walks the **same** [`servant`] API description the server routes
//! on, producing a [`ClientEndpoint`] per endpoint (or a nested tuple mirroring
//! `Alt`). Calling an endpoint builds a [`ClientRequest`] from the same `Args`
//! HList the server consumes — so client and server can never disagree about an
//! endpoint's shape — runs it over a pluggable [`RunClient`] transport, and
//! decodes the response, enforcing the declared status and content types.
//!
//! ```no_run
//! use servant::hlist::hlist1;
//! use servant::prelude::*;
//! use servant_client::{BaseUrl, HyperClient, client};
//!
//! # async fn ex() -> Result<(), Box<dyn std::error::Error>> {
//! // "users" :> Capture "id" u64 :> Get '[JSON] u64
//! let api = path("users", capture::<u64, _>("id", get::<(Json,), u64>()));
//! let endpoint = client(api);
//! let transport = HyperClient::new(BaseUrl::http("localhost", 8080));
//! let user = endpoint.call(&transport, hlist1(42u64)).await?;
//! # let _ = user; Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]

pub mod client;
pub mod request;
pub mod runclient;

pub use client::{ClientEndpoint, HasClient, HasRawClient, MakeClient, RawClientEndpoint, client};
pub use request::{
    BaseUrl,
    ClientError,
    ClientRequest,
    ClientResponse,
    RequestByteStream,
    Scheme,
    StreamingRequestBody,
};
#[cfg(feature = "hyper")]
pub use runclient::HyperClient;
pub use runclient::{RunClient, RunStreamingClient, StreamingResponse};

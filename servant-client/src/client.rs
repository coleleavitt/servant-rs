//! The client interpretation: walk the API description to build a request from
//! the handler argument list, then decode the response.
//!
//! Mirrors `Servant.Client.Core.HasClient`. `build_request` consumes the same
//! `Args` HList the server produces (so client and server cannot drift) and the
//! decode step enforces the endpoint's declared status and content types.

mod build;
mod decode;
mod endpoint;
mod streaming;

pub use endpoint::{ClientEndpoint, HasClient, MakeClient, client};
pub use streaming::StreamInfo;

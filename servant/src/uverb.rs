//! Union responses, mirroring `Servant.API.UVerb`.
//!
//! A `UVerb` endpoint can return one of several response types, each with its
//! own status code. The handler returns a union value ([`Union2`]/[`Union3`]/
//! [`Union4`]) whose arms are [`WithStatus`]-tagged values; the server renders
//! the active arm (negotiating its body over the endpoint's content types) with
//! that arm's status, and the client decodes by matching the response status.
//! `MultiVerb`-style fixed-content, empty, per-arm-header, and streaming arms
//! are represented by the `With*Status*` helpers below.

mod arms;
mod decode;
mod render;
mod unions;

pub use arms::{
    ArmHeaders,
    HeaderError,
    WithFixedStatus,
    WithStatus,
    WithStatusHeaders,
    WithStatusNoBody,
    WithStreamingStatus,
};
pub use decode::UnionDecode;
pub use render::{ArmBody, ArmResponse, UnionResponse};
pub use unions::{Union2, Union3, Union4};

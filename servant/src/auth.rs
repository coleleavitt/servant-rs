//! Authentication primitives, mirroring `Servant.API.BasicAuth`.
//!
//! `BasicAuth` is a **server-side** combinator: the server reads the
//! `Authorization: Basic …` header, runs a check function supplied via the
//! server `Context`, and passes the authenticated user to the handler (or
//! rejects with `401`). Because the server's handler argument (the *user*)
//! differs in type from what a client would send (*credentials*), and this
//! crate shares one argument list between server and client, `BasicAuth`
//! endpoints are server + docs + links only — they are not part of a generated
//! typed client. **\[diff\]**

/// Decoded `Basic` credentials from the `Authorization` header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BasicAuthData {
    /// The username (the part before `:`).
    pub username: String,
    /// The password (the part after `:`).
    pub password: String,
}

/// Outcome of a basic-auth check, mirroring Servant's `BasicAuthResult`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BasicAuthResult<Usr> {
    /// Credentials are valid; the resolved user.
    Authorized(Usr),
    /// The user exists but the password is wrong → 401.
    BadPassword,
    /// No such user → 401.
    NoSuchUser,
    /// Otherwise unauthorized → 401.
    Unauthorized,
}

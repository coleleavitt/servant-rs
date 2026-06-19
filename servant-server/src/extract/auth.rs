use base64::Engine;
use servant::api::{AuthProtect, BasicAuth, Endpoint};
use servant::auth::{BasicAuthData, BasicAuthResult};
use servant::error::ServerError;
use servant::hlist::HCons;

use super::chain::{Rendered, ServerChain, cons_tail};
use super::state::ExtractState;
use crate::result::RouteResult;

// --- BasicAuth (server-only; resolves Usr from the Authorization header) ---

fn parse_basic_auth(header: &str) -> Option<BasicAuthData> {
    let rest = header
        .strip_prefix("Basic ")
        .or_else(|| header.strip_prefix("basic "))?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(rest.trim())
        .ok()?;
    let s = String::from_utf8(decoded).ok()?;
    let (username, password) = s.split_once(':')?;
    Some(BasicAuthData {
        username: username.to_owned(),
        password: password.to_owned(),
    })
}

impl<Usr, Next> ServerChain for BasicAuth<Usr, Next>
where
    Usr: Send + Sync + 'static,
    Next: ServerChain,
    Self: Endpoint<Args = HCons<Usr, Next::Args>, Output = Next::Output>,
{
    fn validate_captures(
        &self,
        c: &[String],
        i: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        self.next.validate_captures(c, i, ca)
    }
    forward_response_checks!();
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        let realm = self.realm.replace('"', "");
        let unauthorized = || {
            let mut e = ServerError::err401();
            if let Ok(v) = http::HeaderValue::from_str(&format!("Basic realm=\"{realm}\"")) {
                e = e.with_header(http::header::WWW_AUTHENTICATE, v);
            }
            RouteResult::FailFatal(e)
        };

        let Some(check) = st.lookup_ctx::<crate::context::BasicAuthCheck<Usr>>() else {
            return RouteResult::FailFatal(
                ServerError::err500().with_body("basic-auth check not configured in context"),
            );
        };
        let Some(data) = st
            .req
            .headers
            .get(http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_basic_auth)
        else {
            return unauthorized();
        };

        match (check.0)(&data) {
            BasicAuthResult::Authorized(usr) => cons_tail(usr, &self.next, st),
            BasicAuthResult::BadPassword
            | BasicAuthResult::NoSuchUser
            | BasicAuthResult::Unauthorized => unauthorized(),
        }
    }
}

// --- AuthProtect (generalized auth) ---

impl<Usr, Next> ServerChain for AuthProtect<Usr, Next>
where
    Usr: Send + Sync + 'static,
    Next: ServerChain,
    Self: Endpoint<Args = HCons<Usr, Next::Args>, Output = Next::Output>,
{
    fn validate_captures(
        &self,
        c: &[String],
        i: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        self.next.validate_captures(c, i, ca)
    }
    forward_response_checks!();
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        let Some(check) = st.lookup_ctx::<crate::context::AuthCheck<Usr>>() else {
            return RouteResult::FailFatal(
                ServerError::err500().with_body("auth check not configured in context"),
            );
        };
        match (check.0)(&st.req.headers) {
            Ok(usr) => cons_tail(usr, &self.next, st),
            Err(e) => RouteResult::FailFatal(e),
        }
    }
}

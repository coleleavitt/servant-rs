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

fn unauthorized_basic(realm: &str) -> ServerError {
    let realm = realm.replace('"', "");
    let mut error = ServerError::err401();
    if let Ok(value) = http::HeaderValue::from_str(&format!("Basic realm=\"{realm}\"")) {
        error = error.with_header(http::header::WWW_AUTHENTICATE, value);
    }
    error
}

fn basic_auth_user<Usr>(st: &ExtractState<'_>, realm: &str) -> Result<Usr, ServerError>
where
    Usr: Send + Sync + 'static,
{
    let Some(check) = st.lookup_ctx::<crate::context::BasicAuthCheck<Usr>>() else {
        return Err(ServerError::err500().with_body("basic-auth check not configured in context"));
    };
    let Some(data) = st
        .req
        .headers
        .get(http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_basic_auth)
    else {
        return Err(unauthorized_basic(realm));
    };

    match (check.0)(&data) {
        BasicAuthResult::Authorized(user) => Ok(user),
        BasicAuthResult::BadPassword
        | BasicAuthResult::NoSuchUser
        | BasicAuthResult::Unauthorized => Err(unauthorized_basic(realm)),
    }
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
    forward_response_checks_without_pre_body!();

    fn pre_body_check(&self, st: &mut ExtractState<'_>) -> RouteResult<()> {
        match basic_auth_user::<Usr>(st, &self.realm) {
            Ok(user) => {
                st.push_prechecked(user);
                self.next.pre_body_check(st)
            }
            Err(error) => RouteResult::FailFatal(error),
        }
    }

    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        if let Some(usr) = st.take_prechecked::<Usr>() {
            return cons_tail(usr, &self.next, st);
        }
        match basic_auth_user::<Usr>(st, &self.realm) {
            Ok(usr) => cons_tail(usr, &self.next, st),
            Err(error) => RouteResult::FailFatal(error),
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
    forward_response_checks_without_pre_body!();

    fn pre_body_check(&self, st: &mut ExtractState<'_>) -> RouteResult<()> {
        let Some(check) = st.lookup_ctx::<crate::context::AuthCheck<Usr>>() else {
            return RouteResult::FailFatal(
                ServerError::err500().with_body("auth check not configured in context"),
            );
        };
        match (check.0)(&st.req.headers) {
            Ok(user) => {
                st.push_prechecked(user);
                self.next.pre_body_check(st)
            }
            Err(e) => RouteResult::FailFatal(e),
        }
    }

    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        if let Some(usr) = st.take_prechecked::<Usr>() {
            return cons_tail(usr, &self.next, st);
        }
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

use servant::prelude::*;
use servant_server::{BasicAuthCheck, Context, serve_with_context};

#[derive(Clone)]
struct AuthenticatedUser;

fn main() {
    let api = basic_auth::<AuthenticatedUser, _>("admin", get::<(PlainText,), String>());

    // Auth handlers receive the authenticated user type declared by `basic_auth`.
    let _router = serve_with_context(
        api,
        |name: String| async move { Ok::<_, ServerError>(format!("hello {name}")) },
        Context::new().with(BasicAuthCheck::new(|_data: &servant::auth::BasicAuthData| {
            servant::auth::BasicAuthResult::Authorized(AuthenticatedUser)
        })),
    );
}
//! The three-way routing result and the error-priority selection that mirrors
//! `Servant.Server.Internal.RouteResult` / `Router`.

use servant::error::ServerError;

/// The outcome of attempting to serve a request along one route.
#[derive(Debug)]
pub enum RouteResult<T> {
    /// The route handled the request (success *or* a handler-thrown error that
    /// is still a real response).
    Route(T),
    /// This route did not apply; recover by trying the next sibling.
    Fail(ServerError),
    /// This route applies but failed definitively; do not try siblings.
    FailFatal(ServerError),
}

/// Servant's `worseHTTPCode` priority: higher value wins when selecting the
/// "best" error among sibling `Fail`s. Hardcoded (never sort by numeric status).
///
/// `404 < 405 < 401 < 415 < 406 < (others) < 400`
pub fn priority(status: u16) -> u8 {
    match status {
        404 => 0, // not found
        405 => 1, // method not allowed
        401 => 2, // unauthorized
        415 => 3, // unsupported media type
        406 => 4, // not acceptable
        400 => 6, // bad request
        _ => 5,
    }
}

/// Combine the current best `Fail` with a newer one, keeping the higher
/// priority; ties keep `best` (left/earlier — left-biased like Servant).
pub fn combine_fail(best: Option<ServerError>, new: ServerError) -> ServerError {
    match best {
        None => new,
        Some(b) => {
            if priority(new.code()) > priority(b.code()) {
                new
            } else {
                b
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_table_matches_servant() {
        // 400 beats everything; 404 loses to everything.
        assert!(priority(400) > priority(406));
        assert!(priority(406) > priority(415));
        assert!(priority(415) > priority(401));
        assert!(priority(401) > priority(405));
        assert!(priority(405) > priority(404));
        assert!(priority(400) > priority(500)); // others = 5 < 6
        assert!(priority(500) > priority(406)); // others = 5 > 4
    }

    #[test]
    fn combine_keeps_higher_priority_then_left() {
        let m405 = ServerError::err405();
        let b400 = ServerError::err400();
        // 400 beats 405 regardless of order
        assert_eq!(combine_fail(Some(m405.clone()), b400.clone()).code(), 400);
        assert_eq!(combine_fail(Some(b400.clone()), m405.clone()).code(), 400);
        // tie keeps left (earlier `best`)
        let a = ServerError::err400().with_reason("A");
        let b = ServerError::err400().with_reason("B");
        assert_eq!(combine_fail(Some(a), b).reason(), "A");
    }
}

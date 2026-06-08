//! The runtime routing tree and its interpreter, mirroring
//! `Servant.Server.Internal.Router`.
//!
//! The tree matches the request *path* (collecting raw captured segments into an
//! environment); the per-endpoint extraction/handler logic lives behind a
//! [`Leaf`] that runs once the path is fully consumed. Raw capture segments are
//! parsed at the leaf (not the node) so that the [`choice`] smart-constructor
//! can merge sibling captures of different types, exactly as Servant does.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use servant::error::ServerError;

use crate::request::RequestData;
use crate::response::ResponseBody;
use crate::result::{RouteResult, combine_fail};

/// A boxed future producing a routing result for one candidate.
pub type BoxRouteFuture<'a> =
    Pin<Box<dyn Future<Output = RouteResult<http::Response<ResponseBody>>> + Send + 'a>>;

/// The per-endpoint logic invoked once the router has matched the path.
pub trait LeafService: Send + Sync {
    /// Run the endpoint with the raw captured segments collected during routing.
    fn call<'a>(
        &'a self,
        req: &'a RequestData,
        captures: Vec<String>,
        capture_all: Option<Vec<String>>,
    ) -> BoxRouteFuture<'a>;
}

/// A shared, type-erased endpoint leaf.
pub type Leaf = Arc<dyn LeafService>;

/// Debug information about a captured path variable (name + Rust type).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureHint {
    /// The capture's documentation name.
    pub name: String,
    /// The captured value's Rust type name.
    pub type_name: &'static str,
}

/// The routing tree. Structurally identical to Servant's `Router'`.
pub enum Router {
    /// Map from next path component to sub-router, plus empty-path leaves tried
    /// in order.
    Static(BTreeMap<String, Router>, Vec<Leaf>),
    /// Consume one path segment into the capture environment.
    Capture(Vec<CaptureHint>, Box<Router>),
    /// Consume all remaining path segments into the capture environment.
    CaptureAll(Vec<CaptureHint>, Box<Router>),
    /// An opaque handler that owns the rest of the path.
    Raw(Leaf),
    /// Left-biased choice between two routers.
    Choice(Box<Router>, Box<Router>),
}

impl Router {
    /// A leaf router expecting the empty path.
    pub fn leaf(l: Leaf) -> Router {
        Router::Static(BTreeMap::new(), vec![l])
    }

    /// A single static path component wrapping a sub-router.
    pub fn path(segment: impl Into<String>, inner: Router) -> Router {
        let mut m = BTreeMap::new();
        m.insert(segment.into(), inner);
        Router::Static(m, Vec::new())
    }
}

/// Servant's `choice` smart-constructor: merge static maps and capture routers,
/// reassociate nested choices, otherwise build a `Choice` node.
pub fn choice(a: Router, b: Router) -> Router {
    match (a, b) {
        (Router::Static(mut t1, mut l1), Router::Static(t2, l2)) => {
            for (k, r2) in t2 {
                match t1.remove(&k) {
                    Some(r1) => {
                        t1.insert(k, choice(r1, r2));
                    }
                    None => {
                        t1.insert(k, r2);
                    }
                }
            }
            l1.extend(l2);
            Router::Static(t1, l1)
        }
        (Router::Capture(mut h1, r1), Router::Capture(h2, r2)) => {
            for h in h2 {
                if !h1.contains(&h) {
                    h1.push(h);
                }
            }
            Router::Capture(h1, Box::new(choice(*r1, *r2)))
        }
        (r1, Router::Choice(r2, r3)) => Router::Choice(Box::new(choice(r1, *r2)), r3),
        (r1, r2) => Router::Choice(Box::new(r1), Box::new(r2)),
    }
}

fn is_empty_path(segments: &[String]) -> bool {
    segments.is_empty() || (segments.len() == 1 && segments[0].is_empty())
}

/// The default 404 used when no route matches.
pub fn not_found() -> ServerError {
    ServerError::err404()
}

/// Interpret a router for a request, returning the chosen route's result.
pub fn dispatch<'a>(
    router: &'a Router,
    segments: &'a [String],
    captures: Vec<String>,
    capture_all: Option<Vec<String>>,
    req: &'a RequestData,
) -> BoxRouteFuture<'a> {
    Box::pin(async move {
        match router {
            Router::Static(map, leaves) => {
                if is_empty_path(segments) {
                    run_choice_leaves(leaves, req, captures, capture_all).await
                } else {
                    let (first, rest) = segments.split_first().expect("non-empty");
                    match map.get(first) {
                        Some(inner) => dispatch(inner, rest, captures, capture_all, req).await,
                        None => RouteResult::Fail(not_found()),
                    }
                }
            }
            Router::Capture(_, inner) => {
                if is_empty_path(segments) {
                    RouteResult::Fail(not_found())
                } else {
                    let (first, rest) = segments.split_first().expect("non-empty");
                    let mut caps = captures;
                    caps.push(first.clone());
                    dispatch(inner, rest, caps, capture_all, req).await
                }
            }
            Router::CaptureAll(_, inner) => {
                // Drop a leading empty segment (trailing-slash artifact).
                let segs: Vec<String> = if !segments.is_empty() && segments[0].is_empty() {
                    segments[1..].to_vec()
                } else {
                    segments.to_vec()
                };
                dispatch(inner, &[], captures, Some(segs), req).await
            }
            Router::Raw(leaf) => leaf.call(req, captures, capture_all).await,
            Router::Choice(a, b) => {
                match dispatch(a, segments, captures.clone(), capture_all.clone(), req).await {
                    RouteResult::Route(r) => RouteResult::Route(r),
                    RouteResult::FailFatal(e) => RouteResult::FailFatal(e),
                    RouteResult::Fail(e1) => {
                        match dispatch(b, segments, captures, capture_all, req).await {
                            RouteResult::Route(r) => RouteResult::Route(r),
                            RouteResult::FailFatal(e) => RouteResult::FailFatal(e),
                            RouteResult::Fail(e2) => RouteResult::Fail(combine_fail(Some(e1), e2)),
                        }
                    }
                }
            }
        }
    })
}

async fn run_choice_leaves(
    leaves: &[Leaf],
    req: &RequestData,
    captures: Vec<String>,
    capture_all: Option<Vec<String>>,
) -> RouteResult<http::Response<ResponseBody>> {
    let mut best: Option<ServerError> = None;
    for leaf in leaves {
        match leaf.call(req, captures.clone(), capture_all.clone()).await {
            RouteResult::Route(r) => return RouteResult::Route(r),
            RouteResult::FailFatal(e) => return RouteResult::FailFatal(e),
            RouteResult::Fail(e) => best = Some(combine_fail(best, e)),
        }
    }
    RouteResult::Fail(best.unwrap_or_else(not_found))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choice_merges_static_maps() {
        let a = Router::path("users", Router::leaf(dummy_leaf()));
        let b = Router::path("posts", Router::leaf(dummy_leaf()));
        match choice(a, b) {
            Router::Static(map, leaves) => {
                assert_eq!(map.len(), 2);
                assert!(map.contains_key("users"));
                assert!(map.contains_key("posts"));
                assert!(leaves.is_empty());
            }
            _ => panic!("expected merged Static"),
        }
    }

    #[test]
    fn choice_concatenates_leaves_under_same_path() {
        let a = Router::path("users", Router::leaf(dummy_leaf()));
        let b = Router::path("users", Router::leaf(dummy_leaf()));
        match choice(a, b) {
            Router::Static(map, _) => match map.get("users").unwrap() {
                Router::Static(_, leaves) => assert_eq!(leaves.len(), 2),
                _ => panic!(),
            },
            _ => panic!(),
        }
    }

    fn dummy_leaf() -> Leaf {
        struct D;
        impl LeafService for D {
            fn call<'a>(
                &'a self,
                _req: &'a RequestData,
                _c: Vec<String>,
                _ca: Option<Vec<String>>,
            ) -> BoxRouteFuture<'a> {
                Box::pin(async { RouteResult::Fail(not_found()) })
            }
        }
        Arc::new(D)
    }
}

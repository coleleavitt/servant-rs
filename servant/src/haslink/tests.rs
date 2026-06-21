use super::*;
use crate::api::{alt, capture, fragment, get, path, query_flag, query_param};
use crate::content::Json;
use crate::hlist::{HCons, HNil, hlist1};

#[test]
fn builds_link_for_capture_endpoint() {
    let api = path("users", capture::<u64, _>("id", get::<(Json,), u64>()));
    let ep = links(api);
    let link = ep.link(hlist1(42u64));
    assert_eq!(link.to_uri(), "/users/42");
}

#[test]
fn builds_link_with_query_and_flag() {
    let api = path(
        "search",
        query_param::<String, _>("q", query_flag("verbose", get::<(Json,), u64>())),
    );
    let ep = links(api);
    let link = ep.link(HCons {
        head: Some("rust".to_string()),
        tail: hlist1(true),
    });
    assert_eq!(link.to_uri(), "/search?q=rust&verbose");

    let ep2 = links(path(
        "search",
        query_param::<String, _>("q", query_flag("verbose", get::<(Json,), u64>())),
    ));
    let link2 = ep2.link(HCons {
        head: None,
        tail: hlist1(false),
    });
    assert_eq!(link2.to_uri(), "/search");
}

#[test]
fn alt_produces_a_link_builder_per_endpoint() {
    let api = alt(
        path("a", get::<(Json,), u64>()),
        path("b", capture::<u64, _>("n", get::<(Json,), u64>())),
    );
    let (a, b) = links(api);
    assert_eq!(a.link(HNil).to_uri(), "/a");
    assert_eq!(b.link(hlist1(7u64)).to_uri(), "/b/7");
}

#[test]
fn fragment_safe_link_renders() {
    let api = path(
        "article",
        fragment::<String, _>("article section", get::<(Json,), u64>()),
    );
    let ep = links(api);
    let link = ep.link(hlist1("intro".to_string()));
    assert_eq!(link.to_uri(), "/article#intro");
}

#[test]
fn fragment_safe_link_escapes_fragment_marker() {
    let api = path(
        "article",
        fragment::<String, _>("article section", get::<(Json,), u64>()),
    );
    let ep = links(api);
    let link = ep.link(hlist1("intro#details".to_string()));
    assert_eq!(link.to_uri(), "/article#intro%23details");
}
